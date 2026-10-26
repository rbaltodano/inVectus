# Clustering spike

Candidate: [`hdbscan` 0.12.0](https://docs.rs/hdbscan/0.12.0/hdbscan/), Rust's serial default feature. Input is the **same** Python-generated 10-D UMAP projection for both implementations. Cosine exact kNN k=30; UMAP min_dist=0, 200 epochs, seed 0; HDBSCAN min_cluster_size=30, min_samples=10. These smaller cluster sizes suit the 2,000-row spike; gapmap uses 80/10 for its full dataset.

Reference: read `gapmap/cluster.py`; reproduced its Python `umap-learn` / `hdbscan` path locally without invoking its write/cache functions. Compare both leaf (gapmap's actual selection) and EOM (closest mode to Rust). Raw results and ARI/best-Jaccard live beside this note.

## Measured results

Rust: **18.68 ms**, 11 clusters, **29.4% noise**. Python EOM: 19.51 ms, 11 clusters, 30.5% noise. Python leaf: 21.31 ms, 14 clusters, 47.5% noise. Shared 10-D Python UMAP took 1.127 s (kNN 0.093 s separately).

| Rust against reference | Adjusted Rand index | Mean best-Jaccard | Noise agreement |
|---|---:|---:|---:|
| Python EOM | **0.9332** | 0.9691 | 97.7% |
| Python leaf | **0.4916** | 0.6854 | 81.2% |

Good EOM agreement shows the algorithm works on this sample; the mismatch with leaf is material. No multi-seed stability or full-corpus scale claim is made. Raw results: `clustering-results.json`, `python-results.json`, `comparison-results.json`.

## Decision

**No-go as a drop-in production region backend.** It does cluster a sample in pure Rust, but its public builder lacks leaf selection and membership probabilities. Source inspection also finds a Prim MST with nested loops over samples (quadratic distance work); a 2k success cannot support the 100k/5-minute target. Missing leaf selection is a material difference from gapmap's 656 fine-topic pipeline, not a parameter to silently ignore.

Recommend a kNN-graph/Leiden spike as the next **P3 prerequisite**, with an audited implementation, disconnected-graph handling, multi-seed best-Jaccard stability and comparison to the leaf reference. Leiden has not been run or approved by this P0 spike. If Rust HDBSCAN is retained, it needs explicit selection/probability/scalability work; Python HDBSCAN remains the quality reference, not an unapproved runtime dependency.
