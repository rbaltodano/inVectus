//! `project.duckdb`: items, collections, embedding spaces, parity checks and jobs (SPEC §7.5).

use crate::identity::EmbeddingIdentity;
use crate::item::{Item, Role};
use anyhow::{Context, Result, bail};
use duckdb::{AccessMode, Config, Connection, OptionalExt, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER);
CREATE TABLE IF NOT EXISTS collections (
  collection_id TEXT PRIMARY KEY, source JSON, mapping JSON, chunking JSON,
  created_at TIMESTAMP, updated_at TIMESTAMP);
CREATE TABLE IF NOT EXISTS items (
  item_id BIGINT PRIMARY KEY, collection_id TEXT, source_id TEXT, title TEXT, text TEXT,
  url TEXT, date DATE, role TEXT, meta JSON, parent_id BIGINT, chunk_index INTEGER,
  input_hash TEXT, imported_at TIMESTAMP);
CREATE TABLE IF NOT EXISTS embedding_spaces (
  space_id TEXT PRIMARY KEY, identity JSON, created_at TIMESTAMP);
CREATE TABLE IF NOT EXISTS vector_tables (
  collection_id TEXT, space_id TEXT, table_name TEXT, origin TEXT, created_at TIMESTAMP,
  ann JSON, PRIMARY KEY (collection_id, space_id));
CREATE TABLE IF NOT EXISTS parity_checks (
  space_id TEXT, encoder_space_id TEXT, sample INTEGER, mean_cos DOUBLE, min_cos DOUBLE,
  p05_cos DOUBLE, passed BOOLEAN, details JSON, checked_at TIMESTAMP);
CREATE TABLE IF NOT EXISTS jobs (
  job_id TEXT PRIMARY KEY, kind TEXT, params JSON, state TEXT, progress REAL,
  checkpoint JSON, updated_at TIMESTAMP);
CREATE TABLE IF NOT EXISTS calibrations (
  collection_id TEXT, space_id TEXT, method TEXT, sample INTEGER, vectors INTEGER,
  curve JSON, metrics JSON, created_at TIMESTAMP);
CREATE TABLE IF NOT EXISTS region_runs (
  run_id TEXT PRIMARY KEY, collection_id TEXT, space_id TEXT, params JSON, stats JSON,
  created_at TIMESTAMP);
CREATE TABLE IF NOT EXISTS regions (
  run_id TEXT, region_id INTEGER, level TEXT, parent_id INTEGER, label TEXT, terms JSON,
  size INTEGER, stability DOUBLE, centroid BLOB, x DOUBLE, y DOUBLE, radius DOUBLE);
CREATE TABLE IF NOT EXISTS region_members (
  run_id TEXT, item_id BIGINT, fine_id INTEGER, broad_id INTEGER, noise BOOLEAN,
  centroid_sim REAL, x REAL, y REAL);
CREATE TABLE IF NOT EXISTS gap_scans (
  run_id TEXT PRIMARY KEY, params JSON, stats JSON, created_at TIMESTAMP);
CREATE TABLE IF NOT EXISTS gaps (
  run_id TEXT, gap_id INTEGER, kind TEXT, score DOUBLE, data JSON);
CREATE TABLE IF NOT EXISTS gap_verdicts (
  collection_id TEXT, space_id TEXT, kind TEXT, core JSON, verdict TEXT, note TEXT,
  updated_at TIMESTAMP);
"#;

/// Settings applied to every DuckDB connection: never fetch extensions from the network.
pub fn harden(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "SET autoinstall_known_extensions = false; SET autoload_known_extensions = false;",
    )?;
    Ok(())
}

/// Open an in-memory DuckDB with the same network hardening (used to read CSV/Parquet).
pub fn scratch_connection() -> Result<Connection> {
    let conn =
        Connection::open_in_memory_with_flags(Config::default().enable_autoload_extension(false)?)?;
    harden(&conn)?;
    Ok(conn)
}

