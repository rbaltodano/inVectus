//! Parity check: does a local encoder reproduce vectors stored by another runtime (e.g. gapmap's
//! sentence-transformers fp16 vectors)? Re-embeds a deterministic sample of stored items with
//! the encoder and compares cosine similarity to the stored vectors (SPEC recommendation 4).

use crate::vectors::{VectorStore, cosine};
use anyhow::{Result, bail};
use invectus_core::store::{ParityRecord, VectorTableRecord};
use invectus_core::{InputKind, Project, TextEmbedder};
use serde::Serialize;

/// Thresholds for admitting an encoder to another runtime's space.
pub const MIN_MEAN_COS: f64 = 0.995;
pub const MIN_COS: f64 = 0.98;

#[derive(Clone, Debug, Serialize)]
pub struct ParityReport {
    pub record: ParityRecord,
    pub table: String,
    pub encoder_runtime: String,
    pub stored_runtime: String,
    pub thresholds: (f64, f64),
}

pub fn run_parity(
    project: &Project,
    vectors: &VectorStore,
    table: &VectorTableRecord,
    encoder: &mut dyn TextEmbedder,
    sample: usize,
) -> Result<ParityReport> {
    let enc = encoder.identity().clone();
    if !table.identity.parity_compatible(&enc) {
        bail!(
            "encoder {} does not share the text policy of {}",
            enc.model_key,
            table.table_name
        );
    }
    // Item ids are hash-derived, so the first ids form a deterministic pseudo-random sample.
    let rows: Vec<_> = project
        .store
        .embed_rows(&table.collection_id)?
        .into_iter()
        .filter(|r| r.parent_id.is_none())
        .take(sample.max(1))
        .collect();
    let ids: Vec<i64> = rows.iter().map(|r| r.item_id).collect();
    let stored = vectors.vectors_for(table, &ids)?;
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|r| stored.contains_key(&r.item_id))
        .collect();
    if rows.is_empty() {
        bail!(
            "{} has no stored vectors for this collection's items",
            table.table_name
        );
    }
    let texts: Vec<String> = rows
        .iter()
        .map(|r| enc.format_document(r.title.as_deref(), &r.text))
        .collect();
    let fresh = encoder.embed(&texts, InputKind::Document)?;
    let mut cos: Vec<f64> = rows
        .iter()
        .zip(&fresh)
        .map(|(r, e)| cosine(&e.vector, &stored[&r.item_id]) as f64)
        .collect();
    cos.sort_by(f64::total_cmp);
    let mean = cos.iter().sum::<f64>() / cos.len() as f64;
    let p05 = cos[(cos.len() as f64 * 0.05) as usize];
    let record = ParityRecord {
        space_id: table.space_id.clone(),
        encoder_space_id: enc.space_id(),
        sample: cos.len() as i64,
        mean_cos: mean,
        min_cos: cos[0],
        p05_cos: p05,
        passed: mean >= MIN_MEAN_COS && cos[0] >= MIN_COS,
    };
    let truncated = fresh.iter().filter(|e| e.truncated).count();
    project.store.record_parity(
        &record,
        &serde_json::json!({"min_mean_cos": MIN_MEAN_COS, "min_cos": MIN_COS,
                            "truncated_inputs": truncated, "encoder_runtime": enc.runtime}),
    )?;
    Ok(ParityReport {
        record,
        table: table.table_name.clone(),
        encoder_runtime: enc.runtime,
        stored_runtime: table.identity.runtime.clone(),
        thresholds: (MIN_MEAN_COS, MIN_COS),
    })
}
