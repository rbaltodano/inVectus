//! `invectus calibrate`: fit what similarity means "the same thing" for one collection and vector
//! space (SPEC §6.2), without a language model.
//!
//! The no-LLM fallback splits sampled items in two. The **title** plays the paraphrase: it is
//! embedded as a query, exactly as a user's idea would be, and the item's **text alone** (embedded
//! as a document without its title, so the two do not share words verbatim) plays the existing
//! item.
//!
//! The curve is fitted at the **query level**, because a verdict looks at the best match over a
//! whole collection, not at one pair (P0 spec recommendation 2). For each sampled title:
//! - *absent*: the best similarity to every **other** stored item, i.e. what a query sees when
//!   its idea is not in the collection;
//! - *exists*: the best of that and the similarity to the item's own text.
//!
//! Isotonic regression of exists/absent on that top similarity, with the classes weighted
//! equally, gives P(exists | nearest similarity) for this collection at its current size. A fifth
//! of the items are held out to score a curve fitted on the rest. As a diagnostic of the model,
//! the title is also compared with the item's 20th–50th nearest neighbours (SPEC §6.2's
//! related-but-different pairs).

use crate::curve::{Curve, Sample, auc, balanced_brier};
use anyhow::{Result, bail};
use invectus_core::job::ProgressFn;
use invectus_core::store::{CalibrationRecord, JobRecord, VectorTableRecord};
use invectus_core::{CancelToken, InputKind, Progress, Project, TextEmbedder};
use invectus_index::VectorStore;
use invectus_index::search::{check_compatible, collapse};
use invectus_index::vectors::cosine;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Instant;

pub const METHOD_TITLE_TEXT: &str = "title-vs-text split (no LLM)";

#[derive(Clone, Debug)]
pub struct CalibrateOptions {
    /// Items to sample (each gives one positive and `negatives_per_item` negative pairs).
    pub sample: usize,
    pub negatives_per_item: usize,
    /// Neighbour ranks (1-based, inclusive) that serve as related-but-different items.
    pub neighbour_ranks: (usize, usize),
    /// Items whose text is shorter than this are not sampled.
    pub min_text_chars: usize,
}

impl Default for CalibrateOptions {
    fn default() -> Self {
        Self {
            sample: 300,
            negatives_per_item: 5,
            neighbour_ranks: (20, 50),
            min_text_chars: 200,
        }
    }
}

/// Distribution of similarities in one class of pairs.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Spread {
    pub n: usize,
    pub mean: f32,
    pub p10: f32,
    pub p50: f32,
    pub p90: f32,
}

