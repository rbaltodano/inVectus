# Progress ledger

## 2026-10-26
- Created the project and wrote `SPEC.md` v0.1. Stack: Tauri 2 + Rust core + React/TypeScript.
- Machine state: Node and npm installed; **Rust not installed** (`rustup` is needed for P0).

## Original P0 checklist (superseded by checkpoints below)
1. Install Rust (`rustup`), the Tauri prerequisites and pnpm; scaffold the Tauri + React app and the Rust workspace.
2. Spikes, each with a go / no-go note: `ort` embeddings (nomic vs bge-small speed), `lancedb` Rust,
   clustering crate vs the gapmap Python reference, Rust UMAP.
3. CI builds on macOS, Windows and Linux.

### P0 setup checkpoint
- Read SPEC.md in full, then README.md and this ledger. No repository-local AGENTS.md or CLAUDE.md exists.
- Installed rustup + Rust 1.99.1 (minimal, rustfmt, clippy), per-user. Existing Xcode-beta includes clang and macOS SDK; no sudo installation needed.
- Scaffolded Tauri 2 + React **18** + TypeScript + Vite, twelve `invectus-*` workspace crates and `invectus` CLI. Shell invokes the shared core version.
- npm install/build passed. Chose npm (already installed); pnpm is unnecessary and was only mentioned in the old ledger, not required by the spec.
- Added three-OS CI workflow: frontend, workspace tests, formatting, clippy, CLI smoke, native executable build. Windows/Linux execution is pending a future CI run; no push authorized or attempted.
- Reference environment has iCloud `dataless` Python bytecode; reads stalled. Creating a separate spike Python environment; reference project/data stay read-only.
- Available disk started at ~12 GiB; debug symbols and incremental Rust artifacts disabled to limit build storage.

### P0 build validation
- `cargo test --workspace --locked`: passed, one test in each of 12 core/CLI crates plus the shell test (13 total).
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `npm run build`, `cargo run --locked --bin invectus`, native `tauri build --no-bundle --ci`: passed.
- Built a macOS `.app` bundle (9.70 MiB); launched and checked native accessibility text and screenshot. UI displays “Rust core 0.1.0 connected.”
- LanceDB needed `protoc`: downloaded official Protocol Buffers 36.2 arm64 archive into ignored `.spike-data/`, verified SHA-256 against release metadata. No sudo/system install.
- Two pinned ONNX embedding weights downloaded, total 680,403,765 bytes (~680.4 MB) plus tokenizer/config files; manifest in `docs/spikes/model-manifest.json`. No LLM downloads.
- Copied 2,000 vectors + matching texts from the reference using a read-only DuckDB connection and a limited Lance scan. Sample hashes are recorded. No full-corpus benchmark runs or reference writes.

### P0 completed — measurements and decisions
- Serial release-mode measurements on this M4 Pro used 2,000 copied 768-D vectors; embeddings used 256 matched title+abstract texts, batch 32, max 512 tokens, 8 CPU threads, 3 timed passes after warmup. First Lance scan rows are a biased sample, not a full-corpus benchmark.
- **Embeddings:** direct `ort` 2.0.0-rc.15 CPU median Nomic v1.5 **26.69 items/s** (26.47–27.31), BGE-small v1.5 **71.95 items/s** (71.18–72.33). Go for ort; provisional BGE CPU preset/default recommendation, Nomic no-go for ≥60/s in this configuration. Quality/default model decision still needs retrieval evaluation; no existing vectors were mixed across models.
- **fastembed caveat:** 7.5.0 compiled, but empty-provider and explicit-CPU preflights still selected CoreML through ort's default `MaxEfficiency` device policy. Both were stopped; no timings accepted. Measured implementation uses direct ort `PreferCPU` because fastembed does not expose that setting. CPU use/no CoreML model handles were checked. P1 must explicitly control and record the provider.
- **LanceDB 0.40.0:** table creation **9.44 ms**, IVF_FLAT build **43.19 ms**; 50 queries over 2k vectors, 50 candidates. At 4/16 probes: retrieval p50 **0.810/0.796 ms**, p95 **1.083/0.927 ms**, recall@10 **0.88/1.00**. Exact re-score p50 **~0.020 ms**. Go with caveats: default-feature compilation fails in upstream job.rs (`Error::Http` gating); `remote` feature compiled successfully as a workaround, with local filesystem connections only. Apple linker warned about large `__eh_frame`. Prefer a fixed local-only dependency before P1. Exact re-score cannot recover omitted neighbours.
- **Rust hdbscan 0.12.0:** **18.68 ms**, 11 clusters, 29.4% noise. Python EOM: 19.51 ms, 11 clusters, 30.5% noise; leaf reference: 21.31 ms, 14 clusters, 47.5% noise. Rust vs EOM ARI **0.9332**, mean best-Jaccard **0.9691**; vs leaf ARI **0.4916**, Jaccard **0.6854**. No-go as a drop-in region backend: no leaf mode/probabilities and quadratic Prim MST work. Recommend a future Leiden/kNN evaluation before P3, not an untested production selection.
- **Rust umap-rs 0.4.5:** same cosine kNN/PCA initialization, 200 epochs; **0.114 s** in each of two runs vs Python **0.692 s**, but trustworthiness@15 only **0.6232/0.6256** vs **0.8663**. No-go in tested configuration. Recommend centroid projection plus local deterministic PCA as fallback; optional packaged Python UMAP only if a global layout is required. Map coordinates never decide gaps.
- All four go/no-go notes, raw metric JSON, sample/model hashes, query plan, Python dependency pins and reproducible Rust/Python harnesses are in `docs/spikes/`, `spikes/` and `scripts/`. Stopped/preliminary runs are excluded from final metric artifacts.
- CI includes native app and ML spike compilation matrices for macOS/Windows/Linux, without model/sample downloads. **Not run remotely:** no push was made, so Windows/Linux build/runtime status remains unverified.
- Low disk headroom was aggravated by stopped CoreML compilation/swap. Removed only this task's generated compiler intermediates and PID-specific temporary model files; preserved compiled app/spike executables, samples and downloaded models. Rebuilding will regenerate compiler artifacts.
- SPEC.md was not edited. `docs/spikes/spec-recommendations.md` covers performance workload definitions, query-level calibration, bridge gates, model identity/offline control, clustering/layout constraints and cross-platform validation. Licensing remains undecided; packages are marked non-publishable rather than choosing a license.

## Next — P1 (not started)
1. Resolve the LanceDB local-only feature bug and ort provider policy in the selected dependency configuration; require the three-OS CI jobs to pass when a push is eventually authorized.
2. Implement CLI import/embed/search with project model identity, offline loading, resumable work and high-recall/exact verification support; evaluate BGE vs Nomic retrieval quality before finalizing the default.
3. Keep Rust region/layout backend selection open until the documented quality/scalability concerns are resolved. Stop at P0 in this session.
