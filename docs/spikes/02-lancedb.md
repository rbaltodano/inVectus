# LanceDB Rust spike

Candidate: [`lancedb` 0.40.0](https://docs.rs/lancedb/0.40.0/lancedb/) (Lance 13 / Arrow 58), embedded local storage, `remote` enabled solely to work around an upstream feature-gating build bug; all operations use a local filesystem path. Create a new table from the copied 2,000 × 768 float32 vectors, build a cosine IVF_FLAT index with 16 partitions, then run 50 queries at each of 4 and 16 probes (50 candidates each) and exactly re-score candidates with stored-vector dot products. Unit normalization makes dot product equal cosine similarity.

Ground truth: exhaustive dot products over all 2,000 vectors, recall@10 averaged across the same 50 queries. Queries are sampled corpus items, so include self matches; this is an index/recall smoke benchmark, not absence-detection quality. Saved `lance-query-plan.txt` must contain the ANN node and the program asserts a real index was created. Timings separate table creation, index construction, retrieval p50/p95, and exact candidate re-score p50. Query embedding and application IPC are excluded.

## Measured results

Table creation: **9.44 ms**. IVF_FLAT build: **43.19 ms**. Saved plan confirms `ANNSubIndex` and `ANNIvfPartition`.

| Probes / 16 partitions | Retrieval p50 | Retrieval p95 | Exact re-score p50 (50 candidates) | Recall@10 |
|---|---:|---:|---:|---:|
| 4 | 0.810 ms | 1.083 ms | 0.020 ms | **0.88** |
| 16 | 0.796 ms | 0.927 ms | 0.020 ms | **1.00** |

The tiny dataset and warmed caches make the latency difference insignificant; 16 probes visits every partition and sacrifices ANN pruning. Crucially, exact re-score cannot recover candidates missed by retrieval. P1 absence checks need a high-recall/exhaustive verification route rather than assuming re-score fixes recall. Raw results: `lance-results.json`.

## Decision

**Go for the P1 index implementation, subject to platform CI.** Use explicit metrics, keep exact re-score and measure recall; ANN distances alone must not justify absence. IVF_FLAT suits the tiny spike without PQ training constraints; production index type/partition/probe tuning stays open. The spike creates a fresh local store and removes only that temporary store; it never opens the reference vectors for writing.

Build caveat: `protoc` is required by transitive Lance crates. Installed official 36.2 locally inside `.spike-data/` with verified archive checksum; CI installs the same version. The compile graph is large (DataFusion/Arrow/Lance); keep it isolated until P1 and budget CI/disk accordingly. The UI scaffold currently does not link LanceDB or download models.

Upstream issue found locally: 0.40.0 with default features fails E0599 in `src/job.rs:102,112`, referencing `Error::Http` although that variant is gated on `remote`. The spike enables `remote` to compile the published crate; no hosted connection, API key or remote operation is used. Before P1, prefer a fixed upstream release or a narrowly reviewed patch with a local-only feature graph. Enabling cloud support in a dependency is not permission to make network calls; the application's offline gate remains required.

macOS release linking emitted `__eh_frame section too large (max 16MB)` and warned that exception-handling performance might be affected. The executable linked and runs; treat this as a packaging/build-size concern to investigate before shipping, not a silent clean-build claim.
