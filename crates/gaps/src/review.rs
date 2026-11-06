//! Reading gaps with their human verdicts. A verdict attaches to the gap it was given on and,
//! after re-runs, to any gap of the same type whose core items overlap it by Jaccard ≥ 0.5, so
//! decisions survive re-clustering (SPEC §3).

use crate::scan::Gap;
use anyhow::{Context, Result, bail};
use invectus_core::Project;
use invectus_core::store::{GapVerdictRecord, VectorTableRecord};
use serde::Serialize;
use std::collections::HashSet;

/// Core-item overlap needed to carry a verdict over to a re-run's gap.
pub const CARRY_OVER_AT: f64 = 0.5;

#[derive(Clone, Debug, Serialize)]
pub struct GapView {
    #[serde(flatten)]
    pub gap: Gap,
    /// The person's decision (the newest one carried over to this gap).
    pub verdict: Option<GapVerdictRecord>,
    /// An agent's suggestion the person has not acted on yet: shown beside the decision,
    /// never in its place (target product §7).
    pub suggestion: Option<GapVerdictRecord>,
    /// The latest outside-source check of this gap (`invectus gaps verify`), summarised.
    pub verification: Option<VerificationSummary>,
}

/// What the last verification found, without its full hit list.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct VerificationSummary {
    pub matches: u64,
    pub summary: String,
    /// The closest outside items.
    pub closest: Vec<OutsideHit>,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct OutsideHit {
    pub title: String,
    pub source: String,
    pub year: Option<i64>,
    pub url: Option<String>,
    pub similarity: f64,
}

