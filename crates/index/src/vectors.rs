//! LanceDB vector tables: one per collection × embedding space (SPEC §7.4), local files only.
//!
//! Every table's Arrow schema carries its space id and full identity as metadata, and opening a
//! table checks that metadata against the project store, so vectors from different spaces can
//! never share a table.

use anyhow::{Context, Result, bail};
use arrow_array::{
    Array, FixedSizeListArray, Float32Array, Int64Array, RecordBatch, StringArray,
    types::Float32Type,
};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use futures::TryStreamExt;
use invectus_core::store::VectorTableRecord;
use invectus_core::{EmbeddingIdentity, Project};
use lancedb::DistanceType;
use lancedb::index::Index;
use lancedb::index::vector::IvfFlatIndexBuilder;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

pub const META_SPACE: &str = "invectus.space_id";
pub const META_IDENTITY: &str = "invectus.identity";

#[derive(Clone, Debug)]
pub struct VectorRow {
    pub item_id: i64,
    pub parent_id: Option<i64>,
    pub input_hash: String,
    pub vector: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scored {
    pub item_id: i64,
    pub parent_id: Option<i64>,
    /// Exact cosine similarity against the stored vector.
    pub similarity: f32,
}

pub struct VectorStore {
    rt: tokio::runtime::Runtime,
    db: lancedb::Connection,
}

pub fn table_name(collection_id: &str, identity: &EmbeddingIdentity) -> String {
    format!("{collection_id}__{}", identity.short_space_id())
}

fn schema(identity: &EmbeddingIdentity) -> SchemaRef {
    let dims = identity.dimensions as i32;
    let metadata = HashMap::from([
        (META_SPACE.to_string(), identity.space_id()),
        (
            META_IDENTITY.to_string(),
            serde_json::to_string(identity).expect("identity serializes"),
        ),
    ]);
    Arc::new(Schema::new_with_metadata(
        vec![
            Field::new("item_id", DataType::Int64, false),
            Field::new("parent_id", DataType::Int64, true),
            Field::new("input_hash", DataType::Utf8, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    dims,
                ),
                false,
            ),
        ],
        metadata,
    ))
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

impl VectorStore {
    pub fn open(project: &Project) -> Result<Self> {
        Self::open_dir(&project.vectors_dir())
    }

    /// Connect to a local LanceDB directory. Only filesystem paths are accepted.
    pub fn open_dir(dir: &Path) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let uri = dir.to_str().context("vector path is not valid UTF-8")?;
        if uri.contains("://") {
            bail!("only local vector stores are supported");
        }
        let db = rt.block_on(lancedb::connect(uri).execute())?;
        Ok(Self { rt, db })
    }

    pub fn table_names(&self) -> Result<Vec<String>> {
        Ok(self.rt.block_on(self.db.table_names().execute())?)
    }

    /// Create the table for `(collection, identity)` if needed and register it in the store.
    pub fn ensure_table(
        &self,
        project: &Project,
        collection_id: &str,
        identity: &EmbeddingIdentity,
        origin: &str,
    ) -> Result<VectorTableRecord> {
        let name = table_name(collection_id, identity);
        if !self.table_names()?.contains(&name) {
            self.rt.block_on(
                self.db
                    .create_empty_table(&name, schema(identity))
                    .execute(),
            )?;
        }
        let rec = VectorTableRecord {
            collection_id: collection_id.into(),
            space_id: identity.space_id(),
            table_name: name,
            origin: origin.into(),
            identity: identity.clone(),
            ann: None,
        };
        project.store.register_vector_table(&rec)?;
        let stored = project
            .store
            .vector_tables(Some(collection_id))?
            .into_iter()
            .find(|r| r.space_id == rec.space_id)
            .context("vector table was not registered")?;
        if stored.origin != origin {
            bail!(
                "vector table {} is {} vectors; it cannot also hold {origin} vectors",
                stored.table_name,
                stored.origin
            );
        }
        self.check(&stored)?;
        Ok(stored)
    }

    fn open_table(&self, name: &str) -> Result<lancedb::Table> {
        Ok(self.rt.block_on(self.db.open_table(name).execute())?)
    }

    /// Verify the table's embedded identity matches its registration.
    pub fn check(&self, rec: &VectorTableRecord) -> Result<()> {
        let t = self.open_table(&rec.table_name)?;
        let schema = self.rt.block_on(t.schema())?;
        let got = schema
            .metadata()
            .get(META_SPACE)
            .cloned()
            .unwrap_or_default();
        if got != rec.space_id {
            bail!(
                "vector table {} belongs to space {got:?}, not {}",
                rec.table_name,
                rec.space_id
            );
        }
        Ok(())
    }

    pub fn count(&self, rec: &VectorTableRecord) -> Result<usize> {
        let t = self.open_table(&rec.table_name)?;
        Ok(self.rt.block_on(t.count_rows(None))?)
    }

