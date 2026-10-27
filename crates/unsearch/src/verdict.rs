//! `invectus check`: does an idea already exist in a collection? (SPEC §6.1)
//!
//! The query is embedded with the collection's model, compared exactly against every stored
//! vector by default (an absence claim must not rest on approximate retrieval), and the nearest
//! similarity is mapped to P(exists) with the collection's query-level calibration curve. When
//! the collection has regions, the two nearest fine regions say where the idea sits.

use crate::curve::Curve;
use anyhow::{Context, Result};
use invectus_core::store::{CalibrationRecord, VectorTableRecord};
use invectus_core::{Project, Role, TextEmbedder};
use invectus_index::{SearchMode, SearchOptions, SearchResult, VectorStore, search};
use invectus_regions::{NearRegion, nearest_regions};
use serde::Serialize;

/// P(exists) at or above this is `exists`.
pub const EXISTS_AT: f64 = 0.7;
/// P(exists) at or above this (and below [`EXISTS_AT`]) is `near`; below it is `open`.
pub const NEAR_AT: f64 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Exists,
    Near,
    Open,
}

impl Verdict {
    pub fn from_p(p: f64) -> Self {
        if p >= EXISTS_AT {
            Self::Exists
        } else if p >= NEAR_AT {
            Self::Near
        } else {
            Self::Open
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exists => "exists",
            Self::Near => "near",
            Self::Open => "open",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CheckOptions {
    /// Nearest items returned as evidence (density uses the top 10).
    pub k: usize,
    /// Compare only against items with this role (usually coverage).
    pub role: Option<Role>,
    /// Exhaustive by default; ANN is allowed but flagged.
    pub mode: SearchMode,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            k: 10,
            role: None,
            mode: SearchMode::Exhaustive,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CalibrationInfo {
    pub method: String,
    pub sample: i64,
    pub fitted_at: Option<String>,
    pub holdout_auc: Option<f64>,
    /// The collection changed since fitting (different vector count).
    pub stale: bool,
    /// Plain-language caveat about how much to trust P(exists).
    pub quality: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckResult {
    pub verdict: Verdict,
    /// Calibrated chance the idea is already in the collection, from the nearest similarity.
    /// It is a query-level estimate; individual items have no probability of their own.
    pub p_exists: f64,
    pub summary: String,
    pub next_step: String,
    /// Similarity of the nearest item.
    pub nearest_similarity: Option<f32>,
    /// Drop in similarity from the 1st to the 10th nearest item: a large drop means one item
    /// stands out, a small one a crowded neighbourhood.
    pub drop_to_10th: Option<f32>,
    /// Mean similarity of the 10 nearest items.
    pub local_density: Option<f32>,
    /// The two fine regions whose centroids are closest to the idea (needs `invectus regions`).
    pub between: Vec<NearRegion>,
    /// "Your idea sits between …" when regions exist.
    pub position: Option<String>,
    pub calibration: CalibrationInfo,
    pub search: SearchResult,
    pub notes: Vec<String>,
}

/// The latest calibration of a table, or an error that says how to make one.
pub fn calibration_for(project: &Project, table: &VectorTableRecord) -> Result<CalibrationRecord> {
    project
        .store
        .latest_calibration(&table.collection_id, &table.space_id)?
        .with_context(|| {
            format!(
                "collection {:?} has no calibration for space {} ({}); run `invectus calibrate -c {}`",
                table.collection_id,
                &table.space_id[..12],
                table.identity.model_key,
                table.collection_id
            )
        })
}

pub fn check(
    project: &Project,
    vectors: &VectorStore,
    table: &VectorTableRecord,
    encoder: &mut dyn TextEmbedder,
    query: &str,
    opts: &CheckOptions,
) -> Result<CheckResult> {
    let cal = calibration_for(project, table)?;
    let curve: Curve = serde_json::from_value(cal.curve.clone())
        .context("stored calibration curve is unreadable; re-run `invectus calibrate`")?;
    let mut search = search(
        project,
        vectors,
        table,
        encoder,
        query,
        &SearchOptions {
            k: opts.k.max(10),
            role: opts.role,
            mode: opts.mode,
        },
    )?;
    let mut notes: Vec<String> = std::mem::take(&mut search.notes)
        .into_iter()
        .filter(|n| !n.contains("not a calibrated"))
        .collect();
    // An exhaustive scan reads every stored vector (the role filter applies after reading).
    let stale = search.exhaustive && cal.vectors != search.vectors_compared as i64;
    let sims: Vec<f32> = search.hits.iter().map(|h| h.similarity).collect();
    let top10 = &sims[..sims.len().min(10)];
    let p_exists = sims
        .first()
        .map_or(curve.predict(-1.0), |&s| curve.predict(s));
    let verdict = Verdict::from_p(p_exists);
    let scope = match opts.role {
        Some(r) => format!("the {} items of {}", r.as_str(), table.collection_id),
        None => table.collection_id.clone(),
    };
    let summary = match (verdict, search.hits.first()) {
        (_, None) => format!("There is nothing in {scope} to compare against."),
        (Verdict::Exists, Some(h)) => format!(
            "Something very close is already in {scope}: {:?} (similarity {:.3}).",
            h.item.title.as_deref().unwrap_or(&h.item.source_id),
            h.similarity
        ),
        (Verdict::Near, Some(_)) => {
            format!("Related items exist in {scope}, but none is clearly the same thing.")
        }
        (Verdict::Open, Some(_)) => {
            format!("Not found in {scope}: nothing there looks like the same thing.")
        }
    };
    let next_step = match verdict {
        Verdict::Exists => "Read the closest match: it probably covers this idea.",
        Verdict::Near => "Read the top matches to see how they differ from your idea.",
        Verdict::Open => {
            "This covers only this collection. Check outside sources before relying on it."
        }
    }
    .to_string();
    if !search.exhaustive && verdict != Verdict::Exists {
        notes.push(
            "approximate retrieval was used; an exhaustive check can find a closer item".into(),
        );
    }
    if stale {
        notes.push(format!(
            "the collection changed since calibration ({} vectors then, {} now); re-run \
             `invectus calibrate`",
            cal.vectors, search.vectors_compared
        ));
    }
    if opts.role.is_some() {
        notes.push(
            "the calibration was fitted on the whole collection; restricted to one role, the \
             nearest item tends to be farther, so P(exists) is approximate"
                .into(),
        );
    }
    let between = match project
        .store
        .latest_region_run(&table.collection_id, &table.space_id)?
    {
        Some(run) => nearest_regions(
            &project.store.regions(&run.run_id)?,
            &search.query_vector,
            "fine",
            2,
        ),
        None => {
            notes.push("run `invectus regions` to see which regions the idea sits between".into());
            Vec::new()
        }
    };
    let position = match between.as_slice() {
        [a, b] => Some(format!(
            "Your idea sits between \"{}\" and \"{}\".",
            a.label, b.label
        )),
        [a] => Some(format!("Your idea sits in \"{}\".", a.label)),
        _ => None,
    };
    let holdout_auc = cal.metrics["holdout"]["auc"].as_f64();
    let quality = if cal.method.contains("no LLM") {
        format!(
            "Approximate: calibrated without a language model on {} titled items (title vs own \
             text){}.",
            cal.sample,
            holdout_auc
                .map(|a| format!("; held-out AUC {a:.3}"))
                .unwrap_or_default()
        )
    } else {
        format!("Calibrated with {} on {} items.", cal.method, cal.sample)
    };
    Ok(CheckResult {
        verdict,
        p_exists,
        summary,
        next_step,
        nearest_similarity: sims.first().copied(),
        drop_to_10th: (top10.len() == 10).then(|| top10[0] - top10[9]),
        local_density: (!top10.is_empty()).then(|| top10.iter().sum::<f32>() / top10.len() as f32),
        between,
        position,
        calibration: CalibrationInfo {
            method: cal.method,
            sample: cal.sample,
            fitted_at: cal.created_at,
            holdout_auc,
            stale,
            quality,
        },
        search,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds() {
        assert_eq!(Verdict::from_p(0.95), Verdict::Exists);
        assert_eq!(Verdict::from_p(EXISTS_AT), Verdict::Exists);
        assert_eq!(Verdict::from_p(0.5), Verdict::Near);
        assert_eq!(Verdict::from_p(NEAR_AT), Verdict::Near);
        assert_eq!(Verdict::from_p(0.29), Verdict::Open);
    }
}