impl Spread {
    fn of(xs: &[f32]) -> Self {
        if xs.is_empty() {
            return Self::default();
        }
        let mut v = xs.to_vec();
        v.sort_by(f32::total_cmp);
        let q = |f: f64| v[((v.len() - 1) as f64 * f).round() as usize];
        Self {
            n: v.len(),
            mean: v.iter().sum::<f32>() / v.len() as f32,
            p10: q(0.1),
            p50: q(0.5),
            p90: q(0.9),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct HoldoutMetrics {
    pub items: usize,
    /// Chance a held-out "exists" query scores above an "absent" one.
    pub auc: f64,
    /// Class-balanced Brier score of the curve fitted without these items (0.25 = coin flip).
    pub brier: f64,
    /// Share of held-out queries on the correct side of P = 0.5 (classes weighted equally).
    pub balanced_accuracy: f64,
}

/// Title → own text retrieval against every other item in the collection: how often the item's
/// own text outranks all other stored items. Also the model comparison metric.
#[derive(Clone, Debug, Serialize)]
pub struct KnownItem {
    pub recall_at_1: f64,
    pub recall_at_10: f64,
    pub mrr: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CalibrationReport {
    pub collection: String,
    pub space_id: String,
    pub model_key: String,
    pub method: String,
    pub compatibility: String,
    pub items: usize,
    /// Title vs the item's own text.
    pub own_text: Spread,
    /// Title vs the most similar other item (the "absent" score).
    pub nearest_other: Spread,
    /// Title vs 20th–50th neighbours of the item (related, different).
    pub related: Spread,
    /// Diagnostic: chance own-text similarity beats a related item's.
    pub pairwise_auc: f64,
    pub holdout: HoldoutMetrics,
    pub known_item: KnownItem,
    /// Similarity where P(same) reaches 0.5 and 0.7 on the final curve.
    pub similarity_at_p50: Option<f32>,
    pub similarity_at_p70: Option<f32>,
    pub curve: Curve,
    pub vectors: usize,
    pub embed_seconds: f64,
    pub seconds: f64,
    pub cancelled: bool,
    pub notes: Vec<String>,
}

pub fn job_id(collection_id: &str, space_id: &str) -> String {
    format!("calibrate:{collection_id}:{}", &space_id[..12])
}

struct Pairs {
    /// Title vs own text.
    positive: f32,
    /// Title vs related neighbours.
    negatives: Vec<f32>,
    /// Best similarity to any other item.
    nearest_other: f32,
    /// Rank of the own text among all other items (1 = first).
    rank: usize,
}

impl Pairs {
    fn exists(&self) -> f32 {
        self.positive.max(self.nearest_other)
    }
}

pub fn run_calibration(
    project: &Project,
    vectors: &VectorStore,
    table: &VectorTableRecord,
    encoder: &mut dyn TextEmbedder,
    opts: &CalibrateOptions,
    cancel: &CancelToken,
    progress: ProgressFn<'_>,
) -> Result<CalibrationReport> {
    let started = Instant::now();
    let compatibility = check_compatible(project, table, encoder)?;
    vectors.check(table)?;
    let total_vectors = vectors.count(table)?;
    let job_id = job_id(&table.collection_id, &table.space_id);
    let mut job = JobRecord {
        job_id: job_id.clone(),
        kind: "calibrate".into(),
        params: serde_json::json!({"collection": table.collection_id, "space_id": table.space_id,
                                   "sample": opts.sample, "method": METHOD_TITLE_TEXT}),
        state: "running".into(),
        progress: 0.0,
        checkpoint: serde_json::json!({}),
    };
    project.store.save_job(&job)?;
    let result = calibrate_inner(
        project,
        vectors,
        table,
        encoder,
        opts,
        cancel,
        progress,
        &job_id,
        total_vectors,
    );
    job.state = match &result {
        Ok(Some(_)) => {
            job.progress = 1.0;
            "completed".into()
        }
        Ok(None) => "cancelled".into(),
        Err(_) => "failed".into(),
    };
    job.checkpoint = serde_json::json!({"error": result.as_ref().err().map(|e| format!("{e:#}"))});
    project.store.save_job(&job)?;
    let Some((pairs, embed_seconds)) = result? else {
        return Ok(CalibrationReport {
            collection: table.collection_id.clone(),
            space_id: table.space_id.clone(),
            model_key: table.identity.model_key.clone(),
            method: METHOD_TITLE_TEXT.into(),
            compatibility,
            items: 0,
            own_text: Spread::default(),
            nearest_other: Spread::default(),
            related: Spread::default(),
            pairwise_auc: f64::NAN,
            holdout: HoldoutMetrics {
                items: 0,
                auc: f64::NAN,
                brier: f64::NAN,
                balanced_accuracy: f64::NAN,
            },
            known_item: KnownItem {
                recall_at_1: f64::NAN,
                recall_at_10: f64::NAN,
                mrr: f64::NAN,
            },
            similarity_at_p50: None,
            similarity_at_p70: None,
            curve: Curve { points: vec![] },
            vectors: total_vectors,
            embed_seconds: 0.0,
            seconds: started.elapsed().as_secs_f64(),
            cancelled: true,
            notes: vec!["cancelled; nothing was saved".into()],
        });
    };

    // Every fifth item is held out to test a curve fitted on the others.
    let (train, test): (Vec<_>, Vec<_>) = pairs.iter().enumerate().partition(|(i, _)| i % 5 != 4);
    let train: Vec<&Pairs> = train.into_iter().map(|(_, p)| p).collect();
    let test: Vec<&Pairs> = test.into_iter().map(|(_, p)| p).collect();
    let train_curve = fit(&train)?;
    let (tp, tn) = queries(&test);
    let holdout = HoldoutMetrics {
        items: test.len(),
        auc: auc(&tp, &tn),
        brier: balanced_brier(&train_curve, &tp, &tn),
        balanced_accuracy: 0.5
            * (tp
                .iter()
                .filter(|&&x| train_curve.predict(x) >= 0.5)
                .count() as f64
                / tp.len().max(1) as f64
                + tn.iter().filter(|&&x| train_curve.predict(x) < 0.5).count() as f64
                    / tn.len().max(1) as f64),
    };
    let all: Vec<&Pairs> = pairs.iter().collect();
    let curve = fit(&all)?;
    let own: Vec<f32> = pairs.iter().map(|p| p.positive).collect();
    let others: Vec<f32> = pairs.iter().map(|p| p.nearest_other).collect();
    let related: Vec<f32> = pairs.iter().flat_map(|p| p.negatives.clone()).collect();
    let n = pairs.len() as f64;
    let known_item = KnownItem {
        recall_at_1: pairs.iter().filter(|p| p.rank == 1).count() as f64 / n,
        recall_at_10: pairs.iter().filter(|p| p.rank <= 10).count() as f64 / n,
        mrr: pairs.iter().map(|p| 1.0 / p.rank as f64).sum::<f64>() / n,
    };
    let mut notes = vec![
        "fallback calibration without a language model: titles stand in for paraphrases, so \
         P(exists) is approximate (SPEC §6.2)"
            .to_string(),
    ];
    if pairs.len() < 100 {
        notes.push(format!(
            "only {} items could be sampled; the curve is coarse",
            pairs.len()
        ));
    }
    if holdout.auc < 0.8 {
        notes.push(format!(
            "held-out AUC {:.3}: in this collection the best match of a new idea often looks as \
             close as an existing item's own text; treat verdicts with care",
            holdout.auc
        ));
    }
    notes.push(
        "\"absent\" queries assume the nearest other item is a different thing; duplicates in \
         the collection make the curve more cautious"
            .into(),
    );
    let report = CalibrationReport {
        collection: table.collection_id.clone(),
        space_id: table.space_id.clone(),
        model_key: table.identity.model_key.clone(),
        method: METHOD_TITLE_TEXT.into(),
        compatibility,
        items: pairs.len(),
        own_text: Spread::of(&own),
        nearest_other: Spread::of(&others),
        related: Spread::of(&related),
        pairwise_auc: auc(&own, &related),
        holdout,
        known_item,
        similarity_at_p50: curve.similarity_at(0.5),
        similarity_at_p70: curve.similarity_at(0.7),
        curve: curve.clone(),
        vectors: total_vectors,
        embed_seconds,
        seconds: started.elapsed().as_secs_f64(),
        cancelled: false,
        notes,
    };
    project.store.save_calibration(&CalibrationRecord {
        collection_id: table.collection_id.clone(),
        space_id: table.space_id.clone(),
        method: METHOD_TITLE_TEXT.into(),
        sample: pairs.len() as i64,
        vectors: total_vectors as i64,
        curve: serde_json::to_value(&curve)?,
        metrics: serde_json::json!({
            "holdout": report.holdout, "known_item": report.known_item,
            "own_text": report.own_text, "nearest_other": report.nearest_other,
            "related": report.related, "pairwise_auc": report.pairwise_auc,
            "similarity_at_p50": report.similarity_at_p50,
            "similarity_at_p70": report.similarity_at_p70,
        }),
        created_at: None,
    })?;
    Ok(report)
}

/// Top similarity per query when the item exists, and when it is absent.
fn queries(pairs: &[&Pairs]) -> (Vec<f32>, Vec<f32>) {
    (
        pairs.iter().map(|p| p.exists()).collect(),
        pairs.iter().map(|p| p.nearest_other).collect(),
    )
}

/// One exists and one absent query per item, so the classes are already balanced (even odds).
fn fit(pairs: &[&Pairs]) -> Result<Curve> {
    let (exists, absent) = queries(pairs);
    let label = |same: bool| {
        move |&similarity: &f32| Sample {
            similarity,
            same,
            weight: 1.0,
        }
    };
    let samples: Vec<Sample> = exists
        .iter()
        .map(label(true))
        .chain(absent.iter().map(label(false)))
        .collect();
    Curve::fit(&samples)
}

/// Embed in small batches so the job can report progress and stop between batches.
fn embed_batched(
    encoder: &mut dyn TextEmbedder,
    texts: &[String],
    kind: InputKind,
    cancel: &CancelToken,
    tick: &mut dyn FnMut(usize),
) -> Result<Option<Vec<Vec<f32>>>> {
    let mut out = Vec::with_capacity(texts.len());
    for part in texts.chunks(32) {
        if cancel.is_cancelled() {
            return Ok(None);
        }
        out.extend(encoder.embed(part, kind)?.into_iter().map(|e| e.vector));
        tick(part.len());
    }
    Ok(Some(out))
}

#[allow(clippy::too_many_arguments)]
fn calibrate_inner(
    project: &Project,
    vectors: &VectorStore,
    table: &VectorTableRecord,
    encoder: &mut dyn TextEmbedder,
    opts: &CalibrateOptions,
    cancel: &CancelToken,
    progress: ProgressFn<'_>,
    job_id: &str,
    total_vectors: usize,
) -> Result<Option<(Vec<Pairs>, f64)>> {
    let candidates =
        project
            .store
            .titled_items(&table.collection_id, opts.min_text_chars, opts.sample)?;
    let stored = vectors.vectors_for(
        table,
        &candidates.iter().map(|r| r.item_id).collect::<Vec<_>>(),
    )?;
    let items: Vec<_> = candidates
        .into_iter()
        .filter(|r| stored.contains_key(&r.item_id))
        .collect();
    if items.len() < 30 {
        bail!(
            "only {} embedded items have a title and at least {} characters of text; calibration \
             needs 30 or more",
            items.len(),
            opts.min_text_chars
        );
    }
    let identity = encoder.identity().clone();
    let titles: Vec<String> = items
        .iter()
        .map(|r| r.title.clone().unwrap_or_default().trim().to_string())
        .collect();
    let texts: Vec<String> = items
        .iter()
        .map(|r| identity.format_document(None, &r.text))
        .collect();

    let steps = items.len() * 3;
    let mut done = 0usize;
    let mut tick = |n: usize| {
        done += n;
        progress(&Progress {
            job_id: job_id.into(),
            phase: "calibrate".into(),
            done,
            total: steps,
        });
    };
    let t = Instant::now();
    let Some(queries) = embed_batched(encoder, &titles, InputKind::Query, cancel, &mut tick)?
    else {
        return Ok(None);
    };
    let Some(docs) = embed_batched(encoder, &texts, InputKind::Document, cancel, &mut tick)? else {
        return Ok(None);
    };
    let embed_seconds = t.elapsed().as_secs_f64();

    let (lo, hi) = opts.neighbour_ranks;
    let mut pairs = Vec::with_capacity(items.len());
    for (i, r) in items.iter().enumerate() {
        if cancel.is_cancelled() {
            return Ok(None);
        }
        // Related-but-different items: neighbours of the stored item vector at ranks lo..=hi.
        let (scored, _) = vectors.exhaustive(table, &stored[&r.item_id], (hi + 1) * 4, None)?;
        let neighbours: Vec<i64> = collapse(&scored, hi + 1)
            .into_iter()
            .map(|(id, _, _)| id)
            .filter(|&id| id != r.item_id)
            .collect();
        let band: &[i64] = if neighbours.len() >= lo {
            &neighbours[lo - 1..neighbours.len().min(hi)]
        } else {
            // Small collections: the farther half of whatever neighbours exist.
            &neighbours[neighbours.len() / 2..]
        };
        let picks = pick_evenly(band, opts.negatives_per_item);

        // The query against every stored item, collapsed to parents as search does.
        let (all, _) = vectors.exhaustive(table, &queries[i], total_vectors.max(1), None)?;
        let best: HashMap<i64, f32> = collapse(&all, usize::MAX)
            .into_iter()
            .map(|(id, sim, _)| (id, sim))
            .collect();
        let positive = cosine(&queries[i], &docs[i]);
        let negatives: Vec<f32> = picks
            .iter()
            .filter_map(|id| best.get(id).copied())
            .collect();
        let others = best.iter().filter(|(id, _)| **id != r.item_id);
        let nearest_other = others.clone().map(|(_, s)| *s).fold(-1.0, f32::max);
        let rank = 1 + others.filter(|(_, sim)| **sim > positive).count();
        pairs.push(Pairs {
            positive,
            negatives,
            nearest_other,
            rank,
        });
        tick(1);
    }
    Ok(Some((pairs, embed_seconds)))
}

/// `n` items spread evenly across `xs` (all of them when there are fewer).
fn pick_evenly(xs: &[i64], n: usize) -> Vec<i64> {
    if xs.len() <= n || n == 0 {
        return xs.to_vec();
    }
    (0..n)
        .map(|k| xs[k * (xs.len() - 1) / (n - 1).max(1)])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_spread_over_the_band() {
        let xs: Vec<i64> = (20..=50).collect();
        assert_eq!(pick_evenly(&xs, 5), vec![20, 27, 35, 42, 50]);
        assert_eq!(pick_evenly(&[1, 2], 5), vec![1, 2]);
        assert_eq!(pick_evenly(&xs, 1), vec![20]);
    }

    #[test]
    fn spread_quantiles() {
        let s = Spread::of(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 0.0]);
        assert_eq!((s.n, s.p50, s.p10, s.p90), (11, 0.5, 0.1, 0.9));
    }
}
