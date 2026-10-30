//! Project-local bookmarks and the latest human decisions, scoped to one embedding space.
use crate::review::{CARRY_OVER_AT, GapView, jaccard};
use anyhow::{Context, Result};
use invectus_core::{
    Project,
    store::{GapBookmarkRecord, GapVerdictRecord, VectorTableRecord},
};
use serde::Serialize;

#[derive(Serialize)]
pub struct SavedBookmark {
    /// Opaque JSON identity; item ids exceed JavaScript's exact integer range.
    pub core_key: String,
    #[serde(flatten)]
    pub record: GapBookmarkRecord,
    pub gap_id: Option<i32>,
}

#[derive(Serialize)]
pub struct SavedDecision {
    pub core_key: String,
    pub label: String,
    pub gap_id: Option<i32>,
    pub verdict: GapVerdictRecord,
}

#[derive(Serialize)]
pub struct SavedOverview {
    pub bookmarks: Vec<SavedBookmark>,
    pub decisions: Vec<SavedDecision>,
}

fn current<'a>(gaps: &'a [GapView], kind: &str, core: &[i64]) -> Option<&'a GapView> {
    gaps.iter()
        .filter(|g| g.gap.kind.as_str() == kind)
        .filter_map(|g| {
            let overlap = jaccard(&g.gap.core, core);
            (overlap >= CARRY_OVER_AT).then_some((g, overlap))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(g, _)| g)
}

pub fn overview(project: &Project, table: &VectorTableRecord) -> Result<SavedOverview> {
    // Saved work remains accessible even between mapping and scanning.
    let run = project
        .store
        .latest_region_run(&table.collection_id, &table.space_id)?;
    let scanned = if let Some(run) = run {
        project.store.gaps(&run.run_id)?.is_some()
    } else {
        false
    };
    let gaps = if scanned {
        crate::list(project, table)?.gaps
    } else {
        vec![]
    };
    let bookmarks = project
        .store
        .gap_bookmarks(&table.collection_id, &table.space_id)?
        .into_iter()
        .map(|record| {
            Ok(SavedBookmark {
                core_key: serde_json::to_string(&record.core)?,
                gap_id: current(&gaps, &record.kind, &record.core).map(|g| g.gap.gap_id),
                record,
            })
        })
        .collect::<Result<_>>()?;
    let records = project
        .store
        .gap_verdicts(&table.collection_id, &table.space_id)?;
    let mut decisions: Vec<SavedDecision> = vec![];
    for verdict in records {
        // Records arrive newest first; retain the latest judgment for each matching gap.
        if decisions.iter().any(|d| {
            d.verdict.kind == verdict.kind
                && jaccard(&d.verdict.core, &verdict.core) >= CARRY_OVER_AT
        }) {
            continue;
        }
        let matched = current(&gaps, &verdict.kind, &verdict.core);
        let label = if let Some(g) = matched {
            g.gap.label.clone()
        } else {
            let items = project.store.items_by_ids(&verdict.core)?;
            verdict
                .core
                .iter()
                .find_map(|id| items.get(id).and_then(|i| i.title.clone()))
                .unwrap_or_else(|| {
                    format!("{} gap · {} core items", verdict.kind, verdict.core.len())
                })
        };
        decisions.push(SavedDecision {
            core_key: serde_json::to_string(&verdict.core)?,
            label,
            gap_id: matched.map(|g| g.gap.gap_id),
            verdict,
        });
    }
    Ok(SavedOverview {
        bookmarks,
        decisions,
    })
}

/// Bookmark the gap's core identity, never its scan-local numeric id.
pub fn bookmark(project: &Project, table: &VectorTableRecord, gap_id: i32) -> Result<()> {
    let view = crate::list(project, table)?
        .gaps
        .into_iter()
        .find(|g| g.gap.gap_id == gap_id)
        .context("gap is not in the latest scan")?;
    let existing = project
        .store
        .gap_bookmarks(&table.collection_id, &table.space_id)?;
    if existing.iter().any(|b| {
        b.kind == view.gap.kind.as_str() && jaccard(&b.core, &view.gap.core) >= CARRY_OVER_AT
    }) {
        return Ok(());
    }
    project.store.set_gap_bookmark(
        &table.collection_id,
        &table.space_id,
        view.gap.kind.as_str(),
        &view.gap.core,
        Some(&serde_json::to_value(&view.gap)?),
    )
}

pub fn remove_bookmark(
    project: &Project,
    table: &VectorTableRecord,
    kind: &str,
    core: &[i64],
) -> Result<()> {
    project
        .store
        .set_gap_bookmark(&table.collection_id, &table.space_id, kind, core, None)
}
