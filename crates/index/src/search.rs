//! Positive nearest-neighbour search: embed the query in the table's space, retrieve, re-score
//! exactly, and return parent items with evidence. This supports verification; it is **not** an
//! absence verdict (calibrated verdicts are P2). Results only describe this collection.

use crate::vectors::{Scored, VectorStore};
use anyhow::{Result, bail};
use invectus_core::store::VectorTableRecord;
use invectus_core::{InputKind, Item, Project, Role, TextEmbedder};
use serde::Serialize;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// ANN when an index exists, otherwise exhaustive.
    Auto,
    /// IVF index with `nprobes` partitions probed (default: a quarter, at least 8).
    Ann { nprobes: Option<usize> },
    /// Exact similarity against every stored vector.
    Exhaustive,
}

#[derive(Clone, Debug)]
pub struct SearchOptions {
    pub k: usize,
    pub role: Option<Role>,
    pub mode: SearchMode,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            k: 10,
            role: None,
            mode: SearchMode::Auto,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchHit {
    pub rank: usize,
    /// Exact cosine similarity of the best-matching vector (item or one of its chunks).
    pub similarity: f32,
    /// Chunk index when the best match was a chunk of the item.
    pub via_chunk: Option<i32>,
    pub item: Item,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchResult {
    pub query: String,
    pub collection: String,
    pub space_id: String,
    pub model_key: String,
    /// How the query encoder was admitted to this space.
    pub compatibility: String,
    /// `ann (nprobes p/P, approximate)` or `exhaustive (exact)`.
    pub mode: String,
    /// True when every stored vector was compared (no index pruning).
    pub exhaustive: bool,
    pub vectors_compared: usize,
    pub hits: Vec<SearchHit>,
    pub notes: Vec<String>,
    pub embed_ms: f64,
    pub retrieve_ms: f64,
    /// The embedded query (for region lookups); not serialized.
    #[serde(skip)]
    pub query_vector: Vec<f32>,
}

/// Refuse to compare a query from `encoder` with `table` unless both share a space, or a parity
/// check of this encoder against the table's stored vectors passed.
pub fn check_compatible(
    project: &Project,
    table: &VectorTableRecord,
    encoder: &dyn TextEmbedder,
) -> Result<String> {
    let enc = encoder.identity();
    let enc_space = enc.space_id();
    if enc_space == table.space_id {
        return Ok("same space".into());
    }
    if !table.identity.parity_compatible(enc) {
        bail!(
            "query model {} (space {}) is not in the vector space of {} (space {}, model {}); \
             vectors from different models or text policies are never compared",
            enc.model_key,
            &enc_space[..12],
            table.table_name,
            &table.space_id[..12],
            table.identity.model_key
        );
    }
    match project.store.latest_parity(&table.space_id, &enc_space)? {
        Some(p) if p.passed => Ok(format!(
            "parity-verified local encoder (n={}, mean cos {:.4}, min {:.4})",
            p.sample, p.mean_cos, p.min_cos
        )),
        Some(p) => bail!(
            "the local encoder failed its parity check against {} (mean cos {:.4}, min {:.4})",
            table.table_name,
            p.mean_cos,
            p.min_cos
        ),
        None => bail!(
            "{} holds vectors from a different runtime ({}); run `invectus parity` to check the \
             local encoder reproduces them before searching",
            table.table_name,
            table.identity.runtime
        ),
    }
}

pub fn search(
    project: &Project,
    vectors: &VectorStore,
    table: &VectorTableRecord,
    encoder: &mut dyn TextEmbedder,
    query: &str,
    opts: &SearchOptions,
) -> Result<SearchResult> {
    let compatibility = check_compatible(project, table, encoder)?;
    vectors.check(table)?;
    let query_text = query.trim();
    if query_text.is_empty() {
        bail!("empty query");
    }
    let t0 = Instant::now();
    let q = encoder
        .embed(&[query_text.to_string()], InputKind::Query)?
        .remove(0)
        .vector;
    let embed_ms = t0.elapsed().as_secs_f64() * 1e3;
    let k = opts.k.max(1);
    let total = vectors.count(table)?;
    let mut notes = Vec::new();

    let t1 = Instant::now();
    let partitions = table
        .ann
        .as_ref()
        .and_then(|a| a["partitions"].as_u64())
        .map(|p| p as usize);
    let use_ann = match opts.mode {
        SearchMode::Exhaustive => false,
        SearchMode::Ann { .. } => {
            if partitions.is_none() || !vectors.has_ann(table)? {
                bail!(
                    "{} has no ANN index; use exhaustive search",
                    table.table_name
                );
            }
            true
        }
        SearchMode::Auto => partitions.is_some() && vectors.has_ann(table)?,
    };
    let allowed = opts
        .role
        .map(|r| project.store.item_ids_with_role(&table.collection_id, r))
        .transpose()?;

    let mut mode;
    let mut exhaustive = false;
    let mut compared;
    let mut parents = Vec::new();
    if use_ann {
        let p = partitions.unwrap_or(8);
        let nprobes = match opts.mode {
            SearchMode::Ann { nprobes: Some(n) } => n.clamp(1, p),
            _ => (p / 4).max(8).min(p),
        };
        // Over-fetch: chunks collapse into parents and the role filter removes candidates.
        let candidates = (k * 10).max(100);
        let mut scored = vectors.ann(table, &q, candidates, nprobes)?;
        if let Some(a) = &allowed {
            scored.retain(|s| a.contains(&s.item_id));
        }
        compared = scored.len();
        mode = format!("ann (IVF_FLAT, nprobes {nprobes}/{p}, approximate; exact re-score)");
        parents = collapse(&scored, k);
        if parents.len() < k && parents.len() < total {
            notes.push(format!(
                "ANN returned {} matching item(s) after filtering; fell back to exhaustive search",
                parents.len()
            ));
        }
    } else {
        mode = String::new();
        compared = 0;
    }
    if !use_ann || (parents.len() < k && parents.len() < total) {
        // Enough raw vectors that `k` distinct parents survive chunk collapsing.
        let (scored, scanned) = vectors.exhaustive(table, &q, total.max(1), allowed.as_ref())?;
        parents = collapse(&scored, k);
        compared = scanned;
        exhaustive = true;
        mode = "exhaustive (exact cosine over all stored vectors)".into();
    }
    let retrieve_ms = t1.elapsed().as_secs_f64() * 1e3;

    let ids: Vec<i64> = parents.iter().map(|p| p.0).collect();
    let lookup: Vec<i64> = ids
        .iter()
        .copied()
        .chain(parents.iter().filter_map(|p| p.2))
        .collect();
    let items = project.store.items_by_ids(&lookup)?;
    let hits = parents
        .into_iter()
        .enumerate()
        .filter_map(|(i, (id, sim, chunk))| {
            let item = items.get(&id)?.clone();
            Some(SearchHit {
                rank: i + 1,
                similarity: sim,
                via_chunk: chunk
                    .and_then(|c| items.get(&c))
                    .and_then(|c| c.chunk_index),
                item,
            })
        })
        .collect::<Vec<_>>();
    if hits.len() < ids.len() {
        notes.push("some vectors have no matching item; re-run `invectus embed`".into());
    }
    if !exhaustive {
        notes.push(
            "approximate retrieval can miss near neighbours; use exhaustive mode before treating \
             a missing match as meaningful"
                .into(),
        );
    }
    notes.push(
        "nearest items in this collection only; similarity is not a calibrated existence verdict"
            .into(),
    );
    Ok(SearchResult {
        query: query_text.into(),
        collection: table.collection_id.clone(),
        space_id: table.space_id.clone(),
        model_key: table.identity.model_key.clone(),
        compatibility,
        mode,
        exhaustive,
        vectors_compared: compared,
        hits,
        notes,
        embed_ms,
        retrieve_ms,
        query_vector: q,
    })
}

/// Collapse chunk hits into their parents (best similarity wins) and keep the top `k`.
/// Returns `(parent id, similarity, chunk item id when the best match was a chunk)`.
pub fn collapse(scored: &[Scored], k: usize) -> Vec<(i64, f32, Option<i64>)> {
    let mut best: HashMap<i64, (f32, Option<i64>)> = HashMap::new();
    for s in scored {
        let parent = s.parent_id.unwrap_or(s.item_id);
        let via = s.parent_id.map(|_| s.item_id);
        let e = best.entry(parent).or_insert((f32::NEG_INFINITY, None));
        if s.similarity > e.0 {
            *e = (s.similarity, via);
        }
    }
    let mut v: Vec<_> = best
        .into_iter()
        .map(|(id, (sim, via))| (id, sim, via))
        .collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(k);
    v
}
