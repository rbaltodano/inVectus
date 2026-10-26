//! `invectus embed`: a resumable, idempotent job that embeds a collection into one space.
//!
//! Inputs are hashed (space id + exact model input); stored vectors with the same hash are kept,
//! changed or deleted items lose their old vectors, and new vectors are appended in small
//! batches. A cancelled or crashed run keeps every flushed batch, so re-running resumes.

use crate::vectors::{VectorRow, VectorStore};
use anyhow::{Result, bail};
use invectus_core::job::ProgressFn;
use invectus_core::store::{JobRecord, VectorTableRecord};
use invectus_core::{CancelToken, InputKind, Progress, Project, TextEmbedder};
use serde::Serialize;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct EmbedJobOptions {
    /// Vectors per Lance append (and checkpoint).
    pub flush_every: usize,
    /// Build an ANN index when the table has at least this many vectors.
    pub ann_min_rows: usize,
}

impl Default for EmbedJobOptions {
    fn default() -> Self {
        Self {
            flush_every: 256,
            ann_min_rows: 5_000,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct EmbedReport {
    pub job_id: String,
    pub table: String,
    pub space_id: String,
    pub model_key: String,
    pub inputs: usize,
    pub already_embedded: usize,
    pub embedded: usize,
    pub removed_stale: usize,
    pub truncated: usize,
    pub mean_tokens: f64,
    pub seconds: f64,
    pub items_per_second: f64,
    pub cancelled: bool,
    pub vectors_in_table: usize,
    pub ann_partitions: Option<u32>,
}

pub fn job_id(collection_id: &str, space_id: &str) -> String {
    format!("embed:{collection_id}:{}", &space_id[..12])
}

pub fn run_embed(
    project: &Project,
    vectors: &VectorStore,
    collection_id: &str,
    embedder: &mut dyn TextEmbedder,
    opts: &EmbedJobOptions,
    cancel: &CancelToken,
    progress: ProgressFn<'_>,
) -> Result<EmbedReport> {
    project.store.require_collection(collection_id)?;
    let identity = embedder.identity().clone();
    let table = vectors.ensure_table(project, collection_id, &identity, "embedded")?;
    let job_id = job_id(collection_id, &table.space_id);
    let started = Instant::now();
    let mut report = EmbedReport {
        job_id: job_id.clone(),
        table: table.table_name.clone(),
        space_id: table.space_id.clone(),
        model_key: identity.model_key.clone(),
        ..Default::default()
    };
    let mut job = JobRecord {
        job_id: job_id.clone(),
        kind: "embed".into(),
        params: serde_json::json!({"collection": collection_id, "space_id": table.space_id,
                                   "model": identity.model_key}),
        state: "running".into(),
        progress: 0.0,
        checkpoint: serde_json::json!({}),
    };
    project.store.save_job(&job)?;

    let result = embed_inner(
        project,
        vectors,
        &table,
        embedder,
        opts,
        cancel,
        progress,
        &mut report,
        &mut job,
    );
    report.seconds = started.elapsed().as_secs_f64();
    if report.embedded > 0 {
        report.items_per_second = report.embedded as f64 / report.seconds;
    }
    job.state = match &result {
        Ok(()) if report.cancelled => "cancelled".into(),
        Ok(()) => "completed".into(),
        Err(_) => "failed".into(),
    };
    job.checkpoint = serde_json::json!({
        "embedded": report.embedded, "already_embedded": report.already_embedded,
        "inputs": report.inputs, "error": result.as_ref().err().map(|e| format!("{e:#}")),
    });
    project.store.save_job(&job)?;
    result.map(|()| report)
}

#[allow(clippy::too_many_arguments)]
fn embed_inner(
    project: &Project,
    vectors: &VectorStore,
    table: &VectorTableRecord,
    embedder: &mut dyn TextEmbedder,
    opts: &EmbedJobOptions,
    cancel: &CancelToken,
    progress: ProgressFn<'_>,
    report: &mut EmbedReport,
    job: &mut JobRecord,
) -> Result<()> {
    if table.origin != "embedded" {
        bail!(
            "{} holds imported vectors and is query-only",
            table.table_name
        );
    }
    let identity = embedder.identity().clone();
    let rows = project.store.embed_rows(&table.collection_id)?;
    report.inputs = rows.len();
    let stored = vectors.stored_hashes(table)?;

    let mut todo = Vec::new();
    let mut wanted: HashMap<i64, String> = HashMap::with_capacity(rows.len());
    for r in rows {
        let text = identity.format_document(r.title.as_deref(), &r.text);
        let hash = identity.input_hash(&text);
        if stored.get(&r.item_id) == Some(&hash) {
            report.already_embedded += 1;
        } else {
            todo.push((r.item_id, r.parent_id, hash.clone(), text));
        }
        wanted.insert(r.item_id, hash);
    }
    // Changed items (new hash) and items no longer in the collection lose their old vectors.
    let stale: Vec<i64> = stored
        .iter()
        .filter(|(id, h)| wanted.get(*id) != Some(*h))
        .map(|(id, _)| *id)
        .collect();
    if !stale.is_empty() {
        vectors.delete_ids(table, &stale)?;
        report.removed_stale = stale.len();
    }

    // Similar lengths together keep padding low; ids break ties so runs are reproducible.
    todo.sort_by(|a, b| a.3.len().cmp(&b.3.len()).then(a.0.cmp(&b.0)));
    let total = todo.len();
    let mut token_sum = 0usize;
    let mut tick = |done: usize| {
        progress(&Progress {
            job_id: job.job_id.clone(),
            phase: "embed".into(),
            done,
            total,
        })
    };
    tick(0);
    for chunk in todo.chunks(opts.flush_every.max(1)) {
        if cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }
        let texts: Vec<String> = chunk.iter().map(|t| t.3.clone()).collect();
        let out = embedder.embed(&texts, InputKind::Document)?;
        let rows: Vec<VectorRow> = chunk
            .iter()
            .zip(out)
            .map(|((id, parent, hash, _), e)| {
                token_sum += e.tokens;
                report.truncated += e.truncated as usize;
                VectorRow {
                    item_id: *id,
                    parent_id: *parent,
                    input_hash: hash.clone(),
                    vector: e.vector,
                }
            })
            .collect();
        vectors.add(table, &rows)?;
        report.embedded += rows.len();
        job.progress = report.embedded as f64 / total.max(1) as f64;
        job.checkpoint =
            serde_json::json!({"embedded": report.embedded, "remaining": total - report.embedded});
        project.store.save_job(job)?;
        tick(report.embedded);
    }
    if report.embedded > 0 {
        report.mean_tokens = token_sum as f64 / report.embedded as f64;
    }
    report.vectors_in_table = vectors.count(table)?;
    let changed = report.embedded + report.removed_stale > 0;
    let rows_in_table = report.vectors_in_table;
    if !report.cancelled && rows_in_table >= opts.ann_min_rows && (changed || table.ann.is_none()) {
        let partitions = vectors.build_ann(table, rows_in_table)?;
        project.store.set_ann(
            &table.collection_id,
            &table.space_id,
            &serde_json::json!({"type": "IVF_FLAT", "metric": "cosine",
                                "partitions": partitions, "rows": rows_in_table}),
        )?;
        report.ann_partitions = Some(partitions);
    }
    Ok(())
}
