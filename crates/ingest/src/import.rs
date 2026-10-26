//! `invectus import`: source → mapped items (+ chunks) → `project.duckdb`. Idempotent: item ids
//! derive from collection and source id, and unchanged rows are skipped by content hash.

use crate::chunk::ChunkPolicy;
use crate::mapping::{MappedRow, Mapping, Resolved};
use crate::source::{self, Source};
use anyhow::{Result, bail};
use invectus_core::item::{item_id, validate_collection_name};
use invectus_core::store::{CollectionRecord, WriteCounts};
use invectus_core::{Item, Project};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

const WRITE_BATCH: usize = 5_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportRequest {
    pub collection: String,
    pub source: Source,
    pub mapping: Mapping,
    pub chunking: ChunkPolicy,
    /// Read at most this many source rows.
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ImportReport {
    pub collection: String,
    pub source_rows: usize,
    pub items: usize,
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub chunks_written: usize,
    pub items_chunked: usize,
    /// Reason → count for rows that were not imported.
    pub skipped: BTreeMap<String, usize>,
    /// Items already in the collection that this source did not contain (kept, not deleted).
    pub not_in_source: usize,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Preview {
    pub columns: Vec<String>,
    pub rows: Vec<MappedRow>,
    pub skipped: BTreeMap<String, usize>,
}

/// Map the first `n` source rows without writing anything (the import wizard's preview).
pub fn preview(req: &ImportRequest, n: usize) -> Result<Preview> {
    req.chunking.validate()?;
    let table = source::read(&req.source, Some(n))?;
    let resolved = Resolved::new(&req.mapping, &table)?;
    let mut skipped = BTreeMap::new();
    let mut rows = Vec::new();
    for (i, row) in table.rows.iter().enumerate() {
        match resolved.map(i, row) {
            Some(r) => rows.push(r),
            None => *skipped.entry("empty text".to_string()).or_insert(0) += 1,
        }
    }
    for (_, reason) in &table.unreadable {
        *skipped.entry(reason.clone()).or_insert(0) += 1;
    }
    Ok(Preview {
        columns: table.columns,
        rows,
        skipped,
    })
}

pub fn import(project: &mut Project, req: &ImportRequest) -> Result<ImportReport> {
    validate_collection_name(&req.collection)?;
    req.chunking.validate()?;
    if let Some(existing) = project.store.collection(&req.collection)? {
        let existing_policy: Option<ChunkPolicy> = serde_json::from_value(existing.chunking).ok();
        if existing_policy.is_some_and(|p| p != req.chunking) {
            bail!(
                "collection {:?} was imported with a different chunk policy; use a new collection \
                 to change it",
                req.collection
            );
        }
    }
    let table = source::read(&req.source, req.limit)?;
    let resolved = Resolved::new(&req.mapping, &table)?;
    let mut report = ImportReport {
        collection: req.collection.clone(),
        source_rows: table.rows.len(),
        ..Default::default()
    };
    if req.mapping.id.is_none() {
        report.warnings.push(
            "no id column mapped: rows are identified by position, so editing the source may \
             re-import rows as new items"
                .into(),
        );
    }
    for (path, reason) in &table.unreadable {
        *report.skipped.entry(reason.clone()).or_insert(0) += 1;
        if report.warnings.len() < 20 {
            report.warnings.push(format!("skipped {path}: {reason}"));
        }
    }
    project.store.upsert_collection(&CollectionRecord {
        collection_id: req.collection.clone(),
        source: serde_json::to_value(&req.source)?,
        mapping: serde_json::to_value(&req.mapping)?,
        chunking: serde_json::to_value(req.chunking)?,
    })?;
    let existing: HashSet<i64> = project
        .store
        .item_hashes(&req.collection)?
        .into_keys()
        .collect();

    let mut seen = HashSet::new();
    let mut unparsed_dates = 0;
    let mut batch = Vec::with_capacity(WRITE_BATCH);
    let mut totals = WriteCounts::default();
    for (i, row) in table.rows.iter().enumerate() {
        let Some(mapped) = resolved.map(i, row) else {
            *report.skipped.entry("empty text".into()).or_insert(0) += 1;
            continue;
        };
        if !seen.insert(mapped.source_id.clone()) {
            *report.skipped.entry("duplicate id".into()).or_insert(0) += 1;
            continue;
        }
        if mapped.unparsed_date.is_some() {
            unparsed_dates += 1;
        }
        let (item, chunks) = build_item(&req.collection, &req.chunking, mapped);
        if !chunks.is_empty() {
            report.items_chunked += 1;
        }
        report.items += 1;
        batch.push((item, chunks));
        if batch.len() == WRITE_BATCH {
            add(&mut totals, project.store.write_items(&batch)?);
            batch.clear();
        }
    }
    add(&mut totals, project.store.write_items(&batch)?);
    if unparsed_dates > 0 {
        report.warnings.push(format!(
            "{unparsed_dates} date value(s) could not be parsed; stored as no date"
        ));
    }
    let seen_ids: HashSet<i64> = seen
        .iter()
        .map(|s| item_id(&req.collection, s, None))
        .collect();
    report.not_in_source = existing.difference(&seen_ids).count();
    report.inserted = totals.inserted;
    report.updated = totals.updated;
    report.unchanged = totals.unchanged;
    report.chunks_written = totals.chunks_written;
    Ok(report)
}

fn add(t: &mut WriteCounts, c: WriteCounts) {
    t.inserted += c.inserted;
    t.updated += c.updated;
    t.unchanged += c.unchanged;
    t.chunks_written += c.chunks_written;
}

/// Build the stored item and its chunks. The chunk policy is part of the parent's hash, so a
/// policy change would rebuild chunks.
pub fn build_item(collection: &str, policy: &ChunkPolicy, r: MappedRow) -> (Item, Vec<Item>) {
    let id = item_id(collection, &r.source_id, None);
    let hash_meta = serde_json::json!({"meta": r.meta, "chunking": policy});
    let hash = Item::content_hash(
        r.title.as_deref(),
        &r.text,
        r.url.as_deref(),
        r.date.as_deref(),
        r.role,
        &hash_meta,
    );
    let chunks = policy
        .split(&r.text)
        .into_iter()
        .enumerate()
        .map(|(i, text)| Item {
            item_id: item_id(collection, &r.source_id, Some(i)),
            collection_id: collection.into(),
            source_id: format!("{}#chunk{i}", r.source_id),
            title: r.title.clone(),
            input_hash: Item::content_hash(
                r.title.as_deref(),
                &text,
                None,
                None,
                r.role,
                &serde_json::Value::Null,
            ),
            text,
            url: r.url.clone(),
            date: r.date.clone(),
            role: r.role,
            meta: serde_json::Value::Null,
            parent_id: Some(id),
            chunk_index: Some(i as i32),
        })
        .collect();
    let item = Item {
        item_id: id,
        collection_id: collection.into(),
        source_id: r.source_id,
        title: r.title,
        text: r.text,
        url: r.url,
        date: r.date,
        role: r.role,
        meta: r.meta,
        parent_id: None,
        chunk_index: None,
        input_hash: hash,
    };
    (item, chunks)
}
