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

## 2026-10-26 — P1 engine CLI

Scope: SPEC §11 P1 only (`invectus import / embed / search` on CSV, Parquet and folder input;
gapmap import; tests) plus a desktop shell over the same crates. P2 (calibrated verdicts) not
started. Nothing pushed.

### Changes
- **Dependencies.** LanceDB 0.40.0 vendored in `third_party/` with a local-only patch: two
  `Error::Http` branches in `job.rs` are cfg-gated, and the resulting dead code is allowed. It is
  built with no features, so `remote` is gone; the patch and the crate checksum are in
  `third_party/README.md`. DuckDB `=1.20000.0` (DuckDB 2.0, bundled, parquet+json; extension
  auto-install/auto-load disabled on every connection). ort `=2.0.0-rc.15` with `api-31` and
  `AutoDevicePolicy::PreferCPU`. tokenizers without `http`. The workspace now needs `protoc` (CI
  updated).
- **core:** `EmbeddingIdentity` and the space id (SHA-256 over the model commit, weights SHA,
  tokenizer SHA, document/query prefixes, text template, pooling, normalization, token cap,
  truncation, dims, compute/storage precision and runtime). Also the project layout
  (`project.json` with network off by default, `project.duckdb`, `vectors/`, `cache/`, `exports/`),
  the cloud-sync folder warning, the DuckDB store (`items`, `collections`, `embedding_spaces`,
  `vector_tables`, `parity_checks`, `jobs`), a read-only open helper, and the offline gate.
- **ingest:** CSV/TSV/Parquet via DuckDB (all values read as text, so ids keep leading zeros);
  folder reader (txt/md/markdown/rst/text; skips hidden files, symlinks, non-UTF-8 and files over
  5 MB). Schema mapping (id/title/text/date/url/meta), roles as a constant or column rules (list
  values `[a, b]` or `a; b`), and 20-row preview. Character chunking (2,000/200), then idempotent
  import: hash-derived item ids, content hashes, duplicates/empty rows reported, rows missing from
  the source counted and kept. Read-only gapmap import.
- **embed:** pinned registry (BGE-small, Nomic v1.5; commits + SHA-256), and checksum-verified
  install (from a local folder, hard-linked) or explicit download (network, refused offline). The
  ONNX CPU encoder applies mean/CLS pooling and L2 / layer-norm+L2, reports truncation, and keeps
  length-sorted batches in input order.
- **index:** one LanceDB table per collection × space; the identity is stored in the Arrow schema
  metadata and checked on open. Embed job: resumable and idempotent (input hash = space +
  model input). It deletes stale and orphaned vectors, flushes and checkpoints every 256 vectors,
  supports cancel, and builds an IVF_FLAT cosine index at ≥ 50k vectors. Search: ANN candidates,
  then exact cosine re-score against the stored vectors, or exhaustive exact scan (the
  verification route). It collapses chunks to parents ("via chunk n"), filters by role (falling
  back to exhaustive when ANN leaves too few results), and refuses cross-space queries. Parity
  check: re-embed a sample and compare with the stored vectors; this is required before querying
  imported foreign-runtime vectors.
- **cli:** `init`, `status`, `import` (incl. `--preview`), `embed` (Ctrl-C keeps progress), `search`
  (`--exhaustive`, `--nprobes`, `--role`, `--space`, `--json`), `parity`,
  `models list|install|download|verify`, `gapmap import`, and global `--offline`.
- **desktop:** Tauri commands over the same crates (open/create project, import preview/run, embed
  job with progress events and cancel, search). Engine work runs on OS threads; each project is
  used by one process. The React page is a functional, unstyled flow.
- **Naming (owner direction, mid-session):** "unSearch" is retired because the name is taken. SPEC.md,
  README and UI text now say "inVectus search". The crate is still `invectus-unsearch`, pending a
  rename decision.
- Scripts: `scripts/sample_p1.py` (5,000-paper hash-ordered sample, read-only) and
  `scripts/p1_search_eval.py` (ANN vs exhaustive).

### Checks (arm64 macOS only)
- `cargo test --workspace --locked`: **50 passed**, including real-BGE tests and CLI end-to-end
  tests. Model tests skip, rather than fail, when the pinned files are absent.
  `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings`
  are clean. `npm run build` passes. The release CLI build took 7 m 52 s.
- Reference safety: a SHA-256 of (path, size, mtime) for `gapmap.duckdb` and all 232 files under
  `vectors/` was identical before and after every reference read (`3375279f…`). Copied:
  5,000 papers plus 50 held-out titles (5,050 ≤ 10k), all in ignored `.spike-data/p1/`.

