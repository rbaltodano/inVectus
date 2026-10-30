//! Offline chunking comparison on a local CSV (pmid, title, abstract).
//! cargo run --release -p invectus-cli --example chunking_eval -- papers.csv
//! Queries are verbatim abstract endings, a coverage diagnostic, not a relevance benchmark.
use anyhow::Result;
use invectus_core::{InputKind, TextEmbedder};
use invectus_embed::{EmbedOptions, ModelStore, ModelTokenizer, OnnxEmbedder, registry};
use invectus_ingest::import::build_item;
use invectus_ingest::{ChunkPolicy, ImportRequest, TokenChunking, preview};
use serde_json::json;

fn main() -> Result<()> {
    let source = std::env::args().nth(1).expect("local CSV path required");
    let store = ModelStore::new(ModelStore::default_root()?);
    let tokenizer = ModelTokenizer::load(&store, registry::DEFAULT_MODEL)?;
    let mut encoder = OnnxEmbedder::load(
        &store,
        registry::DEFAULT_MODEL,
        &EmbedOptions {
            threads: 4,
            batch_size: 32,
        },
    )?;
    let identity = encoder.identity().clone();
    let mut req: ImportRequest = serde_json::from_value(json!({
        "collection": "eval", "source": {"kind": "csv", "path": source},
        "mapping": {"id": "pmid", "title": "title", "text": ["abstract"],
            "date": null, "url": null, "role": {"kind": "constant", "role": "neutral"},
            "meta": {"kind": "none"}},
        "chunking": {"max_chars": 2000, "overlap_chars": 200}, "limit": null
    }))?;
    let rows = preview(&req, 5000)?.rows;
    // Same deterministic 100 long documents and queries for both policies.
    let queries: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.text.chars().count() > 2000)
        .take(100)
        .map(|(i, r)| {
            let words: Vec<_> = r.text.split_whitespace().collect();
            (i, words[words.len().saturating_sub(24)..].join(" "))
        })
        .collect();
    anyhow::ensure!(!queries.is_empty(), "no long abstracts to query");
    let query_texts: Vec<_> = queries.iter().map(|(_, q)| q.clone()).collect();
    let query_vectors = encoder.embed(&query_texts, InputKind::Query)?;
    let mut results = Vec::new();
    for (mode, policy) in [
        ("characters", ChunkPolicy::default()),
        (
            "tokens",
            ChunkPolicy::tokens(
                &tokenizer,
                TokenChunking::DEFAULT_MAX_TOKENS,
                TokenChunking::DEFAULT_OVERLAP_TOKENS,
            ),
        ),
    ] {
        let start = std::time::Instant::now();
        req.chunking = policy;
        let mut inputs = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let (parent, chunks) =
                build_item("eval", &req.chunking, Some(&tokenizer), row.clone())?;
            inputs.push((
                i,
                false,
                identity.format_document(parent.title.as_deref(), &parent.text),
            ));
            for chunk in chunks {
                inputs.push((
                    i,
                    true,
                    identity.format_document(chunk.title.as_deref(), &chunk.text),
                ));
            }
        }
        let mut vectors = Vec::new();
        let (mut parent_truncated, mut chunk_truncated) = (0, 0);
        for batch in inputs.chunks(32) {
            let texts: Vec<_> = batch.iter().map(|(_, _, text)| text.clone()).collect();
            for ((i, chunk, _), output) in batch
                .iter()
                .zip(encoder.embed(&texts, InputKind::Document)?)
            {
                if output.truncated {
                    if *chunk {
                        chunk_truncated += 1;
                    } else {
                        parent_truncated += 1;
                    }
                }
                vectors.push((*i, output.vector));
            }
        }
        let (mut at1, mut at10, mut reciprocal_rank) = (0, 0, 0.0);
        for ((own, _), query) in queries.iter().zip(&query_vectors) {
            let mut scores = vec![f32::NEG_INFINITY; rows.len()];
            for (i, vector) in &vectors {
                let cosine: f32 = vector.iter().zip(&query.vector).map(|(a, b)| a * b).sum();
                scores[*i] = scores[*i].max(cosine);
            }
            let mut order: Vec<_> = (0..rows.len()).collect();
            order.sort_by(|a, b| scores[*b].total_cmp(&scores[*a]).then(a.cmp(b)));
            let rank = order.iter().position(|i| i == own).unwrap() + 1;
            at1 += usize::from(rank == 1);
            at10 += usize::from(rank <= 10);
            reciprocal_rank += 1.0 / rank as f64;
        }
        let n = queries.len() as f64;
        results.push(
            json!({"mode": mode, "items": rows.len(), "chunks": inputs.len() - rows.len(),
            "parents_truncated": parent_truncated, "chunks_truncated": chunk_truncated,
            "seconds": start.elapsed().as_secs_f64(), "queries": queries.len(), "recall_at_1": at1 as f64 / n,
            "recall_at_10": at10 as f64 / n, "mrr": reciprocal_rank / n}),
        );
        eprintln!("{mode}: {}", results.last().unwrap());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "method": "exact cosine, parent max over item and chunks; last 24 words of first 100 abstracts over 2000 characters",
            "model": registry::DEFAULT_MODEL, "space_id": identity.space_id(),
        "threads": 4, "batch_size": 32, "results": results
        }))?
    );
    Ok(())
}