    /// `item_id -> input_hash` of stored vectors (key columns only, no vectors).
    pub fn stored_hashes(&self, rec: &VectorTableRecord) -> Result<HashMap<i64, String>> {
        let t = self.open_table(&rec.table_name)?;
        let batches: Vec<RecordBatch> = self.rt.block_on(async {
            t.query()
                .select(Select::columns(&["item_id", "input_hash"]))
                .execute()
                .await?
                .try_collect()
                .await
        })?;
        let mut out = HashMap::new();
        for b in &batches {
            let ids = col::<Int64Array>(b, "item_id")?;
            let hashes = col::<StringArray>(b, "input_hash")?;
            for i in 0..b.num_rows() {
                out.insert(ids.value(i), hashes.value(i).to_string());
            }
        }
        Ok(out)
    }

    pub fn add(&self, rec: &VectorTableRecord, rows: &[VectorRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let dims = rec.identity.dimensions;
        for r in rows {
            if r.vector.len() != dims || r.vector.iter().any(|x| !x.is_finite()) {
                bail!("item {}: vector is not {dims} finite floats", r.item_id);
            }
        }
        let vectors = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            rows.iter()
                .map(|r| Some(r.vector.iter().copied().map(Some))),
            dims as i32,
        );
        let batch = RecordBatch::try_new(
            schema(&rec.identity),
            vec![
                Arc::new(Int64Array::from_iter_values(rows.iter().map(|r| r.item_id))),
                Arc::new(Int64Array::from_iter(rows.iter().map(|r| r.parent_id))),
                Arc::new(StringArray::from_iter_values(
                    rows.iter().map(|r| r.input_hash.as_str()),
                )),
                Arc::new(vectors),
            ],
        )?;
        let t = self.open_table(&rec.table_name)?;
        self.rt.block_on(t.add(batch).execute())?;
        Ok(())
    }

    pub fn delete_ids(&self, rec: &VectorTableRecord, ids: &[i64]) -> Result<()> {
        let t = self.open_table(&rec.table_name)?;
        for part in ids.chunks(1000) {
            let list = part
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let predicate = format!("item_id IN ({list})");
            self.rt.block_on(t.delete(&predicate))?;
        }
        Ok(())
    }

    /// Build (or replace) a cosine IVF_FLAT index. Returns the number of partitions.
    pub fn build_ann(&self, rec: &VectorTableRecord, rows: usize) -> Result<u32> {
        let partitions = ((rows as f64).sqrt().round() as u32).clamp(8, 1024);
        let t = self.open_table(&rec.table_name)?;
        self.rt.block_on(
            t.create_index(
                &["vector"],
                Index::IvfFlat(
                    IvfFlatIndexBuilder::default()
                        .num_partitions(partitions)
                        .distance_type(DistanceType::Cosine),
                ),
            )
            .replace(true)
            .execute(),
        )?;
        Ok(partitions)
    }

    pub fn has_ann(&self, rec: &VectorTableRecord) -> Result<bool> {
        let t = self.open_table(&rec.table_name)?;
        Ok(!self.rt.block_on(t.list_indices())?.is_empty())
    }

    /// Approximate candidates from the IVF index (plus unindexed rows, which Lance scans flat),
    /// re-scored exactly against the returned stored vectors. Candidates the index misses are
    /// not recovered: use [`Self::exhaustive`] for absence checks.
    pub fn ann(
        &self,
        rec: &VectorTableRecord,
        query: &[f32],
        candidates: usize,
        nprobes: usize,
    ) -> Result<Vec<Scored>> {
        let t = self.open_table(&rec.table_name)?;
        let batches: Vec<RecordBatch> = self.rt.block_on(async {
            t.query()
                .nearest_to(query)?
                .distance_type(DistanceType::Cosine)
                .nprobes(nprobes)
                .select(Select::columns(&["item_id", "parent_id", "vector"]))
                .limit(candidates)
                .execute()
                .await?
                .try_collect()
                .await
        })?;
        let mut out = Vec::new();
        for b in &batches {
            scan_batch(b, query, &mut |s| out.push(s))?;
        }
        out.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
        Ok(out)
    }

    /// Exact top-`k` over every stored vector (optionally restricted to `allowed` ids). This is
    /// the high-recall verification route: nothing is pruned by an index.
    pub fn exhaustive(
        &self,
        rec: &VectorTableRecord,
        query: &[f32],
        k: usize,
        allowed: Option<&HashSet<i64>>,
    ) -> Result<(Vec<Scored>, usize)> {
        let t = self.open_table(&rec.table_name)?;
        let mut stream = self.rt.block_on(
            t.query()
                .select(Select::columns(&["item_id", "parent_id", "vector"]))
                .execute(),
        )?;
        let mut all: Vec<Scored> = Vec::new();
        let mut scanned = 0usize;
        while let Some(b) = self.rt.block_on(stream.try_next())? {
            scanned += b.num_rows();
            scan_batch(&b, query, &mut |s| {
                if allowed.is_none_or(|a| a.contains(&s.item_id)) {
                    all.push(s);
                }
            })?;
        }
        let by_sim = |a: &Scored, b: &Scored| {
            b.similarity
                .total_cmp(&a.similarity)
                .then(a.item_id.cmp(&b.item_id))
        };
        if k < all.len() {
            all.select_nth_unstable_by(k, by_sim);
            all.truncate(k);
        }
        all.sort_by(by_sim);
        Ok((all, scanned))
    }

