# P0 spikes

These are isolated experiments, not P1 implementation. `spikes/` is a separate Cargo workspace so provisional ML dependencies do not enter the desktop binary. Lockfiles pin both workspaces. No model or sample content is committed.

| Spike | Verdict | Key result |
|---|---|---|
| [Embeddings](01-embeddings.md) | Go: direct ort; BGE CPU preset | BGE 71.95/s vs Nomic 26.69/s; fastembed provider-policy caveat |
| [LanceDB](02-lancedb.md) | Go with build caveats | ~0.8 ms p50; recall@10 0.88 → 1.00 at 4 → 16 probes |
| [Clustering](03-clustering.md) | No-go as a drop-in region backend | 18.68 ms; ARI 0.933 EOM / 0.492 leaf |
| [Rust UMAP](04-umap.md) | No-go in tested configuration | 0.114 s; trustworthiness 0.623–0.626 vs Python 0.866 |

[Spec recommendations before P1](spec-recommendations.md). All numbers below are sample measurements, not production guarantees.

Hardware: Apple M4 Pro, 12 cores (8 performance + 4 efficiency), 24 GB RAM, arm64 macOS 27.0.1; Rust 1.99.1, Xcode 27 beta / Apple clang 21. Node 25.9.0 locally, Node 24 in CI.

Sample: 2,000 rows from `embeddings_nomic`, first rows in Lance scan order (not random); normalized float32 768-D vectors, matched by PMID to title + abstract from DuckDB. Embedding speed uses the first 256 texts. Read-only DuckDB connection; Lance performs scan only. All extracted content and generated arrays live in ignored `.spike-data/`. This small, biased sample cannot establish corpus-wide accuracy or 100k/591k scalability.

Reproduce from repo root:

```sh
uv venv .spike-data/venv --python 3.12
uv pip install --python .spike-data/venv/bin/python -r scripts/spike-requirements.txt
.spike-data/venv/bin/python -B scripts/sample_gapmap.py
python3 scripts/download_spike_models.py
cargo build --release --manifest-path spikes/Cargo.toml --features embedding,lance
NUMBA_NUM_THREADS=8 OPENBLAS_NUM_THREADS=8 .spike-data/venv/bin/python -B scripts/reference_spikes.py
spikes/target/release/embedding nomic
spikes/target/release/embedding bge
spikes/target/release/lance
RAYON_NUM_THREADS=8 spikes/target/release/regions cluster
RAYON_NUM_THREADS=8 spikes/target/release/regions umap
.spike-data/venv/bin/python -B scripts/compare_spikes.py
```

LanceDB also requires `protoc` on PATH (tested with official Protocol Buffers 36.2; a local copy can be selected via `PROTOC=/absolute/path/to/protoc`). On this Mac it lives at `.spike-data/protoc/bin/protoc`, SHA-256 of the official arm64 archive: `9cd98a532c5c5e0c4161314de0225de27e4c8a323917b6ea7b1b714d3ae23466`.

Finish all builds before benchmarking to avoid compiler contention. Cold model load and Python Numba warmup are separated from inference/layout timings. Python reference uses the same parameters and implementation choices as `gapmap.cluster` (cosine kNN, UMAP 10-D/min_dist 0, HDBSCAN), but exact kNN on the small sample rather than NNDescent on the full corpus. No LLM downloads or inference.

For the serial measurement pass that writes metric JSON artifacts, use `python3 scripts/run_spikes.py` after compilation. It runs the commands above sequentially. Do not benchmark while building.