### Measurements (5,000-paper dev sample; M4 Pro, 8 threads, batch 32; not a 100k validation)
- Import CSV: 5,000 rows → 5,000 items, 470 chunked into 947 chunks, **1.5 s**. Re-import: all
  unchanged, 0.1 s. The Parquet copy of the same data: all unchanged (CSV/Parquet parity). After
  correcting the role rules, 3,582 items were updated.
- **BGE embed:** 5,947 inputs (items + chunks) at **50.7 items/s** end to end (Lance writes and
  checkpoints included). 584 inputs were truncated at 512 tokens; at most 470 of them are parents
  of chunked items, which is by design, so ≥ 114 chunks exceeded 512 tokens despite the
  2,000-character limit, because medical text tokenizes densely. CPU ~634%, with no
  CoreML/mlmodel files open. Re-run: 0 embedded.
- **Interrupt/resume:** SIGINT during the Nomic embed kept the 1,536 flushed vectors (job
  `cancelled`). Re-running embedded the remaining 4,411, for 5,947 in total. Nomic throughput
  (42.1/s first part, 16.6/s remainder) overlapped a parity run and length-sorted ordering, so it
  is **not** a clean benchmark.
- **Search, BGE, 50 held-out titles:** ANN at default probes (19/77) gave recall@10 vs exhaustive
  **0.974** (min 0.8) and top-1 agreement 1.0. Retrieval p50 was 5.3 ms ANN vs **4.6 ms
  exhaustive**, so the auto-index threshold was raised from 5k to 50k vectors. Query embedding
  p50 3.9 ms. Model load 80 ms (BGE) / 260 ms (Nomic).
- **gapmap import** (`--limit 2000`, Lance scan order, a biased sample): 2,000 papers + vectors in
  7.3 s. Roles from gapmap flags: 5 coverage / 87 demand / 1,908 neutral. Search was refused until
  parity ran.
- **Parity** (local ONNX Nomic vs gapmap's sentence-transformers fp16 vectors, n = 200): mean cos
  **0.99907**, p05 0.99409, min 0.99117, so PASS. This confirms that gapmap stored mean-pooled,
  L2-normalized vectors; the P0 spike's layer-norm variant is not gapmap's space. Searching
  2,000 gapmap vectors: 12.6 ms embed + 3.8 ms exhaustive retrieval.

### Decisions and spec conflicts to review
1. **Default model:** BGE-small is the provisional CLI/UI default (P0 speed). SPEC §7.3 still names
   Nomic, and §14 Q1 is open. Retrieval quality has not been compared.
2. **Storage layout:** collections, spaces and ANN parameters live in `project.duckdb`.
   `project.json` holds only the format version, name and network flag. SPEC §7.4 lists
   collections and model ids in `project.json`. `items` adds `chunk_index` and `imported_at`.
3. **Parent vectors** embed the head-truncated full text, and chunks are separate vectors. Chunking
   is by characters; token-aware chunking is a follow-up (see the truncation numbers above).
4. **Imported gapmap space:** query prefix `clustering: ` (symmetric with the documents), and the
   weights SHA is recorded as `unrecorded:sentence-transformers@<rev>`. It is query-only, and a
   passing parity record (mean ≥ 0.995, min ≥ 0.98) gates search.
5. **Search modes:** ANN is used only for indexed tables (≥ 50k vectors). Exhaustive is exact over
   every vector, and the desktop defaults to it. Output always says "nearest items in this
   collection only; similarity is not a calibrated existence verdict".
6. **Network:** inference, import and search make no network calls. However, `lance-namespace`
   unconditionally links a `reqwest` REST client; it is unused because only local paths are
   accepted (`://` is rejected), but this is not a compile-time guarantee. The ONNX Runtime
   binaries are fetched at build time by `ort-sys`.
7. One process per project: DuckDB's file lock rejects a second writer, with an explanatory error.

### Limitations
- Windows/Linux were not built or run, and CI has not run remotely. The 100k / 30 min
  target is not validated; the dev sample is short abstracts. The desktop flow was compiled and
  type-checked, not exercised by clicking through the UI.
- The Apple linker still warns about a large `__eh_frame` section. `target/` is ~29 GB; disk free fell
  to ~9.5 GiB during the build.

## Next — P2 (not started)
1. Decide the default model with a retrieval-quality comparison (BGE vs Nomic, gapmap parity
   now available), and add token-aware chunking.
2. Calibration and verdicts (§6.1–6.2, spec-recommendations 2), on top of the exhaustive route.
3. Run the three-OS CI when a push is authorized; add Windows/Linux inference smoke fixtures.
4. Rename the `invectus-unsearch` crate once a name is chosen.
