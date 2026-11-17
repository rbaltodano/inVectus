//! Verify a gap against outside sources (SPEC §6.6, §4 step 7). The query is keyword search over
//! the gap's region terms (reviews only for coverage and stale gaps; both sides for bridges).
//! Hits are re-ranked by local similarity to the gap's accepted description, or to its label
//! when it has none, and a plain summary says how many look like matches. Titles and links are
//! shown so a person can judge: similarity alone is not the verdict.

use crate::connectors::{ExternalHit, KeywordQuery, Source, SourceResult};
use anyhow::{Context, Result, bail};
use invectus_core::store::VectorTableRecord;
use invectus_core::{InputKind, Project, TextEmbedder};
use invectus_gaps::GapKind;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct VerifyOptions {
    pub sources: Vec<Source>,
    pub max_per_source: usize,
    /// Allow this check even when the project's external-checks switch is off.
    pub allow_network: bool,
}

impl VerifyOptions {
    /// The project's chosen outside sources (unknown names are skipped), else the default pair.
    pub fn for_project(project: &Project) -> Self {
        let sources: Vec<Source> = project
            .config
            .verify_sources
            .iter()
            .flatten()
            .filter_map(|s| s.parse().ok())
            .collect();
        Self {
            sources: if sources.is_empty() {
                Self::default().sources
            } else {
                sources
            },
            ..Self::default()
        }
    }
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            sources: vec![Source::PubMed, Source::OpenAlex],
            max_per_source: 20,
            allow_network: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RankedHit {
    #[serde(flatten)]
    pub hit: ExternalHit,
    /// Similarity of the hit's title to the description (local model; titles only).
    pub similarity: f32,
    pub looks_like_match: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Verification {
    pub gap_id: i32,
    pub kind: String,
    pub label: String,
    pub query: KeywordQuery,
    /// What the hits were compared with: the accepted description or the gap's label.
    pub compared_with: String,
    pub sources: Vec<SourceSummary>,
    pub hits: Vec<RankedHit>,
    pub matches: usize,
    /// Similarity at which a hit "looks like a match" (the calibration's P = 0.5 point).
    pub match_at: f32,
    pub summary: String,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceSummary {
    pub source: String,
    pub query: String,
    pub total: Option<u64>,
    pub fetched: usize,
    pub cached: bool,
    pub error: Option<String>,
}

/// The accepted description of a gap's target, if one was generated.
fn accepted_description(
    project: &Project,
    table: &VectorTableRecord,
    target: &serde_json::Value,
) -> Result<Option<String>> {
    for (t, result, _) in project
        .store
        .midpoints(&table.collection_id, &table.space_id)?
    {
        if &t != target {
            continue;
        }
        if let Some(i) = result["accepted"].as_u64() {
            let a = &result["attempts"][i as usize];
            return Ok(Some(format!(
                "{}. {}",
                a["title"].as_str().unwrap_or(""),
                a["description"].as_str().unwrap_or("")
            )));
        }
    }
    Ok(None)
}

/// A check prepared from the project (under its lock): what to ask and what to compare with.
/// Holds no reference to the project, so the network work can run without the lock.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub root: std::path::PathBuf,
    files: invectus_core::project_files::ProjectFiles,
    collection_id: String,
    space_id: String,
    run_id: String,
    gap_id: i32,
    kind: String,
    label: String,
    query: KeywordQuery,
    compared_with: String,
    match_at: f32,
    notes: Vec<String>,
    opts: VerifyOptions,
}

/// What the outside sources answered.
#[derive(Clone, Debug)]
pub struct Fetched {
    sources: Vec<SourceSummary>,
    hits: Vec<ExternalHit>,
}

/// Check a gap end to end while holding `project` (the CLI). The app and agents use
/// [`prepare`], [`fetch`] and [`finish`] so no lock is held during network waits.
pub fn verify_gap(
    project: &Project,
    table: &VectorTableRecord,
    encoder: &mut dyn TextEmbedder,
    gap_id: i32,
    opts: &VerifyOptions,
) -> Result<Verification> {
    let p = prepare(project, table, gap_id, opts)?;
    let f = fetch(&p, None);
    finish(project, table, encoder, &p, f)
}

/// Read what the check needs from the project. Refused unless External checks is on (or the
/// CLI's `--network` override is given).
pub fn prepare(
    project: &Project,
    table: &VectorTableRecord,
    gap_id: i32,
    opts: &VerifyOptions,
) -> Result<Prepared> {
    if !opts.allow_network && !project.config.network_enabled {
        bail!(
            "external checks are off for this project: checking a gap sends its description to {}. \
             Turn on External checks (Settings → Network in the app) or pass --network to the \
             CLI (results are cached in the project)",
            opts.sources
                .iter()
                .map(|s| s.name())
                .collect::<Vec<_>>()
                .join(" and ")
        );
    }
    let run = project
        .store
        .latest_region_run(&table.collection_id, &table.space_id)?
        .context("no regions yet; run `invectus regions run` first")?;
    let gap = invectus_gaps::detail(project, table, gap_id)?.view.gap;
    let regions = project.store.regions(&run.run_id)?;
    let fine = |id: i32| {
        regions
            .iter()
            .find(|r| r.level == "fine" && r.region_id == id)
            .with_context(|| format!("no fine region {id}"))
    };
    let ra = fine(gap.region_a)?;
    let (query, target) = match (gap.kind, gap.region_b) {
        (GapKind::Bridge, Some(b)) => {
            let rb = fine(b)?;
            (
                KeywordQuery {
                    groups: vec![
                        ra.terms.iter().take(3).cloned().collect(),
                        rb.terms.iter().take(3).cloned().collect(),
                    ],
                    reviews_only: false,
                },
                serde_json::json!({"kind": "between", "a": gap.region_a, "b": b, "t": 0.5}),
            )
        }
        _ => (
            // Coverage and stale gaps: the region's distinctive terms, two required.
            KeywordQuery {
                groups: vec![
                    ra.terms.iter().take(2).cloned().collect(),
                    ra.terms.iter().skip(2).take(3).cloned().collect(),
                ],
                reviews_only: true,
            },
            serde_json::json!({"kind": "region", "region": gap.region_a}),
        ),
    };
    let label = ra
        .llm_label
        .clone()
        .map(|l| match gap.region_b.and_then(|b| fine(b).ok()) {
            Some(rb) => format!("{l} ↔ {}", rb.llm_label.clone().unwrap_or(rb.label.clone())),
            None => l,
        })
        .unwrap_or_else(|| gap.label.clone());
    let mut notes = Vec::new();
    let compared_with = match accepted_description(project, table, &target)? {
        Some(d) => d,
        None => {
            notes.push(
                "no accepted description for this gap: hits are compared with its label (run \
                 `invectus gaps describe` for a sharper comparison)"
                    .into(),
            );
            label.clone()
        }
    };

    let match_at = project
        .store
        .latest_calibration(&table.collection_id, &table.space_id)?
        .and_then(|c| c.metrics["similarity_at_p50"].as_f64())
        .map(|x| x as f32)
        .unwrap_or_else(|| {
            notes.push("no calibration: the match threshold defaults to 0.8".into());
            0.8
        });
    Ok(Prepared {
        files: invectus_core::project_files::ProjectFiles::for_project(project)?,
        root: project.root.clone(),
        collection_id: table.collection_id.clone(),
        space_id: table.space_id.clone(),
        run_id: run.run_id.clone(),
        gap_id,
        kind: gap.kind.as_str().into(),
        label,
        query,
        compared_with,
        match_at,
        notes,
        opts: opts.clone(),
    })
}

/// Ask the outside sources (network; no project lock needed). Bounded by
/// [`crate::connectors::BUDGET`] and stopped early by `cancel`. Per-source failures are kept in
/// the result rather than failing the whole check.
pub fn fetch(p: &Prepared, cancel: Option<&invectus_core::CancelToken>) -> Fetched {
    let deadline = std::time::Instant::now() + crate::connectors::BUDGET;
    crate::connectors::with_limits(cancel, deadline, || {
        let query = &p.query;
        let opts = &p.opts;
        let mut sources = Vec::new();
        let mut hits: Vec<ExternalHit> = Vec::new();
        for s in &opts.sources {
            match crate::connectors::search_with_files(
                &p.root,
                &p.files,
                *s,
                query,
                opts.max_per_source,
            ) {
                Ok(SourceResult {
                    source,
                    query,
                    total,
                    hits: h,
                    cached,
                    ..
                }) => {
                    sources.push(SourceSummary {
                        source,
                        query,
                        total,
                        fetched: h.len(),
                        cached,
                        error: None,
                    });
                    hits.extend(h);
                }
                Err(e) => sources.push(SourceSummary {
                    source: s.name().into(),
                    query: s.query(query),
                    total: None,
                    fetched: 0,
                    cached: false,
                    error: Some(format!("{e:#}")),
                }),
            }
        }

        Fetched { sources, hits }
    })
}

/// Rank the answers with the local model and save the verification (under the project lock).
pub fn finish(
    project: &Project,
    table: &VectorTableRecord,
    encoder: &mut dyn TextEmbedder,
    p: &Prepared,
    f: Fetched,
) -> Result<Verification> {
    if project.root != p.root
        || table.collection_id != p.collection_id
        || table.space_id != p.space_id
    {
        bail!("the project or collection changed while checking; check the gap again");
    }
    let Fetched { sources, hits } = f;
    let mut notes = p.notes.clone();
    let match_at = p.match_at;
    let compared_with = p.compared_with.clone();
    let mut ranked = Vec::new();
    if !hits.is_empty() {
        let q = encoder
            .embed(std::slice::from_ref(&compared_with), InputKind::Query)?
            .remove(0)
            .vector;
        let titles: Vec<String> = hits.iter().map(|h| h.title.clone()).collect();
        let docs = encoder.embed(&titles, InputKind::Document)?;
        for (h, d) in hits.into_iter().zip(docs) {
            let similarity: f32 = q.iter().zip(&d.vector).map(|(a, b)| a * b).sum();
            ranked.push(RankedHit {
                hit: h,
                similarity,
                looks_like_match: similarity >= match_at,
            });
        }
        ranked.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
    }
    let matches = ranked.iter().filter(|h| h.looks_like_match).count();
    let total: u64 = sources.iter().filter_map(|s| s.total).sum();
    let summary = match ranked.first() {
        None if sources.iter().all(|s| s.error.is_some()) => {
            "No outside source could be searched.".to_string()
        }
        None => "The outside sources found nothing for these keywords.".to_string(),
        Some(top) if matches > 0 => format!(
            "{matches} outside item(s) look like matches; the closest is \"{}\" ({}{}, \
             similarity {:.3}).",
            top.hit.title,
            top.hit.source,
            top.hit.year.map(|y| format!(" {y}")).unwrap_or_default(),
            top.similarity
        ),
        Some(top) => format!(
            "None of the {} item(s) fetched (of {total} matching the keywords) looks like a \
             match; the closest is \"{}\" (similarity {:.3}).",
            ranked.len(),
            top.hit.title,
            top.similarity
        ),
    };
    notes.push(
        "similarity uses titles only and the collection's calibration point; read the titles \
         before deciding"
            .into(),
    );
    let v = Verification {
        gap_id: p.gap_id,
        kind: p.kind.clone(),
        label: p.label.clone(),
        query: p.query.clone(),
        compared_with,
        sources,
        hits: ranked,
        matches,
        match_at,
        summary,
        notes,
    };
    project.store.save_verification(
        &table.collection_id,
        &table.space_id,
        &p.run_id,
        p.gap_id,
        &serde_json::to_value(&v)?,
    )?;
    Ok(v)
}