/// Open another DuckDB file strictly read-only (e.g. the gapmap reference database).
pub fn open_read_only(path: &Path) -> Result<Connection> {
    if !path.is_file() {
        bail!("{} does not exist", path.display());
    }
    let cfg = Config::default()
        .access_mode(AccessMode::ReadOnly)?
        .enable_autoload_extension(false)?;
    let conn = Connection::open_with_flags(path, cfg)
        .with_context(|| format!("opening {} read-only", path.display()))?;
    harden(&conn)?;
    Ok(conn)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollectionRecord {
    pub collection_id: String,
    pub source: serde_json::Value,
    pub mapping: serde_json::Value,
    pub chunking: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollectionSummary {
    pub collection_id: String,
    pub items: i64,
    pub chunks: i64,
    pub by_role: HashMap<String, i64>,
    pub source: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VectorTableRecord {
    pub collection_id: String,
    pub space_id: String,
    pub table_name: String,
    /// `embedded` (computed here) or `imported` (copied from another tool; query-only).
    pub origin: String,
    pub identity: EmbeddingIdentity,
    /// ANN index parameters when one was built (`{"partitions": n, "rows": n, ...}`).
    pub ann: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParityRecord {
    pub space_id: String,
    pub encoder_space_id: String,
    pub sample: i64,
    pub mean_cos: f64,
    pub min_cos: f64,
    pub p05_cos: f64,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobRecord {
    pub job_id: String,
    pub kind: String,
    pub params: serde_json::Value,
    pub state: String,
    pub progress: f64,
    pub checkpoint: serde_json::Value,
}

/// A fitted similarity → P(same thing) curve for one collection and vector space (SPEC §6.2).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalibrationRecord {
    pub collection_id: String,
    pub space_id: String,
    /// How positive and negative pairs were made, e.g. `title-vs-text (no LLM)`.
    pub method: String,
    /// Items sampled.
    pub sample: i64,
    /// Vectors in the table when fitted; a different count means the collection changed.
    pub vectors: i64,
    pub curve: serde_json::Value,
    pub metrics: serde_json::Value,
    /// Set by the store when read back.
    #[serde(default)]
    pub created_at: Option<String>,
}

/// One clustering of a collection's vector space (SPEC §6.4). Only the latest run per collection
/// and space keeps its regions and members; earlier run records stay as history.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegionRun {
    pub run_id: String,
    pub collection_id: String,
    pub space_id: String,
    pub params: serde_json::Value,
    pub stats: serde_json::Value,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegionRecord {
    pub region_id: i32,
    /// `fine` or `broad`.
    pub level: String,
    /// Broad region of a fine region.
    pub parent_id: Option<i32>,
    pub label: String,
    pub terms: Vec<String>,
    pub size: i64,
    /// Mean best-Jaccard match across seeds (fine regions); `None` for broad regions.
    pub stability: Option<f64>,
    /// Unit centroid vector.
    #[serde(skip)]
    pub centroid: Vec<f32>,
    /// Map position and radius (display only).
    pub x: f64,
    pub y: f64,
    pub radius: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct MemberRecord {
    pub item_id: i64,
    pub fine_id: i32,
    pub broad_id: i32,
    /// In a community too small to be a region; attached to the nearest region.
    pub noise: bool,
    /// Cosine similarity to the fine region's centroid.
    pub centroid_sim: f32,
    pub x: f32,
    pub y: f32,
}

/// One scored gap of a scan; `data` holds the full gap (terms, explanation, timeline).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GapRecord {
    pub gap_id: i32,
    pub kind: String,
    pub score: f64,
    pub data: serde_json::Value,
}

/// A human decision on a gap (SPEC §3). Matched to gaps of later scans by their core items, so
/// verdicts survive re-runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GapVerdictRecord {
    pub collection_id: String,
    pub space_id: String,
    pub kind: String,
    pub core: Vec<i64>,
    /// open, confirmed_gap, already_exists, not_viable or selected.
    pub verdict: String,
    pub note: String,
    #[serde(default)]
    pub updated_at: Option<String>,
}

pub const GAP_VERDICTS: &[&str] = &[
    "open",
    "confirmed_gap",
    "already_exists",
    "not_viable",
    "selected",
];

fn f32s_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn blob_to_f32s(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// The role and date of an item.
#[derive(Clone, Debug)]
pub struct ItemFacts {
    pub role: Role,
    pub date: Option<String>,
}

/// Row needed to embed an item.
#[derive(Clone, Debug)]
pub struct EmbedRow {
    pub item_id: i64,
    pub parent_id: Option<i64>,
    pub title: Option<String>,
    pub text: String,
}

/// What an import changed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WriteCounts {
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub chunks_written: usize,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            Config::default().enable_autoload_extension(false)?,
        )
        .with_context(|| {
            format!(
                "opening {} (a project can be used by one inVectus process at a time; close the \
                 desktop app or other `invectus` commands using it)",
                path.display()
            )
        })?;
        harden(&conn)?;
        conn.execute_batch(SCHEMA)?;
        let v: Option<i32> = conn
            .query_row("SELECT max(version) FROM schema_version", [], |r| r.get(0))
            .optional()?
            .flatten();
        match v {
            None => {
                conn.execute("INSERT INTO schema_version VALUES (1)", [])?;
            }
            Some(1) => {}
            Some(other) => bail!("project schema version {other} is newer than this build"),
        }
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    // ---- collections -------------------------------------------------------------------

    pub fn upsert_collection(&self, c: &CollectionRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO collections VALUES (?, ?, ?, ?, now(), now())
             ON CONFLICT (collection_id) DO UPDATE SET source = excluded.source,
               mapping = excluded.mapping, chunking = excluded.chunking, updated_at = now()",
            params![
                c.collection_id,
                c.source.to_string(),
                c.mapping.to_string(),
                c.chunking.to_string()
            ],
        )?;
        Ok(())
    }

    pub fn collection(&self, id: &str) -> Result<Option<CollectionRecord>> {
        self.conn
            .query_row(
                "SELECT collection_id, source::VARCHAR, mapping::VARCHAR, chunking::VARCHAR
                 FROM collections WHERE collection_id = ?",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?
            .map(|(collection_id, s, m, c)| {
                Ok(CollectionRecord {
                    collection_id,
                    source: serde_json::from_str(&s)?,
                    mapping: serde_json::from_str(&m)?,
                    chunking: serde_json::from_str(&c)?,
                })
            })
            .transpose()
    }

    pub fn require_collection(&self, id: &str) -> Result<CollectionRecord> {
        self.collection(id)?.with_context(|| {
            let known = self.collection_ids().unwrap_or_default().join(", ");
            format!("no collection {id:?} (known: {known})")
        })
    }

    pub fn collection_ids(&self) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT collection_id FROM collections ORDER BY collection_id")?;
        let ids = st
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(ids)
    }

    pub fn collection_summaries(&self) -> Result<Vec<CollectionSummary>> {
        let mut out = Vec::new();
        for id in self.collection_ids()? {
            let c = self.require_collection(&id)?;
            let (items, chunks): (i64, i64) = self.conn.query_row(
                "SELECT count(*) FILTER (WHERE parent_id IS NULL),
                        count(*) FILTER (WHERE parent_id IS NOT NULL)
                 FROM items WHERE collection_id = ?",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let mut st = self.conn.prepare(
                "SELECT role, count(*) FROM items
                 WHERE collection_id = ? AND parent_id IS NULL GROUP BY role",
            )?;
            let by_role = st
                .query_map([&id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            out.push(CollectionSummary {
                collection_id: id,
                items,
                chunks,
                by_role,
                source: c.source,
            });
        }
        Ok(out)
    }

    // ---- items -------------------------------------------------------------------------

    /// `item_id -> input_hash` for top-level items of a collection.
    pub fn item_hashes(&self, collection_id: &str) -> Result<HashMap<i64, String>> {
        let mut st = self.conn.prepare(
            "SELECT item_id, input_hash FROM items WHERE collection_id = ? AND parent_id IS NULL",
        )?;
        let m = st
            .query_map([collection_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(m)
    }

    /// Insert new items and replace changed ones (with their chunks) in one transaction.
    /// `batch` holds each top-level item followed by its chunks; unchanged items are skipped.
    pub fn write_items(&mut self, batch: &[(Item, Vec<Item>)]) -> Result<WriteCounts> {
        let existing = match batch.first() {
            Some((first, _)) => self.item_hashes(&first.collection_id)?,
            None => return Ok(WriteCounts::default()),
        };
        let mut counts = WriteCounts::default();
        let tx = self.conn.transaction()?;
        {
            let mut del = tx.prepare("DELETE FROM items WHERE item_id = ? OR parent_id = ?")?;
            let mut ins = tx.prepare(
                "INSERT INTO items VALUES (?, ?, ?, ?, ?, ?, CAST(? AS DATE), ?, CAST(? AS JSON),
                 ?, ?, ?, now())",
            )?;
            for (item, chunks) in batch {
                match existing.get(&item.item_id) {
                    Some(h) if *h == item.input_hash => {
                        counts.unchanged += 1;
                        continue;
                    }
                    Some(_) => {
                        del.execute(params![item.item_id, item.item_id])?;
                        counts.updated += 1;
                    }
                    None => counts.inserted += 1,
                }
                for it in std::iter::once(item).chain(chunks.iter()) {
                    ins.execute(params![
                        it.item_id,
                        it.collection_id,
                        it.source_id,
                        it.title,
                        it.text,
                        it.url,
                        it.date,
                        it.role.as_str(),
                        it.meta.to_string(),
                        it.parent_id,
                        it.chunk_index,
                        it.input_hash,
                    ])
                    .with_context(|| format!("inserting item {:?}", it.source_id))?;
                }
                counts.chunks_written += chunks.len();
            }
        }
        tx.commit()?;
        Ok(counts)
    }

    /// Items (and chunks) to embed for a collection, ordered by id.
    pub fn embed_rows(&self, collection_id: &str) -> Result<Vec<EmbedRow>> {
        let mut st = self.conn.prepare(
            "SELECT item_id, parent_id, title, text FROM items WHERE collection_id = ?
             ORDER BY item_id",
        )?;
        let rows = st
            .query_map([collection_id], |r| {
                Ok(EmbedRow {
                    item_id: r.get(0)?,
                    parent_id: r.get(1)?,
                    title: r.get(2)?,
                    text: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    pub fn items_by_ids(&self, ids: &[i64]) -> Result<HashMap<i64, Item>> {
        let mut out = HashMap::new();
        if ids.is_empty() {
            return Ok(out);
        }
        // Chunked IN-lists keep statements small; ids are integers, so formatting is safe.
        for part in ids.chunks(1000) {
            let list = part
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let mut st = self.conn.prepare(&format!(
                "SELECT item_id, collection_id, source_id, title, text, url, CAST(date AS VARCHAR),
                        role, meta::VARCHAR, parent_id, chunk_index, input_hash
                 FROM items WHERE item_id IN ({list})"
            ))?;
            let rows = st.query_map([], |r| {
                Ok((
                    Item {
                        item_id: r.get(0)?,
                        collection_id: r.get(1)?,
                        source_id: r.get(2)?,
                        title: r.get(3)?,
                        text: r.get(4)?,
                        url: r.get(5)?,
                        date: r.get(6)?,
                        role: Role::Neutral,
                        meta: serde_json::Value::Null,
                        parent_id: r.get(9)?,
                        chunk_index: r.get(10)?,
                        input_hash: r.get(11)?,
                    },
                    r.get::<_, String>(7)?,
                    r.get::<_, Option<String>>(8)?,
                ))
            })?;
            for row in rows {
                let (mut item, role, meta) = row?;
                item.role = role.parse()?;
                item.meta = meta
                    .map(|m| serde_json::from_str(&m))
                    .transpose()?
                    .unwrap_or(serde_json::Value::Null);
                out.insert(item.item_id, item);
            }
        }
        Ok(out)
    }

    // ---- embedding spaces and vector tables ---------------------------------------------

    pub fn register_vector_table(&self, rec: &VectorTableRecord) -> Result<()> {
        let space_id = rec.identity.space_id();
        if space_id != rec.space_id {
            bail!("vector table record space id does not match its identity");
        }
        self.conn.execute(
            "INSERT INTO embedding_spaces VALUES (?, ?, now()) ON CONFLICT DO NOTHING",
            params![space_id, serde_json::to_string(&rec.identity)?],
        )?;
        self.conn.execute(
            "INSERT INTO vector_tables VALUES (?, ?, ?, ?, now(), NULL) ON CONFLICT DO NOTHING",
            params![rec.collection_id, space_id, rec.table_name, rec.origin],
        )?;
        Ok(())
    }

    pub fn vector_tables(&self, collection_id: Option<&str>) -> Result<Vec<VectorTableRecord>> {
        let mut st = self.conn.prepare(
            "SELECT v.collection_id, v.space_id, v.table_name, v.origin, s.identity::VARCHAR,
                    v.ann::VARCHAR
             FROM vector_tables v JOIN embedding_spaces s USING (space_id)
             WHERE ? IS NULL OR v.collection_id = ? ORDER BY v.collection_id, v.created_at",
        )?;
        let rows = st.query_map(params![collection_id, collection_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (collection_id, space_id, table_name, origin, identity, ann) = row?;
            let identity: EmbeddingIdentity = serde_json::from_str(&identity)?;
            if identity.space_id() != space_id {
                bail!("stored identity for space {space_id} does not hash to its id");
            }
            out.push(VectorTableRecord {
                collection_id,
                space_id,
                table_name,
                origin,
                identity,
                ann: ann.map(|a| serde_json::from_str(&a)).transpose()?,
            });
        }
        Ok(out)
    }

    pub fn set_ann(
        &self,
        collection_id: &str,
        space_id: &str,
        ann: &serde_json::Value,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE vector_tables SET ann = ? WHERE collection_id = ? AND space_id = ?",
            params![ann.to_string(), collection_id, space_id],
        )?;
        Ok(())
    }

    /// Ids of items and chunks in a collection with the given role.
    pub fn item_ids_with_role(
        &self,
        collection_id: &str,
        role: Role,
    ) -> Result<std::collections::HashSet<i64>> {
        let mut st = self
            .conn
            .prepare("SELECT item_id FROM items WHERE collection_id = ? AND role = ?")?;
        let ids = st
            .query_map(params![collection_id, role.as_str()], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(ids)
    }

    pub fn record_parity(&self, p: &ParityRecord, details: &serde_json::Value) -> Result<()> {
        self.conn.execute(
            "INSERT INTO parity_checks VALUES (?, ?, ?, ?, ?, ?, ?, ?, now())",
            params![
                p.space_id,
                p.encoder_space_id,
                p.sample,
                p.mean_cos,
                p.min_cos,
                p.p05_cos,
                p.passed,
                details.to_string()
            ],
        )?;
        Ok(())
    }

    /// Most recent parity check of `encoder_space_id` against `space_id`.
    pub fn latest_parity(
        &self,
        space_id: &str,
        encoder_space_id: &str,
    ) -> Result<Option<ParityRecord>> {
        Ok(self
            .conn
            .query_row(
                "SELECT space_id, encoder_space_id, sample, mean_cos, min_cos, p05_cos, passed
                 FROM parity_checks WHERE space_id = ? AND encoder_space_id = ?
                 ORDER BY checked_at DESC LIMIT 1",
                [space_id, encoder_space_id],
                |r| {
                    Ok(ParityRecord {
                        space_id: r.get(0)?,
                        encoder_space_id: r.get(1)?,
                        sample: r.get(2)?,
                        mean_cos: r.get(3)?,
                        min_cos: r.get(4)?,
                        p05_cos: r.get(5)?,
                        passed: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    // ---- calibration -------------------------------------------------------------------

    pub fn save_calibration(&self, c: &CalibrationRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO calibrations VALUES (?, ?, ?, ?, ?, ?, ?, now())",
            params![
                c.collection_id,
                c.space_id,
                c.method,
                c.sample,
                c.vectors,
                c.curve.to_string(),
                c.metrics.to_string()
            ],
        )?;
        Ok(())
    }

    /// Most recent calibration of a collection's vector space.
    pub fn latest_calibration(
        &self,
        collection_id: &str,
        space_id: &str,
    ) -> Result<Option<CalibrationRecord>> {
        self.conn
            .query_row(
                "SELECT collection_id, space_id, method, sample, vectors, curve::VARCHAR,
                        metrics::VARCHAR, CAST(created_at AS VARCHAR)
                 FROM calibrations WHERE collection_id = ? AND space_id = ?
                 ORDER BY created_at DESC LIMIT 1",
                [collection_id, space_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, String>(7)?,
                    ))
                },
            )
            .optional()?
            .map(
                |(collection_id, space_id, method, sample, vectors, curve, metrics, at)| {
                    Ok(CalibrationRecord {
                        collection_id,
                        space_id,
                        method,
                        sample,
                        vectors,
                        curve: serde_json::from_str(&curve)?,
                        metrics: serde_json::from_str(&metrics)?,
                        created_at: Some(at),
                    })
                },
            )
            .transpose()
    }

    // ---- regions ------------------------------------------------------------------------

    /// Save a run and replace the regions and members of earlier runs of the same collection
    /// and space, in one transaction.
    pub fn save_region_run(
        &mut self,
        run: &RegionRun,
        regions: &[RegionRecord],
        members: &[MemberRecord],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM region_members WHERE run_id IN (SELECT run_id FROM region_runs
               WHERE collection_id = ? AND space_id = ?)",
            params![run.collection_id, run.space_id],
        )?;
        tx.execute(
            "DELETE FROM regions WHERE run_id IN (SELECT run_id FROM region_runs
               WHERE collection_id = ? AND space_id = ?)",
            params![run.collection_id, run.space_id],
        )?;
        tx.execute(
            "INSERT INTO region_runs VALUES (?, ?, ?, ?, ?, now())",
            params![
                run.run_id,
                run.collection_id,
                run.space_id,
                run.params.to_string(),
                run.stats.to_string()
            ],
        )?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO regions VALUES (?, ?, ?, ?, ?, CAST(? AS JSON), ?, ?, ?, ?, ?, ?)",
            )?;
            for r in regions {
                ins.execute(params![
                    run.run_id,
                    r.region_id,
                    r.level,
                    r.parent_id,
                    r.label,
                    serde_json::to_string(&r.terms)?,
                    r.size,
                    r.stability,
                    f32s_to_blob(&r.centroid),
                    r.x,
                    r.y,
                    r.radius
                ])?;
            }
            let mut app = tx.appender("region_members")?;
            for m in members {
                app.append_row(params![
                    run.run_id,
                    m.item_id,
                    m.fine_id,
                    m.broad_id,
                    m.noise,
                    m.centroid_sim,
                    m.x,
                    m.y
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The latest region run of a collection's vector space.
    pub fn latest_region_run(
        &self,
        collection_id: &str,
        space_id: &str,
    ) -> Result<Option<RegionRun>> {
        self.conn
            .query_row(
                "SELECT run_id, collection_id, space_id, params::VARCHAR, stats::VARCHAR,
                        CAST(created_at AS VARCHAR)
                 FROM region_runs WHERE collection_id = ? AND space_id = ?
                 ORDER BY created_at DESC LIMIT 1",
                [collection_id, space_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?
            .map(|(run_id, collection_id, space_id, p, s, at)| {
                Ok(RegionRun {
                    run_id,
                    collection_id,
                    space_id,
                    params: serde_json::from_str(&p)?,
                    stats: serde_json::from_str(&s)?,
                    created_at: Some(at),
                })
            })
            .transpose()
    }

    pub fn regions(&self, run_id: &str) -> Result<Vec<RegionRecord>> {
        let mut st = self.conn.prepare(
            "SELECT region_id, level, parent_id, label, terms::VARCHAR, size, stability, centroid,
                    x, y, radius
             FROM regions WHERE run_id = ? ORDER BY level DESC, region_id",
        )?;
        let rows = st.query_map([run_id], |r| {
            Ok((
                RegionRecord {
                    region_id: r.get(0)?,
                    level: r.get(1)?,
                    parent_id: r.get(2)?,
                    label: r.get(3)?,
                    terms: Vec::new(),
                    size: r.get(5)?,
                    stability: r.get(6)?,
                    centroid: blob_to_f32s(&r.get::<_, Vec<u8>>(7)?),
                    x: r.get(8)?,
                    y: r.get(9)?,
                    radius: r.get(10)?,
                },
                r.get::<_, String>(4)?,
            ))
        })?;
        rows.map(|row| {
            let (mut rec, terms) = row?;
            rec.terms = serde_json::from_str(&terms)?;
            Ok(rec)
        })
        .collect()
    }

    pub fn region_members(&self, run_id: &str) -> Result<Vec<MemberRecord>> {
        let mut st = self.conn.prepare(
            "SELECT item_id, fine_id, broad_id, noise, centroid_sim, x, y
             FROM region_members WHERE run_id = ? ORDER BY item_id",
        )?;
        let rows = st
            .query_map([run_id], |r| {
                Ok(MemberRecord {
                    item_id: r.get(0)?,
                    fine_id: r.get(1)?,
                    broad_id: r.get(2)?,
                    noise: r.get(3)?,
                    centroid_sim: r.get(4)?,
                    x: r.get(5)?,
                    y: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    // ---- gaps ---------------------------------------------------------------------------

    /// Replace the gap scan of a region run.
    pub fn save_gaps(
        &mut self,
        run_id: &str,
        params: &serde_json::Value,
        stats: &serde_json::Value,
        gaps: &[GapRecord],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM gaps WHERE run_id = ?", [run_id])?;
        tx.execute("DELETE FROM gap_scans WHERE run_id = ?", [run_id])?;
        tx.execute(
            "INSERT INTO gap_scans VALUES (?, ?, ?, now())",
            params![run_id, params.to_string(), stats.to_string()],
        )?;
        {
            let mut ins = tx.prepare("INSERT INTO gaps VALUES (?, ?, ?, ?, CAST(? AS JSON))")?;
            for g in gaps {
                ins.execute(params![
                    run_id,
                    g.gap_id,
                    g.kind,
                    g.score,
                    g.data.to_string()
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Gaps of a region run's scan, by id (rank), with the scan's stats; `None` if not scanned.
    pub fn gaps(&self, run_id: &str) -> Result<Option<(serde_json::Value, Vec<GapRecord>)>> {
        let stats: Option<String> = self
            .conn
            .query_row(
                "SELECT stats::VARCHAR FROM gap_scans WHERE run_id = ?",
                [run_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(stats) = stats else {
            return Ok(None);
        };
        let mut st = self.conn.prepare(
            "SELECT gap_id, kind, score, data::VARCHAR FROM gaps WHERE run_id = ? ORDER BY gap_id",
        )?;
        let rows = st.query_map([run_id], |r| {
            Ok((
                r.get::<_, i32>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        let gaps = rows
            .map(|row| {
                let (gap_id, kind, score, data) = row?;
                Ok(GapRecord {
                    gap_id,
                    kind,
                    score,
                    data: serde_json::from_str(&data)?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Some((serde_json::from_str(&stats)?, gaps)))
    }

    pub fn save_gap_verdict(&self, v: &GapVerdictRecord) -> Result<()> {
        if !GAP_VERDICTS.contains(&v.verdict.as_str()) {
            bail!(
                "unknown verdict {:?} (one of: {})",
                v.verdict,
                GAP_VERDICTS.join(", ")
            );
        }
        self.conn.execute(
            "INSERT INTO gap_verdicts VALUES (?, ?, ?, CAST(? AS JSON), ?, ?, now())",
            params![
                v.collection_id,
                v.space_id,
                v.kind,
                serde_json::to_string(&v.core)?,
                v.verdict,
                v.note
            ],
        )?;
        Ok(())
    }

    /// Every verdict of a collection's space, newest first.
    pub fn gap_verdicts(
        &self,
        collection_id: &str,
        space_id: &str,
    ) -> Result<Vec<GapVerdictRecord>> {
        let mut st = self.conn.prepare(
            "SELECT kind, core::VARCHAR, verdict, note, CAST(updated_at AS VARCHAR)
             FROM gap_verdicts WHERE collection_id = ? AND space_id = ?
             ORDER BY updated_at DESC",
        )?;
        let rows = st.query_map([collection_id, space_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        rows.map(|row| {
            let (kind, core, verdict, note, at) = row?;
            Ok(GapVerdictRecord {
                collection_id: collection_id.into(),
                space_id: space_id.into(),
                kind,
                core: serde_json::from_str(&core)?,
                verdict,
                note,
                updated_at: Some(at),
            })
        })
        .collect()
    }

    /// Role and date of top-level items: what gap scans and backtests need, without loading
    /// text or metadata (gigabytes at 500k items).
    pub fn item_facts(&self, collection_id: &str) -> Result<HashMap<i64, ItemFacts>> {
        let mut st = self.conn.prepare(
            "SELECT item_id, role, CAST(date AS VARCHAR) FROM items
             WHERE collection_id = ? AND parent_id IS NULL",
        )?;
        let rows = st.query_map([collection_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, role, date) = row?;
            Ok((
                id,
                ItemFacts {
                    role: role.parse()?,
                    date,
                },
            ))
        })
        .collect()
    }

    /// Top-level items (no chunks) of a collection with their title and text.
    pub fn parent_texts(&self, collection_id: &str) -> Result<HashMap<i64, (String, String)>> {
        let mut st = self.conn.prepare(
            "SELECT item_id, coalesce(title, ''), text FROM items
             WHERE collection_id = ? AND parent_id IS NULL",
        )?;
        let m = st
            .query_map([collection_id], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(m)
    }

    /// Top-level items with a non-empty title and at least `min_text_chars` of text, ordered by
    /// id (ids are hash-derived, so this order is a stable pseudo-random sample).
    pub fn titled_items(
        &self,
        collection_id: &str,
        min_text_chars: usize,
        limit: usize,
    ) -> Result<Vec<EmbedRow>> {
        let mut st = self.conn.prepare(
            "SELECT item_id, parent_id, title, text FROM items
             WHERE collection_id = ? AND parent_id IS NULL AND length(trim(coalesce(title, ''))) > 0
               AND length(text) >= ? AND trim(text) <> trim(title)
             ORDER BY item_id LIMIT ?",
        )?;
        let rows = st
            .query_map(
                params![collection_id, min_text_chars as i64, limit as i64],
                |r| {
                    Ok(EmbedRow {
                        item_id: r.get(0)?,
                        parent_id: r.get(1)?,
                        title: r.get(2)?,
                        text: r.get(3)?,
                    })
                },
            )?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    // ---- jobs --------------------------------------------------------------------------

    pub fn save_job(&self, j: &JobRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO jobs VALUES (?, ?, ?, ?, ?, ?, now())
             ON CONFLICT (job_id) DO UPDATE SET params = excluded.params, state = excluded.state,
               progress = excluded.progress, checkpoint = excluded.checkpoint, updated_at = now()",
            params![
                j.job_id,
                j.kind,
                j.params.to_string(),
                j.state,
                j.progress,
                j.checkpoint.to_string()
            ],
        )?;
        Ok(())
    }

    pub fn job(&self, job_id: &str) -> Result<Option<JobRecord>> {
        self.conn
            .query_row(
                "SELECT job_id, kind, params::VARCHAR, state, progress, checkpoint::VARCHAR
                 FROM jobs WHERE job_id = ?",
                [job_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, f64>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?
            .map(|(job_id, kind, p, state, progress, c)| {
                Ok(JobRecord {
                    job_id,
                    kind,
                    params: serde_json::from_str(&p)?,
                    state,
                    progress,
                    checkpoint: serde_json::from_str(&c)?,
                })
            })
            .transpose()
    }

    pub fn jobs(&self) -> Result<Vec<JobRecord>> {
        let mut st = self
            .conn
            .prepare("SELECT job_id FROM jobs ORDER BY updated_at DESC")?;
        let ids: Vec<String> = st
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        ids.iter()
            .filter_map(|id| self.job(id).transpose())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::item_id;

    fn item(coll: &str, src: &str, text: &str) -> Item {
        let meta = serde_json::json!({"k": src});
        Item {
            item_id: item_id(coll, src, None),
            collection_id: coll.into(),
            source_id: src.into(),
            title: Some(format!("Title {src}")),
            text: text.into(),
            url: None,
            date: Some("2020-05-01".into()),
            role: Role::Demand,
            input_hash: Item::content_hash(None, text, None, None, Role::Demand, &meta),
            meta,
            parent_id: None,
            chunk_index: None,
        }
    }

    #[test]
    fn write_items_is_idempotent_and_detects_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("p.duckdb")).unwrap();
        let a = item("c", "1", "alpha");
        let b = item("c", "2", "beta");
        let mut chunk = item("c", "2#0", "beta part");
        chunk.parent_id = Some(b.item_id);
        chunk.chunk_index = Some(0);
        let c = s
            .write_items(&[(a.clone(), vec![]), (b.clone(), vec![chunk.clone()])])
            .unwrap();
        assert_eq!(
            (c.inserted, c.updated, c.unchanged, c.chunks_written),
            (2, 0, 0, 1)
        );
        let c = s
            .write_items(&[(a.clone(), vec![]), (b.clone(), vec![chunk.clone()])])
            .unwrap();
        assert_eq!((c.inserted, c.updated, c.unchanged), (0, 0, 2));
        // Changing b replaces it and drops its old chunk.
        let b2 = item("c", "2", "beta v2");
        let c = s.write_items(&[(b2.clone(), vec![])]).unwrap();
        assert_eq!((c.inserted, c.updated), (0, 1));
        let rows = s.embed_rows("c").unwrap();
        assert_eq!(rows.len(), 2, "old chunk removed");
        let got = s.items_by_ids(&[a.item_id, b2.item_id]).unwrap();
        assert_eq!(got[&b2.item_id].text, "beta v2");
        assert_eq!(got[&a.item_id].date.as_deref(), Some("2020-05-01"));
        assert_eq!(got[&a.item_id].meta, serde_json::json!({"k": "1"}));
        assert_eq!(got[&a.item_id].role, Role::Demand);
    }

    #[test]
    fn jobs_and_parity_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("p.duckdb")).unwrap();
        let mut j = JobRecord {
            job_id: "j".into(),
            kind: "embed".into(),
            params: serde_json::json!({"a": 1}),
            state: "running".into(),
            progress: 0.5,
            checkpoint: serde_json::json!({"done": 10}),
        };
        s.save_job(&j).unwrap();
        j.state = "completed".into();
        s.save_job(&j).unwrap();
        assert_eq!(s.job("j").unwrap().unwrap().state, "completed");
        assert_eq!(s.jobs().unwrap().len(), 1);
        assert!(s.latest_parity("a", "b").unwrap().is_none());
        let p = ParityRecord {
            space_id: "a".into(),
            encoder_space_id: "b".into(),
            sample: 5,
            mean_cos: 0.999,
            min_cos: 0.99,
            p05_cos: 0.995,
            passed: true,
        };
        s.record_parity(&p, &serde_json::json!({})).unwrap();
        assert!(s.latest_parity("a", "b").unwrap().unwrap().passed);

        assert!(s.latest_calibration("c", "a").unwrap().is_none());
        let mut c = CalibrationRecord {
            collection_id: "c".into(),
            space_id: "a".into(),
            method: "m1".into(),
            sample: 10,
            vectors: 99,
            curve: serde_json::json!({"points": [[0.1, 0.0]]}),
            metrics: serde_json::json!({"auc": 0.9}),
            created_at: None,
        };
        s.save_calibration(&c).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        c.method = "m2".into();
        s.save_calibration(&c).unwrap();
        let got = s.latest_calibration("c", "a").unwrap().unwrap();
        assert_eq!((got.method.as_str(), got.vectors), ("m2", 99));
        assert_eq!(got.metrics["auc"], 0.9);
        assert!(got.created_at.is_some());
    }

    #[test]
    fn vector_tables_verify_identity_hash() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("p.duckdb")).unwrap();
        let identity = crate::identity::test_identity();
        let rec = VectorTableRecord {
            collection_id: "c".into(),
            space_id: identity.space_id(),
            table_name: "t".into(),
            origin: "embedded".into(),
            identity: identity.clone(),
            ann: None,
        };
        s.register_vector_table(&rec).unwrap();
        s.register_vector_table(&rec).unwrap();
        let got = s.vector_tables(Some("c")).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].identity, identity);
        let mut bad = rec.clone();
        bad.space_id = "x".into();
        assert!(s.register_vector_table(&bad).is_err());
    }

    #[test]
    fn read_only_open_refuses_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ref.duckdb");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch("CREATE TABLE t (x INT); INSERT INTO t VALUES (1);")
                .unwrap();
        }
        let ro = open_read_only(&path).unwrap();
        assert!(ro.execute("INSERT INTO t VALUES (2)", []).is_err());
        let n: i64 = ro
            .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }
}