fn summarise(v: &serde_json::Value) -> VerificationSummary {
    VerificationSummary {
        matches: v["matches"].as_u64().unwrap_or(0),
        summary: v["summary"].as_str().unwrap_or("").to_string(),
        closest: v["hits"]
            .as_array()
            .map(|a| {
                a.iter()
                    .take(3)
                    .map(|h| OutsideHit {
                        title: h["title"].as_str().unwrap_or("").to_string(),
                        source: h["source"].as_str().unwrap_or("").to_string(),
                        year: h["year"].as_i64(),
                        url: h["url"].as_str().map(String::from),
                        similarity: h["similarity"].as_f64().unwrap_or(f64::NAN),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ItemSummary {
    pub item_id: i64,
    pub title: Option<String>,
    pub date: Option<String>,
    pub role: String,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GapDetail {
    #[serde(flatten)]
    pub view: GapView,
    /// Items nearest the region centre(s): the evidence a person reads.
    pub core_items: Vec<ItemSummary>,
    /// Stored writing for this target in the current region run, including its checks/author.
    pub description: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GapList {
    pub run_id: String,
    pub stats: serde_json::Value,
    pub gaps: Vec<GapView>,
}

pub(crate) fn jaccard(a: &[i64], b: &[i64]) -> f64 {
    let a: HashSet<_> = a.iter().collect();
    let b: HashSet<_> = b.iter().collect();
    let union = a.union(&b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(&b).count() as f64 / union as f64
}

/// Records (newest first) of the same type whose core overlaps the gap's enough.
fn matching<'a>(
    gap: &'a Gap,
    verdicts: &'a [GapVerdictRecord],
) -> impl Iterator<Item = &'a GapVerdictRecord> {
    verdicts
        .iter()
        .filter(|v| v.kind == gap.kind.as_str() && jaccard(&v.core, &gap.core) >= CARRY_OVER_AT)
}

/// The newest decision, and a suggestion that is newer than any decision or dismissal.
fn decision_and_suggestion(
    gap: &Gap,
    verdicts: &[GapVerdictRecord],
) -> (Option<GapVerdictRecord>, Option<GapVerdictRecord>) {
    let decision = matching(gap, verdicts)
        .find(|v| v.entry == "decision")
        .cloned();
    let suggestion = matching(gap, verdicts)
        .next()
        .filter(|v| v.entry == "suggestion")
        .cloned();
    (decision, suggestion)
}

/// Gaps of the latest scan with their verdicts.
pub fn list(project: &Project, table: &VectorTableRecord) -> Result<GapList> {
    let run = project
        .store
        .latest_region_run(&table.collection_id, &table.space_id)?
        .with_context(|| {
            format!(
                "collection {:?} has no regions; run `invectus regions run` and `invectus gaps \
                 scan` first",
                table.collection_id
            )
        })?;
    let (stats, records) = project.store.gaps(&run.run_id)?.with_context(|| {
        format!(
            "the latest regions of {:?} have not been scanned; run `invectus gaps scan -c {}`",
            table.collection_id, table.collection_id
        )
    })?;
    let verdicts = project
        .store
        .gap_verdicts(&table.collection_id, &table.space_id)?;
    let gaps = records
        .into_iter()
        .map(|r| {
            let gap: Gap = serde_json::from_value(r.data)?;
            let (verdict, suggestion) = decision_and_suggestion(&gap, &verdicts);
            Ok(GapView {
                verdict,
                suggestion,
                verification: project
                    .store
                    .latest_verification(&run.run_id, gap.gap_id)?
                    .map(|v| summarise(&v)),
                gap,
            })
        })
        .collect::<Result<_>>()?;
    Ok(GapList {
        run_id: run.run_id,
        stats,
        gaps,
    })
}

pub fn detail(project: &Project, table: &VectorTableRecord, gap_id: i32) -> Result<GapDetail> {
    detail_at(project, table, gap_id, 0.5)
}

pub fn detail_at(
    project: &Project,
    table: &VectorTableRecord,
    gap_id: i32,
    t: f32,
) -> Result<GapDetail> {
    anyhow::ensure!(
        t.is_finite() && (0.0..=1.0).contains(&t),
        "position must be between 0 and 1"
    );
    let view = list(project, table)?
        .gaps
        .into_iter()
        .find(|g| g.gap.gap_id == gap_id)
        .with_context(|| format!("no gap {gap_id} in the latest scan"))?;
    let items = project.store.items_by_ids(&view.gap.core)?;
    let core_items = view
        .gap
        .core
        .iter()
        .filter_map(|id| items.get(id))
        .map(|i| ItemSummary {
            item_id: i.item_id,
            title: i.title.clone(),
            date: i.date.clone(),
            role: i.role.as_str().into(),
            url: i.url.clone(),
        })
        .collect();
    let run = project
        .store
        .latest_region_run(&table.collection_id, &table.space_id)?
        .context("no current region run")?;
    let target = match view.gap.region_b {
        Some(b) => serde_json::json!({"kind":"between", "a":view.gap.region_a, "b":b, "t":t}),
        None => serde_json::json!({"kind":"region", "region":view.gap.region_a}),
    };
    let description = project.store.latest_midpoint(
        &table.collection_id,
        &table.space_id,
        &run.run_id,
        &target,
    )?;
    Ok(GapDetail {
        view,
        core_items,
        description,
    })
}

/// Record the person's decision on a gap of the latest scan. Any pending suggestion is
/// settled by it.
pub fn set_verdict(
    project: &Project,
    table: &VectorTableRecord,
    gap_id: i32,
    verdict: &str,
    note: &str,
) -> Result<GapView> {
    record(project, table, gap_id, verdict, note, "person", "decision")
}

/// Record an agent's suggested decision and its reasoning. It waits for the person: it never
/// changes the gap's decision.
pub fn suggest(
    project: &Project,
    table: &VectorTableRecord,
    gap_id: i32,
    verdict: &str,
    reasoning: &str,
    author: &str,
) -> Result<GapView> {
    if reasoning.trim().is_empty() {
        bail!("a suggestion needs its reasoning");
    }
    record(
        project,
        table,
        gap_id,
        verdict,
        reasoning,
        author,
        "suggestion",
    )
}

/// Respond only to the suggestion the person actually reviewed. History retains its author
/// and reasoning even after it is accepted or dismissed.
pub fn respond_to_suggestion(
    project: &Project,
    table: &VectorTableRecord,
    gap_id: i32,
    expected_at: &str,
    decision: Option<&str>,
    note: &str,
) -> Result<GapView> {
    let view = find(project, table, gap_id)?;
    let current = view
        .suggestion
        .context("that suggestion is no longer pending; review the latest gap")?;
    anyhow::ensure!(
        current.updated_at.as_deref() == Some(expected_at),
        "the agent suggestion changed; review it before responding"
    );
    match decision {
        Some(verdict) => set_verdict(project, table, gap_id, verdict, note),
        None => dismiss_suggestion(project, table, gap_id),
    }
}

/// The person sets a gap's pending suggestion aside without deciding.
pub fn dismiss_suggestion(
    project: &Project,
    table: &VectorTableRecord,
    gap_id: i32,
) -> Result<GapView> {
    let view = find(project, table, gap_id)?;
    let s = view
        .suggestion
        .with_context(|| format!("gap {gap_id} has no pending suggestion"))?;
    record(
        project,
        table,
        gap_id,
        &s.verdict,
        "",
        "person",
        "dismissal",
    )
}

fn find(project: &Project, table: &VectorTableRecord, gap_id: i32) -> Result<GapView> {
    list(project, table)?
        .gaps
        .into_iter()
        .find(|g| g.gap.gap_id == gap_id)
        .with_context(|| format!("no gap {gap_id} in the latest scan"))
}

fn record(
    project: &Project,
    table: &VectorTableRecord,
    gap_id: i32,
    verdict: &str,
    note: &str,
    author: &str,
    entry: &str,
) -> Result<GapView> {
    let view = find(project, table, gap_id)?;
    if view.gap.core.is_empty() {
        bail!("gap {gap_id} has no core items to remember the verdict by");
    }
    project.store.save_gap_verdict(&GapVerdictRecord {
        collection_id: table.collection_id.clone(),
        space_id: table.space_id.clone(),
        kind: view.gap.kind.as_str().into(),
        core: view.gap.core.clone(),
        verdict: verdict.into(),
        note: note.into(),
        author: author.into(),
        entry: entry.into(),
        updated_at: None,
    })?;
    find(project, table, gap_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_summaries_keep_the_three_closest() {
        let v = serde_json::json!({
            "matches": 1, "summary": "1 outside item looks like a match",
            "hits": [
                {"title": "A", "source": "pubmed", "year": 2020, "url": "u", "similarity": 0.9},
                {"title": "B", "source": "openalex", "similarity": 0.8},
                {"title": "C", "source": "pubmed", "similarity": 0.7},
                {"title": "D", "source": "pubmed", "similarity": 0.6}
            ]
        });
        let s = summarise(&v);
        assert_eq!((s.matches, s.closest.len()), (1, 3));
        assert_eq!(s.closest[0].year, Some(2020));
        assert_eq!(s.closest[1].url, None);
        assert_eq!(summarise(&serde_json::json!({})).closest.len(), 0);
    }

    #[test]
    fn jaccard_overlap() {
        assert_eq!(jaccard(&[1, 2, 3, 4], &[3, 4, 5, 6]), 2.0 / 6.0);
        assert_eq!(jaccard(&[1, 2], &[1, 2]), 1.0);
        assert_eq!(jaccard(&[], &[]), 0.0);
    }
}
