//! `invectus gapmap import`: copy papers and their Nomic vectors from a gapmap project
//! (`data/gapmap.duckdb`, `data/vectors/embeddings_nomic.lance`) without modifying it.
//!
//! The DuckDB file is opened with `access_mode = READ_ONLY`; the Lance table is only scanned.
//! Vectors keep gapmap's identity (sentence-transformers, `clustering: ` prefix, fp16 compute),
//! which is a different runtime from inVectus' ONNX encoder: the imported table is query-only and
//! searching it requires a passing `invectus parity` check.

use anyhow::{Context, Result, bail};
use arrow_array::{Array, FixedSizeListArray, Float32Array, Int64Array, RecordBatch, StringArray};
use invectus_core::item::{item_id, parse_date};
use invectus_core::store::{CollectionRecord, open_read_only};
use invectus_core::{EmbeddingIdentity, Item, Normalization, Pooling, Project, Role};
use invectus_index::{VectorRow, VectorStore};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub const LANCE_TABLE: &str = "embeddings_nomic";
const BATCH: usize = 2_048;

#[derive(Clone, Debug)]
pub struct GapmapPaths {
    pub duckdb: PathBuf,
    pub vectors: PathBuf,
    pub models_yaml: PathBuf,
}

impl GapmapPaths {
    /// Accept the gapmap project root (with `data/` and `config/`).
    pub fn from_root(root: &Path) -> Result<Self> {
        let p = GapmapPaths {
            duckdb: root.join("data/gapmap.duckdb"),
            vectors: root.join("data/vectors"),
            models_yaml: root.join("config/models.yaml"),
        };
        for f in [&p.duckdb, &p.vectors, &p.models_yaml] {
            if !f.exists() {
                bail!("{} not found; pass the gapmap project root", f.display());
            }
        }
        Ok(p)
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct GapmapReport {
    pub collection: String,
    pub vectors_read: usize,
    pub items_inserted: usize,
    pub items_updated: usize,
    pub items_unchanged: usize,
    pub vectors_added: usize,
    pub vectors_unchanged: usize,
    pub vectors_without_paper: usize,
    pub roles: BTreeMap<String, usize>,
    pub space_id: String,
    pub table: String,
    pub model_id: String,
}

/// The `nomic` entry of gapmap's `config/models.yaml` (a small subset of YAML is parsed).
#[derive(Clone, Debug, PartialEq)]
pub struct GapmapModel {
    pub repo: String,
    pub revision: String,
    pub prefix: String,
    pub max_length: usize,
}

pub fn parse_models_yaml(yaml: &str, name: &str) -> Result<GapmapModel> {
    let mut in_block = false;
    let mut indent = 0;
    let mut fields = HashMap::new();
    for line in yaml.lines() {
        let trimmed = line.trim_start();
        let ind = line.len() - trimmed.len();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if in_block {
            if ind <= indent {
                break;
            }
            if let Some((k, v)) = trimmed.split_once(':') {
                let v = v.trim();
                let v = v
                    .strip_prefix('\'')
                    .and_then(|x| x.strip_suffix('\''))
                    .or_else(|| v.strip_prefix('"').and_then(|x| x.strip_suffix('"')))
                    .unwrap_or(v);
                fields.insert(k.trim().to_string(), v.to_string());
            }
        } else if trimmed == format!("{name}:") {
            in_block = true;
            indent = ind;
        }
    }
    let get = |k: &str| {
        fields
            .get(k)
            .cloned()
            .with_context(|| format!("models.yaml: {name}.{k} missing"))
    };
    Ok(GapmapModel {
        repo: get("repo")?,
        revision: get("revision")?,
        prefix: fields.get("prefix").cloned().unwrap_or_default(),
        max_length: get("max_length")?.parse()?,
    })
}

/// Identity of gapmap's stored vectors (see `gapmap/embed.py`: sentence-transformers with
/// `normalize_embeddings=True`, `half()` on MPS, `f"{prefix}{title}. {abstract}"`).
pub fn gapmap_identity(m: &GapmapModel, dims: usize, tokenizer_sha256: &str) -> EmbeddingIdentity {
    EmbeddingIdentity {
        model_key: "nomic-embed-text-v1.5".into(),
        repo: m.repo.clone(),
        revision: m.revision.clone(),
        // The safetensors checkpoint gapmap loaded was not hashed by gapmap; record its origin.
        weights_sha256: format!("unrecorded:sentence-transformers@{}", m.revision),
        tokenizer_sha256: tokenizer_sha256.into(),
        document_prefix: m.prefix.clone(),
        // Symmetric use: queries get the same prefix as the documents they are compared with.
        query_prefix: m.prefix.clone(),
        text_template: "{title}. {text}".into(),
        pooling: Pooling::Mean,
        normalization: Normalization::L2,
        max_tokens: m.max_length,
        truncation: "head".into(),
        dimensions: dims,
        compute_precision: "fp16".into(),
        storage_precision: "f32".into(),
        runtime: "sentence-transformers (gapmap, MPS fp16)".into(),
    }
}

struct Paper {
    title: String,
    abstract_: String,
    date: Option<String>,
    role: Role,
    meta: serde_json::Value,
}

fn read_papers(conn: &duckdb::Connection, pmids: &[i64]) -> Result<HashMap<i64, Paper>> {
    let list = pmids
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut st = conn.prepare(&format!(
        "SELECT pmid, coalesce(title, ''), coalesce(abstract, ''), CAST(pub_date AS VARCHAR), year,
                coalesce(is_synthesis, false), coalesce(is_primary, false),
                json_object('doi', doi, 'journal', journal, 'year', year,
                            'pub_types', pub_types, 'mesh_major', mesh_major,
                            'is_synthesis', is_synthesis, 'is_primary', is_primary,
                            'is_narrative_review', is_narrative_review,
                            'is_case_report', is_case_report)::VARCHAR
         FROM papers WHERE pmid IN ({list})"
    ))?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<i32>>(4)?,
            r.get::<_, bool>(5)?,
            r.get::<_, bool>(6)?,
            r.get::<_, String>(7)?,
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (pmid, title, abstract_, pub_date, year, synthesis, primary, meta) = row?;
        let date = pub_date
            .as_deref()
            .and_then(parse_date)
            .or_else(|| year.and_then(|y| parse_date(&y.to_string())));
        // gapmap roles: synthesis papers cover, primary studies are demand (SPEC §5.1).
        let role = if synthesis {
            Role::Coverage
        } else if primary {
            Role::Demand
        } else {
            Role::Neutral
        };
        out.insert(
            pmid,
            Paper {
                title,
                abstract_,
                date,
                role,
                meta: serde_json::from_str(&meta)?,
            },
        );
    }
    Ok(out)
}

pub fn import_gapmap(
    project: &mut Project,
    vectors: &VectorStore,
    paths: &GapmapPaths,
    collection: &str,
    limit: Option<usize>,
    tokenizer_sha256: &str,
) -> Result<GapmapReport> {
    invectus_core::item::validate_collection_name(collection)?;
    let yaml = std::fs::read_to_string(&paths.models_yaml)?;
    let model = parse_models_yaml(&yaml, "nomic")?;
    let source = VectorStore::open_dir(&paths.vectors)?;
    if !source.table_names()?.iter().any(|t| t == LANCE_TABLE) {
        bail!("{} has no {LANCE_TABLE} table", paths.vectors.display());
    }
    let papers_db = open_read_only(&paths.duckdb)?;
    let expected_model_id = format!("nomic@{}", &model.revision[..12]);

    let mut report = GapmapReport {
        collection: collection.into(),
        model_id: expected_model_id.clone(),
        ..Default::default()
    };
    let mut pending: Vec<(i64, Vec<f32>)> = Vec::new();
    let mut ctx: Option<(
        EmbeddingIdentity,
        invectus_core::store::VectorTableRecord,
        HashMap<i64, String>,
    )> = None;

    let flush = |pending: &mut Vec<(i64, Vec<f32>)>,
                 ctx: &mut Option<_>,
                 report: &mut GapmapReport,
                 project: &mut Project|
     -> Result<()> {
        if pending.is_empty() {
            return Ok(());
        }
        let dims = pending[0].1.len();
        if ctx.is_none() {
            let identity = gapmap_identity(&model, dims, tokenizer_sha256);
            project.store.upsert_collection(&CollectionRecord {
                collection_id: collection.into(),
                source: serde_json::json!({"kind": "gapmap", "duckdb": paths.duckdb,
                                           "vectors": paths.vectors, "table": LANCE_TABLE}),
                mapping: serde_json::json!({"id": "pmid", "title": "title", "text": ["abstract"],
                    "date": "pub_date (else year)", "url": "https://pubmed.ncbi.nlm.nih.gov/{pmid}/",
                    "role": "is_synthesis → coverage; is_primary → demand; else neutral"}),
                chunking: serde_json::json!({"max_chars": 0, "overlap_chars": 0}),
            })?;
            let table = vectors.ensure_table(project, collection, &identity, "imported")?;
            let stored = vectors.stored_hashes(&table)?;
            report.space_id = table.space_id.clone();
            report.table = table.table_name.clone();
            *ctx = Some((identity, table, stored));
        }
        let (identity, table, stored) = ctx.as_ref().unwrap();
        let pmids: Vec<i64> = pending.iter().map(|p| p.0).collect();
        let papers = read_papers(&papers_db, &pmids)?;
        let mut items = Vec::new();
        let mut rows = Vec::new();
        for (pmid, vector) in pending.drain(..) {
            let Some(p) = papers.get(&pmid) else {
                report.vectors_without_paper += 1;
                continue;
            };
            let source_id = pmid.to_string();
            let id = item_id(collection, &source_id, None);
            let url = format!("https://pubmed.ncbi.nlm.nih.gov/{pmid}/");
            let title = Some(p.title.clone()).filter(|t| !t.is_empty());
            *report.roles.entry(p.role.as_str().into()).or_insert(0) += 1;
            items.push((
                Item {
                    item_id: id,
                    collection_id: collection.into(),
                    source_id,
                    input_hash: Item::content_hash(
                        title.as_deref(),
                        &p.abstract_,
                        Some(&url),
                        p.date.as_deref(),
                        p.role,
                        &p.meta,
                    ),
                    title: title.clone(),
                    text: p.abstract_.clone(),
                    url: Some(url),
                    date: p.date.clone(),
                    role: p.role,
                    meta: p.meta.clone(),
                    parent_id: None,
                    chunk_index: None,
                },
                vec![],
            ));
            let hash =
                identity.input_hash(&identity.format_document(title.as_deref(), &p.abstract_));
            if stored.get(&id) == Some(&hash) {
                report.vectors_unchanged += 1;
            } else {
                if stored.contains_key(&id) {
                    vectors.delete_ids(table, &[id])?;
                }
                rows.push(VectorRow {
                    item_id: id,
                    parent_id: None,
                    input_hash: hash,
                    vector,
                });
            }
        }
        let c = project.store.write_items(&items)?;
        report.items_inserted += c.inserted;
        report.items_updated += c.updated;
        report.items_unchanged += c.unchanged;
        vectors.add(table, &rows)?;
        report.vectors_added += rows.len();
        Ok(())
    };

    let mut err: Option<anyhow::Error> = None;
    source
        .scan(
            LANCE_TABLE,
            &["pmid", "model_id", "vector"],
            limit,
            &mut |b: RecordBatch| {
                let pmids = b
                    .column_by_name("pmid")
                    .and_then(|c| c.as_any().downcast_ref::<Int64Array>())
                    .context("pmid column")?;
                let models = b
                    .column_by_name("model_id")
                    .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                    .context("model_id column")?;
                let vecs = b
                    .column_by_name("vector")
                    .and_then(|c| c.as_any().downcast_ref::<FixedSizeListArray>())
                    .context("vector column")?;
                for i in 0..b.num_rows() {
                    if models.value(i) != expected_model_id {
                        bail!(
                            "vector for pmid {} has model_id {:?}, expected {expected_model_id:?}",
                            pmids.value(i),
                            models.value(i)
                        );
                    }
                    let v = vecs.value(i);
                    let v = v
                        .as_any()
                        .downcast_ref::<Float32Array>()
                        .context("vector type")?;
                    pending.push((pmids.value(i), v.values().to_vec()));
                    report.vectors_read += 1;
                    if pending.len() >= BATCH
                        && let Err(e) = flush(&mut pending, &mut ctx, &mut report, project)
                    {
                        err = Some(e);
                        bail!("import failed");
                    }
                }
                Ok(())
            },
        )
        .map_err(|e| err.take().unwrap_or(e))?;
    flush(&mut pending, &mut ctx, &mut report, project)?;
    if report.vectors_read == 0 {
        bail!("{LANCE_TABLE} is empty");
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_nomic_block_of_models_yaml() {
        let yaml = "embedding:\n  selected: nomic\ncandidates:\n  specter2:\n    repo: a/b\n    revision: x\n    max_length: 512\n  nomic:\n    kind: st\n    repo: nomic-ai/nomic-embed-text-v1.5\n    prefix: 'clustering: '\n    revision: e9b6\n    max_length: 512\n  bge_base:\n    repo: c/d\n";
        let m = parse_models_yaml(yaml, "nomic").unwrap();
        assert_eq!(m.repo, "nomic-ai/nomic-embed-text-v1.5");
        assert_eq!(m.prefix, "clustering: ");
        assert_eq!(m.revision, "e9b6");
        assert_eq!(m.max_length, 512);
        assert!(parse_models_yaml(yaml, "missing").is_err());
    }
}
