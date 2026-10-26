# 2-D Rust UMAP spike

Candidate: [`umap-rs` 0.4.5](https://docs.rs/umap-rs/0.4.5/umap_rs/), a classical UMAP core taking precomputed kNN and initial coordinates. It avoids `fast-umap`'s additional neural/GPU framework for this display-only use case. The crate's [documented differences](https://github.com/wilsonzlin/umap-rs/blob/main/DIVERGENCES.md) matter when comparing with Python.

Shared 2,000 × 768 normalized sample, exact cosine kNN k=30, 2 output dimensions, min_dist 0.1, 200 epochs. Both receive the same PCA initialization, rescaled to ±10 with tiny seed-42 jitter. Rust uses 8 Rayon threads; its parallel SGD is nondeterministic, so two runs use identical inputs. Python uses seed 0; timing is an implementation/configuration comparison, not bitwise equivalence. Both exclude kNN time; Python also excludes separately measured Numba warmup. `comparison-results.json` reports cosine trustworthiness@15 against the original vectors, finite-coordinate checks and coordinate spread, not just elapsed time.

## Measured results

| Implementation | Layout time (excluding kNN/init) | Trustworthiness@15 |
|---|---:|---:|
| Python umap-learn | **0.692 s** | **0.8663** |
| Rust run 0 | **0.114 s** | **0.6232** |
| Rust run 1 | **0.114 s** | **0.6256** |

All coordinates were finite and non-collapsed, but Rust's neighbourhood preservation was substantially worse even with shared PCA initialization. A preliminary random-initialization run was worse still (~0.534); it is not included in the final timing artifacts. Python Numba warmup was 3.039 s separately; cosine kNN was 0.093 s. Raw results: `umap-results.json`, `python-results.json`, `comparison-results.json`.

## Decision

**No-go for `umap-rs` 0.4.5 in the tested configuration.** Its speed does not compensate for the measured neighbourhood loss. A future experiment may revisit initialization/optimizer settings or another Rust implementation, but P0 does not claim an acceptable Rust UMAP backend. Any future integration must wrap it with input validation (shapes, k, finite values, valid indices), panic containment in a worker, cancellation/job isolation and an explicit size limit. The API is fit-only: a changed collection needs refitting or another placement policy. Parallel optimization does not give deterministic layouts; persist the actual coordinates and run identity. This spike has not established 100k/500k performance, memory bounds, visual stability or the 60 fps rendering target.

**Fallback:** project region centroids (PCA or UMAP), then deterministic local PCA/placement inside each region, as §6.5 permits. This needs no Python runtime and bounds layout work. If a high-quality global layout remains necessary and Rust validation fails, an explicitly packaged optional `umap-learn` sidecar is the slower, more mature reference; benchmark packaging separately on all OSes. Neither display layout nor its trustworthiness is evidence that a semantic gap exists.