    /// Stream an arbitrary table of this (possibly external) store, read-only, in batches.
    pub fn scan(
        &self,
        table: &str,
        columns: &[&str],
        limit: Option<usize>,
        f: &mut dyn FnMut(RecordBatch) -> Result<()>,
    ) -> Result<()> {
        let t = self.open_table(table)?;
        let mut q = t.query().select(Select::columns(columns));
        if let Some(n) = limit {
            q = q.limit(n);
        }
        let mut stream = self.rt.block_on(q.execute())?;
        while let Some(b) = self.rt.block_on(stream.try_next())? {
            f(b)?;
        }
        Ok(())
    }

    /// Every top-level item vector (chunks excluded): ids, sorted, and a row-major matrix.
    pub fn parent_matrix(&self, rec: &VectorTableRecord) -> Result<(Vec<i64>, Vec<f32>)> {
        let t = self.open_table(&rec.table_name)?;
        let mut stream = self.rt.block_on(
            t.query()
                .only_if("parent_id IS NULL")
                .select(Select::columns(&["item_id", "vector"]))
                .execute(),
        )?;
        let dim = rec.identity.dimensions;
        let mut rows: Vec<(i64, Vec<f32>)> = Vec::new();
        while let Some(b) = self.rt.block_on(stream.try_next())? {
            let ids = col::<Int64Array>(&b, "item_id")?;
            let vecs = col::<FixedSizeListArray>(&b, "vector")?;
            let values = vecs
                .values()
                .as_any()
                .downcast_ref::<Float32Array>()
                .context("vector values are not f32")?
                .values();
            for i in 0..b.num_rows() {
                let start = vecs.value_offset(i) as usize;
                rows.push((ids.value(i), values[start..start + dim].to_vec()));
            }
        }
        rows.sort_by_key(|r| r.0);
        let ids = rows.iter().map(|r| r.0).collect();
        let matrix = rows.into_iter().flat_map(|r| r.1).collect();
        Ok((ids, matrix))
    }

    /// Stored vectors for specific ids.
    pub fn vectors_for(
        &self,
        rec: &VectorTableRecord,
        ids: &[i64],
    ) -> Result<HashMap<i64, Vec<f32>>> {
        let t = self.open_table(&rec.table_name)?;
        let mut out = HashMap::new();
        for part in ids.chunks(1000) {
            let list = part
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let batches: Vec<RecordBatch> = self.rt.block_on(async {
                t.query()
                    .only_if(format!("item_id IN ({list})"))
                    .select(Select::columns(&["item_id", "vector"]))
                    .execute()
                    .await?
                    .try_collect()
                    .await
            })?;
            for b in &batches {
                let ids = col::<Int64Array>(b, "item_id")?;
                let vecs = col::<FixedSizeListArray>(b, "vector")?;
                for i in 0..b.num_rows() {
                    let v = vecs.value(i);
                    let v = v
                        .as_any()
                        .downcast_ref::<Float32Array>()
                        .context("vector type")?;
                    out.insert(ids.value(i), v.values().to_vec());
                }
            }
        }
        Ok(out)
    }
}

fn col<'a, T: 'static>(b: &'a RecordBatch, name: &str) -> Result<&'a T> {
    b.column_by_name(name)
        .with_context(|| format!("missing column {name}"))?
        .as_any()
        .downcast_ref::<T>()
        .with_context(|| format!("unexpected type for column {name}"))
}

fn scan_batch(b: &RecordBatch, query: &[f32], f: &mut dyn FnMut(Scored)) -> Result<()> {
    let ids = col::<Int64Array>(b, "item_id")?;
    let parents = col::<Int64Array>(b, "parent_id")?;
    let vecs = col::<FixedSizeListArray>(b, "vector")?;
    let dim = vecs.value_length() as usize;
    if dim != query.len() {
        bail!("query has {} dimensions, table has {dim}", query.len());
    }
    let values = vecs
        .values()
        .as_any()
        .downcast_ref::<Float32Array>()
        .context("vector values are not f32")?
        .values();
    for i in 0..b.num_rows() {
        let start = vecs.value_offset(i) as usize;
        let v = &values[start..start + dim];
        f(Scored {
            item_id: ids.value(i),
            parent_id: (!parents.is_null(i)).then(|| parents.value(i)),
            similarity: cosine(query, v),
        });
    }
    Ok(())
}
