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
