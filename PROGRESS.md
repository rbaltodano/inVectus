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
  README and UI text now say "inVectus search". The crate stays `invectus-unsearch` (owner
  decision: internal names are fine as long as nothing user-facing says "unSearch"; checked the
  CLI/app binaries, UI and docs).
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

## 2026-10-27 — P2 engine and CLI (UI screen deferred)

Scope (owner direction): SPEC §11 P2 engine and CLI first; the first UI screen waits for the owner's
designs. Calibration uses the no-LLM fallback (owner decision; no LLM downloaded or called).
Nothing pushed.

### Changes
- **unsearch** (internal crate name): `curve` (weighted isotonic regression, pool adjacent
  violators, P clamped to [0.01, 0.99], AUC, balanced Brier), `calibrate`, `verdict`.
- **Calibration (query level, no LLM).** For each sampled titled item, the title is embedded as
  a query and the item's text alone (no title) is embedded as a document. *Absent* = the best
  similarity to every other stored item (chunks collapsed to parents). *Exists* = the best of
  that and the own-text similarity. The fit is isotonic exists/absent on the top similarity, one
  of each per item (even odds). Every fifth item is held out for AUC / Brier / balanced accuracy.
  Diagnostics: own text vs 20th–50th neighbours (SPEC §6.2's pairs) and title → own-text
  known-item recall among all items. Results go to a new `calibrations` table with the vector
  count; it is a cancellable job record that saves nothing when cancelled.
- **Verdict** (`invectus check`): exhaustive exact retrieval by default (`--fast` allows ANN and
  adds a note). P(exists) comes from the nearest similarity. exists ≥ 0.7 > near ≥ 0.3 > open.
  The output also has the drop to the 10th neighbour and local density (mean of the top 10), a
  calibration quality line, a note when the collection changed since calibration (vector count),
  a note when a role filter is used, and "Not found in …" wording. Regions are noted as P3.
- **Token-sized chunking** (opt-in, `import --chunk-tokenizer <model>`; default 448 tokens, 48
  overlap). It uses the pinned, checksum-verified tokenizer (`ModelTokenizer`, `TokenOffsets`
  in core) and cuts at a paragraph, then a sentence, then a word boundary. The policy records
  the tokenizer SHA-256, and imports with another tokenizer are refused. Character policies
  serialize unchanged, so existing collections keep their hashes.
- **CLI:** `calibrate` and `check`, and the import flags above. **Desktop:** backend commands
  `start_calibration` (progress via `calibrate-event`, cancel via `cancel_job`) and
  `check_idea`. There is no UI yet.

### Checks (arm64 macOS only)
- `cargo test --workspace`: **61 passed** (11 new), with no model-test skips. New tests cover
  isotonic fitting, token splitting (including non-ASCII), the stored-policy hash, calibration
  round-trip, end-to-end calibrate → check (exists / open / role filter, stale, cancel,
  too-few-items), real-BGE tokenizer offsets, and CLI `check`/`calibrate`/token import + embed.
  `cargo fmt --check` and `cargo clippy --workspace --all-targets --locked -D warnings` are clean.
- Release CLI build: 7 m 16 s. Free disk is ~23 GiB with the debug tree rebuilt.

### Measurements (5,000-paper dev sample; M4 Pro; not a 100k validation)
- **Model comparison (SPEC §14 Q1), calibration at 1,000 titles, 200 held out:**

  | | held-out AUC | Brier | bal. acc. | recall@1 | recall@10 | MRR | own vs related AUC | embed 2,000 texts |
  |---|---|---|---|---|---|---|---|---|
  | BGE-small | 0.913 | 0.118 | 0.835 | 0.934 | 0.990 | 0.956 | 0.989 | 29.7 s (67/s) |
  | Nomic v1.5 | 0.933 | 0.100 | 0.880 | 0.940 | 0.987 | 0.961 | 0.995 | 75.9 s (26/s) |

  At 300 titles: BGE AUC 0.890 / Nomic 0.905. P(exists) = 0.5 at a nearest similarity of 0.827
  (BGE) / 0.821 (Nomic).
- **50 held-out titles** (papers *not* in the collection): BGE gave open 39 / near 10 / exists 1;
  Nomic gave open 36 / near 10 / exists 4. The "exists" cases are closely related papers (e.g.
  another Graves' orbitopathy review); none were adjudicated. `check` takes ~0.13 s (BGE) /
  0.26 s (Nomic) per query including process start and model load.
- **gapmap (imported Nomic, parity-verified, 2,000 papers):** held-out AUC **0.804**, recall@1
  0.787. Its symmetric `clustering:` prefix and the dense Lance-order sample are the likely
  causes; not investigated.
- **Token chunking (BGE tokenizer) vs P1 characters:** 674 items chunked into 1,366 chunks (vs
  470 → 947); 464 inputs truncated (vs 584). Since 464 < 674 chunked parents (head-truncated by
  design), this is consistent with no truncated chunks, but it was not verified per input.
  Import took 2.6 s (vs 1.5 s).

### Decisions and spec conflicts to review
1. **Default model stays BGE-small.** Nomic is marginally better on the calibration metrics
   (+0.02 AUC with 200 held-out queries, within noise), BGE gave fewer "exists" on papers not
   in the collection (1 vs 4 of 50), and BGE is 2.6× faster. SPEC §7.3 names Nomic; the owner
   should confirm. This is one domain (ophthalmology abstracts), titles as queries, and no human
   relevance labels.
2. **Query-level calibration instead of SPEC §6.2's pairwise curve**, following P0
   recommendation 2: a pairwise P(same) is not P(exists) for the best match over a collection.
   SPEC §6.2's 20th–50th-neighbour pairs are kept as a diagnostic only. The "absent" class assumes
   the nearest other item is a different thing; duplicates make the curve more cautious. There
   are no adjudicated negatives yet.
3. **Interpolation gap:** when exists and absent scores do not overlap, P between the highest
   absent and the lowest exists score is a linear guess (seen in tests with a toy embedder).
4. **Verdicts default to exhaustive retrieval** (SPEC §6.1 says ANN then re-score). An absence
   claim should not rest on approximate retrieval; at 5k vectors exhaustive is ~5 ms.
5. Thresholds 0.7 / 0.3 are provisional. With a role filter, the curve fitted on the whole
   collection is reused, and a note says P(exists) is approximate.
6. Per-item probabilities were dropped: with a query-level curve only the query has a P(exists).
7. Token chunking is opt-in, not the default, because changing the default would change new
   collections' behaviour silently. Owner call.

### Limitations
- No LLM paraphrases (SPEC §6.2 primary method), so the calibration is the flagged fallback.
  There are no human-adjudicated verdicts, and the thresholds are not tuned.
- Regions ("sits between A and B") need P3 clustering. No UI screen yet.
- Windows/Linux are not built or run. The CLI still panics on a broken pipe (`| head`).

## 2026-10-27 — P3 regions and map, P4 gap scan (engine, CLI, desktop backend)

Owner direction: keep the P2 defaults (BGE-small default, token chunking opt-in, no-LLM
calibration). The designs are coming, so the work is engine, CLI and backend only. Nothing pushed.

### Changes
- **regions** (P0 recommended a kNN-graph/Leiden backend over the tested HDBSCAN/UMAP crates):
  - Exact cosine kNN (k = 30, rayon, already in the lockfile) builds a symmetric graph weighted by
    similarity.
  - Leiden is a new implementation: fast local moving, well-connectedness refinement with greedy
    merges, aggregation on the refined partition, two passes, and a seeded RNG. It handles
    isolated nodes and empty graphs.
  - The resolution is automatic: bisection to about √n fine regions (8–1,000). `--resolution`
    and `--target-regions` override it.
  - Communities under 10 items are noise: they are attached to the nearest centroid and flagged.
  - Broad regions come from Ward agglomeration of fine centroids, so they always nest.
  - Labels use c-TF-IDF with a stop-word list; there is no LLM.
  - Stability is the mean best-Jaccard over seeds 1–4 against seed 0; the gate is ≥ 0.5.
  - The map follows P0's fallback: classical MDS of fine centroids, circles sized √n pushed apart,
    then local PCA inside each circle. It is deterministic and reports trustworthiness@15.
  - Storage: `region_runs`, `regions` (centroids as f32 BLOBs) and `region_members`. The latest
    run replaces earlier regions; run records stay as history.
- **check** gains "Your idea sits between A and B" from the two nearest fine centroids.
- **gaps** (SPEC §5.1–5.3, 5.6–5.7):
  - Coverage reach follows gapmap's rule: an item counts toward its 2 nearest regions when it is
    at or above the region's 25th-percentile member similarity.
  - A coverage gap is no coverage, or ≥ 10 demand per coverage item. Otherwise, a stale gap is
    ≥ 50 % of demand newer than the latest coverage.
  - Bridges: each region's 3 nearest regions; both regions stable and not shrinking; a centroid
    similarity at least the collection's median nearest-region similarity (relevance gate, P0
    recommendation 3); near-duplicate siblings (same broad region, ≥ 0.95) excluded; and a
    bridge ratio ≤ 0.2 (density gate).
  - Momentum: a Poisson IRLS slope over the 6 most recent complete years. Saturation is −1 z when
    coverage falls within 24 months.
  - Scores are z-scored per type, and the stored terms (value, z, weight, contribution) sum to the
    score. Dates after today are ignored and noted. Regions with stability < 0.5 are not ranked.
  - Human verdicts (open / confirmed_gap / already_exists / not_viable / selected + note) are
    stored by the gap's 20 core items. They carry over to a re-run's gap of the same type at
    Jaccard ≥ 0.5.
  - Query voids (need a search log) and density holes (exploratory) are not scanned, and the
    output says so.
- **CLI:** `regions run|list|map --out`, `gaps scan|list|show|verdict`. **Desktop backend:**
  `start_regions` (`regions-event`), `region_map`, `scan_gaps`, `list_gaps`, `gap_detail`,
  `set_gap_verdict`. There is no UI.

### Checks (arm64 macOS only)
- `cargo test --workspace`: **71+ passed**. New tests cover:
  - Leiden: planted cliques for 5 seeds, resolution, reproducibility, isolated nodes.
  - Ward, c-TF-IDF, best-Jaccard, layout containment and trustworthiness, Poisson slope.
  - Region job end to end: 4 planted topics, labels, stability, map containment, the auto
    target, rerun replacement, and cancel keeping the old run.
  - Gap scan end to end: planted coverage, stale and bridge gaps; explained scores; future-date
    note; verdict validation; and a verdict carried over a re-clustering.
- fmt and clippy (`-D warnings`) are clean. The lockfile only gained internal crates.

### Measurements (dev data; M4 Pro; not a 100k validation)
- **Demo, 5,000 papers, BGE, automatic resolution 7.34:** 71 fine / 8 broad regions; sizes
  15/66/155; 17 noise items (0.3 %); 58/71 stable (mean 0.685); modularity 0.342; map
  trustworthiness@15 **0.893**. kNN took 0.4 s, clustering 5 seeds plus the search 0.2 s, total
  1.1 s. Labels are specific ("keratoconus · corneal · keratoplasty", "graves · thyroid ·
  disease", "lasik · laser · ablation"). Resolution 1.0 gave only 10 coarse regions.
- **Gap scan on the demo:** 0.22 s, with 56 coverage gaps, 0 stale and 0 bridges (13 unstable
  regions skipped; 4 future-dated items ignored). The demo's P1 role rule marks every "Journal
  Article" as demand (4,922 demand vs 66 coverage), so basic-science regions rank high: that is
  a mapping artefact, not a finding. Bridges: even with no ratio gate only 1 pair passes the
  other gates (ratio 0.27 at similarity 0.943). The 0.2 default yields none on this sample.
- **gapmap (2,000 papers, imported Nomic):** 45 regions (35 stable), trustworthiness 0.916, 0.7 s;
  3 coverage gaps. `check` on a held-out title: exists, P 0.84, *"sits between 'graves · thyroid ·
  serum' and 'orbital · orbit · tumor'"*.
- These trustworthiness values are on different samples from P0's UMAP numbers (Python 0.866,
  Rust 0.62), so they are not a like-for-like comparison.

### Decisions and spec conflicts to review
1. **Leiden instead of HDBSCAN** (SPEC §6.4 allowed either; P0 rejected the HDBSCAN crate). This
   is my implementation with greedy rather than randomized refinement. It is not yet compared with
   a reference Leiden or with gapmap's leaf HDBSCAN on the same data.
2. **Automatic resolution to ≈ √n regions** is my heuristic. For gapmap's 591k it would aim at
   ~770 (gapmap found 656). SPEC says target sizes are set per collection; both flags override it.
3. **Map = MDS + local PCA, not UMAP** (P0 fallback). Distances between regions are meaningful;
   inside a circle only the two main directions of variation are shown.
4. Coverage-gap vs stale split (ratio ≥ 10 → coverage gap; else ≥ 50 % newer → stale) and all the
   default weights are untuned. The backtest is meant to tune them.
5. Exact kNN is O(n²): fine at 5k (0.4 s). 100k is estimated at minutes but not measured; ANN
   neighbours would be the fallback.

## 2026-10-27 — P7 backtest harness (§9.1)

- `invectus eval backtest --cutoff A,B,… --horizon-years N` (`invectus-eval`). For each cutoff it
  builds a throwaway project from the dated items up to that cutoff (undated items are excluded,
  since they could leak the future). It copies their stored vectors (no re-embedding), reruns
  regions (automatic resolution) and the gap scan as of the cutoff, and then marks a coverage or
  stale gap **filled** when coverage dated within the horizon reaches its region (the scan's own
  reach rule). Bridges are reported separately.
- Metrics: precision@10/25 and ROC-AUC for the model ranking against raw-count (demand items) and
  density (region size) baselines on the same gaps. The random baseline is the base rate
  (AUC 0.5). Cutoffs with no filled or no unfilled gaps are left out of the means.
- Without an LLM there is no midpoint description, so "the same thing" (§9.1 step 3) is judged by
  region reach.
- Tests: ranking metrics, plus an end-to-end test where reviews arriving after the cutoff fill
  exactly their region's gap. `cargo test --workspace`: **73 passed**, and clippy/fmt are clean.
- **Demo result (5,000 papers, BGE, cutoffs 2016/18/20/22, horizon 2 years, 10.9 s):**

  | ranking | p@10 | p@25 | AUC |
  |---|---|---|---|
  | model | 0.075 | 0.130 | **0.386** |
  | raw-count | 0.200 | 0.180 | 0.553 |
  | density | 0.175 | 0.180 | 0.551 |

  The base rate was 8.5–24.5 % of 46–53 gaps per cutoff (4–13 fills). The model ranks **below**
  both baselines, so SPEC §9.3's gate (beat the baselines) **fails** on this data. There are
  likely reasons: new reviews land in large, already-reviewed areas, while the score rewards
  regions with no coverage; the demo's crude P1 role rule (every "Journal Article" is demand,
  only 66 coverage); and very few fills. This is not yet evidence either way about the method on
  gapmap-quality roles. Weights were **not** tuned: 4 cutoffs with 4–13 fills each would overfit.

## 2026-10-27 — Gap reports (SPEC §4 step 8)
- `invectus gaps export --out report.md|report.csv`: rank, type, score, label, verdict, note and
  explanation. The Markdown report adds the 5 central items of each gap as titled links. Source
  text is never exported; a test asserts that item text is absent. `cargo test --workspace`:
  **74 passed**.

## 2026-10-27 — Full gapmap benchmark (owner approved copying beyond 10k, read-only)

- **Copy:** `gapmap import` without a limit wrote all **591,484** papers and their stored Nomic
  vectors to `.spike-data/p1/work/gapmap-full.invectus` (2.4 GB) in 13 min 4 s. Roles: 8,462
  coverage / 235,550 demand / 347,472 neutral.
- **Reference safety:** reads used read-only DuckDB and Lance scans. A per-file listing (path,
  size, mtime) of `data/gapmap.duckdb` and `data/vectors/` (233 files) is saved in
  `.spike-data/p1/reference-fingerprint-2026-10-27.txt`. No file or folder there has an mtime
  after Oct 26 18:57 (before this session's work). The listing hash `57523207…` reproduces. A
  first fingerprint taken just before the import (`fad1d81f…`) did not reproduce, and I cannot
  explain it. P1's `3375279f…` (232 files) used a different script, so it is not comparable.
- **Scaling fixes found by the full set:**
  - Exact kNN would take hours at 591k. Regions above 40k items now use an inverted-file
    approximate kNN (√n spherical k-means lists, 8 probes) and report recall against exact
    neighbours on 200 items.
  - The fixed "≥ 10 demand per coverage" threshold flagged 600 of 709 stable regions, because
    this field averages 28 demand items per review. Coverage gaps now need at least twice the
    collection's own ratio (here ≈ 56).
  - Item-to-centroid similarities are now computed in parallel, and demand is counted in one
    pass.
- **Regions (591,484 items):** 769 fine / 28 broad regions; sizes 36/673/3,062; 0 noise;
  **709/769 stable** (mean 0.822); modularity 0.714; trustworthiness@15 0.758. Approximate kNN:
  763 s with **recall@30 = 0.918**. Clustering (search plus 5 seeds) 62 s; total **15 min**;
  peak RSS **6.7 GB**. Labels are specific ("dmek · descemet · endothelial", "rop · prematurity
  · infants", "psp · supranuclear · palsy").
- **Gap scan:** 177 coverage gaps, 15 stale, 0 bridges (60 unstable regions skipped; 451
  future-dated items ignored). It took 171 s before parallelizing.
- **Backtest (cutoffs 2014/16/18/20, horizon 2 years; 356k–469k items per past; 220–265 gaps
  and 52–86 filled per cutoff; 37 min; peak RSS 13.1 GB):**

  | ranking | p@10 | p@25 | AUC |
  |---|---|---|---|
  | model | 0.450 | 0.380 | **0.550** |
  | raw-count | 0.550 | 0.610 | **0.705** |
  | density | 0.525 | 0.420 | 0.662 |

  Every cutoff was informative. The model beats chance but not the baselines, so the SPEC §9.3
  gate still **fails** on the real benchmark. Bridges: 7 across the 4 cutoffs, 3 filled.
  "Filled" asks where reviews get written next, and raw demand predicts that well. Whether that
  is the right definition of a gap (rather than "a review would be valuable here") is a product
  question for the owner.
- **Per-term ablation** (rerun with the same cutoffs; identical model and baseline numbers; 36.6
  min, 12.9 GB). Each coverage-gap term was used alone as a ranking, over coverage gaps only, so
  these are not directly comparable with the baselines above, which rank coverage and stale
  gaps together:

  | term alone | weight now | p@10 | p@25 | AUC |
  |---|---|---|---|---|
  | demand per coverage (log) | 0.5 | 0.500 | 0.460 | **0.661** |
  | demand since last coverage (log) | 1.0 | 0.375 | 0.360 | 0.536 |
  | momentum | 0.5 | 0.200 | 0.200 | 0.539 |

  The most predictive term has the lowest weight, and the most heavily weighted term barely
  beats chance. Weights were **not** changed: each backtest takes ~37 min, and picking weights
  on these 4 cutoffs without a held-out cutoff (§9.1 step 4) would overfit. A proper tuning run
  should cache each cutoff's regions and gap terms and then search weights with
  leave-one-cutoff-out.

## 2026-10-27 — Gap-score weight tuning (leave-one-cutoff-out)

- `invectus eval backtest --json` now saves every ranked gap with its z-scored terms and
  outcome. `invectus eval tune --from <file>` grid-searches the four weights (0, 0.25, 0.5, 1,
  2 each) on all cutoffs but one and scores the held-out cutoff. Tuning takes seconds; only the
  backtest is slow. Each cutoff now runs as its own process, so one failure loses one cutoff.
- Memory: scans and backtests load only role and date per item (`Store::item_facts`), and
  backtests copy items into the past project in batches of 20k.
- **Held-out results, full gapmap, cutoffs 2014/2016/2018/2020** (4 folds):

  | ranking on the held-out cutoff | p@10 | p@25 | AUC |
  |---|---|---|---|
  | tuned weights | 0.500 | 0.370 | **0.623** |
  | previous default weights | 0.450 | 0.380 | 0.550 |
  | raw-count baseline | 0.550 | 0.610 | **0.705** |

  The chosen weights were consistent: demand per coverage **2.0** in every fold, demand since
  last coverage **0** in every fold, momentum 0.25 / 0.5 / 0.5 / 1 (0.5 on all cutoffs), stale
  share 0 in every fold.
- **Defaults changed** to demand per coverage 2.0, demand since 0, momentum 0.5. Stale share
  stays 1.0: only ~15 stale gaps, which is too little evidence to zero it. Tuning improves
  held-out AUC by +0.073 but the model **still trails raw count** (0.623 vs 0.705). §9.3's gate still fails.
  The score terms are relative measures, so they cannot express "big regions get reviewed
  next". Whether the score *should* predict where reviews appear (versus where one is most
  needed) is the open product question.
- The 2020 cutoff (469k items) was killed by the OS three times (SIGKILL at ~5 min, ~10.5 GB
  RSS) while swap was nearly exhausted (5.1 of 6 GB used) and free disk was low. It completed on
  the fourth try (10 min 53 s, 11.3 GB) after the leftover temp projects were removed. Killed runs left four 1.8–2.0 GB temp
  projects behind; these were removed. Temp cleanup on SIGKILL is not possible from inside the
  process.

## 2026-10-27 — Local language model (owner chose local; approved the 6.98 GB download)

- **Model:** `google/gemma-4-12B-it-qat-q4_0-gguf@29d09777…`, file
  `gemma-4-12b-it-qat-q4_0.gguf` (6,975,879,296 bytes, Apache-2.0, ungated). It is Google's
  quantization-aware-trained 4-bit GGUF; the 0.18 GB vision projector was not downloaded. The
  download's SHA-256 `93567e57…` matches the pinned value. It is installed in the dev model store
  (`.spike-data/models-store/gemma-4-12b-it-qat-q4_0/model.gguf`). Qwen 3.5 9B was considered,
  but Qwen publishes only BF16 safetensors (19.3 GB, no official GGUF), which did not fit the
  free disk. Free disk was 25 GiB before the download and ~18 GiB after.
- **`invectus-llm` crate:**
  - A pinned registry (repository, revision, size, SHA-256, license, context 8,192).
  - Checksum verification cached by a size+mtime stamp.
  - A `TextGenerator` trait with a llama.cpp backend (`llama-cpp-2 =0.1.160`, built from
    vendored source with cmake; Metal with all layers on the GPU on macOS). It loads local files
    only and runs offline.
  - llama.cpp's built-in template engine does not know Gemma 4's `<|turn>` format
    (`apply_chat_template` → ffi error −1). A fallback reproduces the model's own Jinja template
    output for one system and one user turn with thinking disabled (empty thought channel), and
    generation stops on the turn-ending control token. Every output records the model identity.
- **CLI:** `invectus llm list | verify | generate`, and `invectus regions label` (LLM region
  names, resumable, skips regions that already have one, Ctrl-C keeps progress). `regions list`
  shows the LLM name next to the c-TF-IDF terms. The model sees each region's top terms and the
  titles of its 10 most central items, never full texts. The labels are stored in new
  `regions.llm_label` / `llm_model` columns, added with `ALTER TABLE … ADD COLUMN IF NOT EXISTS`,
  so existing projects upgrade in place.
- **Measured (M4 Pro, Metal):** load 1.2 s with a cached checksum; ~15 tokens/s end to end on a
  short prompt. Labelling the demo's **71 regions took 140 s (2.0 s per region)**, e.g.
  "Surgical Treatments for Nasolacrimal Duct Obstruction", "Genetic and Clinical Aspects of
  LHON", "Choroidal Thickness and Optical Coherence Tomography". The full set (769 regions)
  would take ~25 min. Labels are not human-reviewed. A few are broad ("Ocular Side Effects and
  Clinical Findings").
- Tests: checksum verification, Gemma 4 prompt format, label cleaning. Clippy and fmt are clean.
  The lockfile gained `llama-cpp-2` / `llama-cpp-sys-2` 0.1.160 and their dependencies.
- **Not yet:** LLM calibration paraphrases (SPEC §6.2) and midpoint descriptions with the
  round-trip check (P5, §6.3). Windows/Linux llama.cpp builds are untested.

## 2026-10-27 — P5 gap descriptions (midpoints) with the local model

- **`invectus-midpoint`** (SPEC §6.3):
  - **Targets.** *Between two fine regions* at `t`: the slerp of the centroids. The model sees
    the titles nearest each centroid and nearest the point, and a description is **on target**
    when its embedding is closer to the point than to either centroid. *One region's missing
    coverage* (coverage and stale gaps): the model sees the region's central demand titles and
    the 3 nearest coverage items, and the description is on target when its nearest fine region
    is that region.
  - **Novel** = an inVectus check of the description (calibrated, exhaustive; coverage-only for
    region targets) is not `exists`. It is skipped, with a note, when there is no calibration.
  - **Attempts.** Up to 3: the first greedy, later ones sampled (temperature 0.8, seeds 1, 2).
  - **Storage.** Every run goes to a new `midpoints` table: target, attempts, both checks, the
    context titles, and the LLM and embedding identities. The model sees titles, roles and years
    only.
- **CLI:** `invectus gaps describe <id>` (bridge → between its regions, coverage/stale → its
  region) and `invectus midpoint --a A --b B [--t]`. `--genre` / `--coverage-genre` default to
  "research study" / "systematic review". **Desktop backend:** `describe_gap`.
- Tests: slerp, reply parsing, and an end-to-end test with a scripted model (an off-target
  first reply is rejected and the retry accepted; a region target; prompts carry no item text;
  stored identities; t out of range). `cargo test --workspace` (release profile): **84 passed**.
  Clippy and fmt are clean.
- **Demo runs (BGE, Gemma 4 12B, M4 Pro):**
  - Coverage gap "Visual Perception of Color and Luminance" (82 demand, no review): accepted on
    the first attempt in 5.7 s. *"Neurophysiological and Computational Models of Chromatic and
    Luminance Perception"* (review); nearest region = target (0.903); novel (open,
    P(exists) 0.07).
  - Between "Biomechanical Properties and Parameters of the Cornea" (47) and "Choroidal
    Thickness and Optical Coherence Tomography" (62), t = 0.5: 3 coherent study designs
    (Corvis ST hysteresis × SD-OCT choroidal thickness in myopia), 6.8–7.7 s each. Two of three
    were on target, but **all were rejected as `exists`** (P 0.92–0.94) because the nearest
    paper is a corneal-biomechanics study at similarity 0.877 that does not involve the choroid.
- **Limitation found:** a single-vector novelty check cannot tell a new *combination* from
  papers on one of its halves, because a description of A×B sits close to strong A papers. The
  calibration was fitted on title→own-text pairs, not on combinations.
- **Fixed (owner approved): combination rule for between-region targets.** Of the description's
  20 nearest items, only those that reach **both** regions count. "Reach" is the scan's rule: at
  least the region's 25th-percentile centroid similarity, measured on each item's stored vector.
  P(exists) comes from the nearest such item through the collection's curve. The nearest
  one-sided item is reported as "not counted". Region targets keep the single-item check
  (coverage only). Rerun of 47 ↔ 62: the first attempt is **accepted** as `near`
  (P(exists) 0.47; nearest two-sided item "Choroidal Thickness and Associated Factors among
  Adult Myopia…" at 0.824). The corneal-only Corvis ST paper is set aside. Reusing the
  single-item curve on a filtered set is approximate, as with role filters. Combination
  verdicts are not human-reviewed. Tests: 85 passed.

## 2026-10-27 — Verdict calibration with LLM paraphrases (SPEC §6.2 primary method)

- `invectus calibrate --llm [model]` (desktop: `start_calibration` with `llm`). Gemma rewrites
  each sampled item, from its title and the first 600 characters of text, as a ≤ 40-word search
  idea "in your own words". The query-level fit compares the paraphrase with the item **as
  stored** (exists) against the best other item (absent). Method, model and 20 example
  paraphrases are stored with the curve. Without `--llm` the title fallback is unchanged. A
  test with a scripted paraphraser covers the method name, examples, holdout and check.
- **Demo (BGE, 300 items, 60 held out; 10 min 40 s; 9.9 GB peak):**

  | calibration | held-out AUC | balanced Brier | balanced accuracy |
  |---|---|---|---|
  | **Gemma paraphrases** | **0.952** | **0.092** | **0.858** |
  | titles (no LLM, same sample size) | 0.890 | 0.141 | 0.808 |

  P(exists) = 0.5 at a nearest similarity of 0.829 and 0.7 at 0.841. Example: "Change in
  intraocular lens power calculation results after ptosis surgery" → "Does surgery to repair
  drooping eyelids affect the accuracy of measurements used to calculate intraocular lens
  power?"
- **50 held-out papers not in the collection** (their titles as queries): open 40 / near 8 /
  exists 2, versus 39 / 10 / 1 with the title calibration. Both "exists" cases are near-topic
  papers (corneal endothelium after keratoplasty; Usher syndrome genetics) and need human
  judgement. The paraphrases are not human-checked, and SPEC §9.2's 300 hand-labelled queries
  and ECE ≤ 0.1 are still not measured.
- The demo's latest calibration is now the paraphrase curve, so `check` uses it. The title
  curve stays in history.

## 2026-10-28 — P6 verification against outside sources (owner approved live PubMed/OpenAlex calls)

- **`invectus-verify`:**
  - **Connectors.** PubMed E-utilities (esearch, then esummary; terms in `[tiab]`; reviews via
    `review[pt] OR systematic review[pt] OR meta-analysis[pt]`) and OpenAlex `/works`
    (boolean full-text search; `filter=type:review`).
  - **Requests.** Explicit only. Refused under `--offline`. Refused when the project's network
    switch is off unless the command passes `--network`. Rate-limited (PubMed ≥ 400 ms apart,
    OpenAlex ≥ 150 ms), retried up to 3 times with backoff, and sent with an inVectus
    User-Agent. No email or API key is sent.
  - **Cache.** Answers are cached in `cache/verify/` for 7 days, so repeats work offline.
  - **Gap verification.** The keyword query comes from region terms. Coverage and stale gaps
    use the top 2 terms ORed AND the next 3 ORed, reviews only; bridges use (side-A top 3) AND
    (side-B top 3). Hit titles are re-ranked with the local embedding model against the gap's
    accepted description (or its label when none exists). Hits at or above the calibration's
    P = 0.5 similarity are marked "looks like a match". Results are stored in a new
    `verifications` table.
- **CLI:** `invectus gaps verify <id> [--sources pubmed,openalex] [--network] [--max N]`.
  **Desktop backend:** `verify_gap`.
- Tests: query syntax for both sources, parsing fixture replies (including missing fields and
  error shapes), and offline refusal with the cache still served. `cargo test --workspace`
  (release profile): **88 passed**. Clippy and fmt are clean.
- **Live run, demo gap 1** (coverage gap "Visual Perception of Color and Luminance", compared
  with its accepted Gemma description): 1.8 s. PubMed reported 1,125 reviews for the keywords
  and OpenAlex 79,219 (20 fetched each). None crossed the 0.829 match line, but the top titles
  were plainly relevant: "Cortical mechanisms of colour vision" (2003, 0.824) and "The Retinal
  Basis of Vertebrate Color Vision" (PubMed 2019, 0.795). A human reading says reviews on this
  topic exist, so the gap mostly reflects the 5,000-paper sample having none. That is the
  gapmap lesson the spec cites. The automatic threshold, fitted on paper-to-paper similarity, is
  conservative for title-only hits. The repeat run was served from the cache under `--offline`.
- Not done: the Steam and other-collection connectors, batch verification (`--top N`), a
  title-calibrated match threshold, and showing verification in gap lists and reports.

## 2026-10-29 — Verification everywhere, batch verify, two fixes

- Gap lists, details, the desktop `list_gaps` / `gap_detail` and both report formats carry each
  gap's latest verification: the summary and the 3 closest outside titles with links. CSV gains
  `outside_matches` and `outside_summary`.
- `invectus gaps verify --top N` checks the N highest-ranked gaps in one run. Demo top 3: 6.8 s
  (one from cache). In each case relevant reviews exist outside the 5,000-paper sample (e.g.
  "Pathology of eyelid tumors" for "Malignant Tumors of the Orbit and Eyelid"), although none
  crossed the automatic match line. The keyword queries are broad: up to 262k OpenAlex reviews
  match the term ORs. Sharper queries, for example written by the local LLM, are a follow-up.
- Fixed: the CLI no longer panics when stdout closes early (`| head`); SIGPIPE's default
  action is restored on Unix. The desktop session now keeps Gemma loaded after its first use
  instead of reloading it for every gap description.
- Disk: `target/release` had grown to 24 GB with only 3.7 GiB free. Its intermediates were
  removed (executables kept); free space is ~22 GiB after this rebuild.

## 2026-10-29 — Desktop UI from the wireframe (owner: "use what you have as a starting point")

- Replaced the P1 single-page UI with the 10-board wireframe (artifact "inVectus Wireframes"):
  a sidebar shell (Projects, Import, Index, Search, Map, Gaps, Settings, a Jobs panel) and a
  header with the collection/model picker and the offline-mode switch. Screens: Projects
  (recent list in browser storage, new/open), a 5-step Import wizard (drag-and-drop paths,
  guessed column mapping, role rules with live counts over the first 1,000 rows, preview,
  result), Index (model choice, download dialog, embed → calibrate → regions → score gaps with
  live progress), Search (calibrated verdict or plain similarity when uncalibrated, "sits
  between" regions), Map (canvas; colour by region/role/date/gap score, top-15 gap markers,
  non-overlapping labels, zoom/pan, click/box selection), Gaps (filters, batch verify, CSV and
  Markdown export, re-score), Gap detail (score terms, per-year chart, LLM description with
  on-target/novelty checks, central items, verdict and note, outside-source check), Settings
  (models, offline mode, local LLM status, per-project gap weights, calibration, embedding
  details). Styles are tokens in `src/App.css` for the owner's later style pass.
- New desktop commands: `inspect_source`, `set_network`, `list_llms`, `install_model`
  (explicit; refused while the project is offline), `default_gap_weights`, `export_gaps`
  (into the project's `exports/`), `open_url` (http(s) only, system browser); `scan_gaps`
  takes optional weights; `project_info` reports network state and each table's calibration
  and regions. Typed paths expand `~`. Item links that hold a bare DOI open via doi.org.
- Not done from the wireframe: native file pickers (needs the Tauri dialog plugin, a new
  download), "Save as gap"/search voids and midpoints from a search (no engine support yet),
  choosing outside sources (fixed to PubMed + OpenAlex), cloud LLM providers (local only).
- Checks: every screen was clicked through in a browser with the engine calls stubbed by real
  demo-project JSON (gaps, gap detail, map with 5,000 points, a calibrated check). The release
  desktop build compiles; the native window was not driven by an agent. Gap weights and the
  recent-projects list live in per-machine browser storage.

## 2026-10-29 — Project-centred UI, automatic indexing, backend job orchestration

Owner's brief: consolidate navigation around projects, index automatically after import, keep
evidence, scope and background status visible, monochrome mission-control look.

### Changes
- **Navigation.** The sidebar holds a project filter, the projects list (alphabetical, so
  opening one never moves the list under the pointer; the landing page keeps recency order),
  New / Open, an Activity box (the running job, its project, progress, Cancel) and Settings.
  Import, Index, Search, Map and Gaps are no longer global. A project opens a workspace with
  Overview, Search, Gaps, Collections and Map, plus a scope picker (collection · model) and the
  network switch. Creating a project lands on an Overview that offers the first collection;
  an empty project stays usable.
- **Collections** replaces Index: per collection the items, roles, source kind/path, last
  import, and per vector space the model, vectors and a recorded index state; Add collection
  opens the existing import wizard; "Update from source" re-imports the stored source,
  mapping and chunking (source files are not watched, and the UI says so). Details keep
  calibration (with or without the LLM), regions and indexing with another model.
- **Automatic indexing.** `import_collection` / `reimport_collection` import, then plan and
  start indexing in the backend: each embedded space is updated with its own model; a new
  collection gets the default model or the first installed one; a missing model is reported,
  never downloaded (the consent panel keeps size, source, pinned revision and checksum, and
  is disabled while offline). Imported (gapmap) vectors are query-only and not updated.
- **Job orchestration (backend).** A job board in `src-tauri/src/jobs.rs`: one job at a time,
  each tagged with its project, collection, space and models; progress, cancel, failure and a
  session history; one `job-event` stream carries the whole job. Jobs run on a second DuckDB
  connection (`Store::try_clone`, `Project::try_clone`) and their own LanceDB handle, so the
  project keeps answering reads while indexing (before, every view errored "busy"). Imports
  and scans are refused while a job runs in the same project; switching projects is refused
  while a project job runs (model downloads do not block). A panicking job is recorded as
  failed. Calibration borrows the session's loaded language model instead of loading a copy.
- **Readiness from records only.** `index_status` per space: indexing, indexed (last embed job
  completed after the last import), outdated (imported since), interrupted (a job record left
  "running" by a restart or crash), failed (with the recorded error), cancelled, not indexed,
  imported (query-only) or unknown (vectors without a job record). Resume/Retry re-run the
  resumable embed job. Project info also returns the recorded job log (survives restarts).
- **Views.** Search shows the assessment (verdict, P(exists) dial with the engine's 0.3/0.7
  lines, calibration quality, regions) apart from the results; each result has its title
  (linked), role, date, source id, matched chunk and a labelled cosine similarity, with a note
  that similarity is not a probability and covers this collection only; exhaustive/approximate
  and partial-index warnings stay. Gaps holds the scan controls and offers "Find regions" when
  regions are missing; Gap detail separates the engine's assessment from your decision. The map
  is monochrome: region circles from the layout's centres and radii, grey shades for role,
  date and gap score. Settings splits app-wide (models, local LLM) from project (network,
  weights, calibration, embedding details).
- **Style.** Black on white, square corners, monospace labels, thin rules, circular status
  marks (ring/half/full/cross), a pipeline diagram drawn from the scope's recorded state.

### Checks (arm64 macOS only)
- New Rust tests: planner (default, fallback, missing, existing spaces keep their models),
  status rules, board (one job at a time, stale ids), **end to end against the installed
  bge-small model**: import a 30-row CSV → plan → index on a second connection while the
  session stays free → "indexed" with 30 vectors → re-import → "outdated" → re-index embeds 0
  → a job with an unknown model fails with its error → retry completes; every event carries
  the project root. A guard test: a running project job blocks imports and switching; a
  download does not.
- Browser checks with a stub backend (real demo JSON for search, gaps, gap detail and the
  5,000-point map; simulated job events): import → automatic indexing → progress in the page
  and sidebar → indexed → search; failure → Retry; no model → consent panel, nothing
  downloaded, disabled offline; switching blocked during a job; Overview, Gaps, Gap detail,
  Map and Settings. No console errors. Found and fixed: the sidebar re-sorted under the
  pointer.
- `cargo fmt`, clippy (`-D warnings`), the workspace tests and `npm run build` pass.

### Unverified / limitations
- The native window was not driven by an agent; the end-to-end path was tested through the
  backend functions, not the webview. Timestamps assume DuckDB stores `now()` in local time.
- One job at a time: a second request is refused with the running job's name (no queue).
- While indexing, reads run concurrently on DuckDB/LanceDB; writes other than verdicts,
  descriptions and verification are refused. Search during indexing sees a partial index and
  says so.
- Session job history is in memory; after a restart only the recorded job log remains.

### Decisions and spec conflicts to review
- SPEC §4 lists "Index" as a user step and §8 an "Import wizard … → index" screen: indexing is
  now automatic after import, and calibration, regions and scans moved into Collections,
  Gaps and context prompts rather than a separate screen.
- SPEC §7.6 lists import, cluster, scan and verify as jobs: on the desktop, index, calibrate,
  regions and model download are board jobs; import, scan and verify remain request/response
  (seconds on the dev data) and are refused while a job runs.
- SPEC §8 asks for a WebGL map coloured by region: the canvas map is monochrome by the
  owner's design direction; regions are circles rather than colours.

## 2026-10-29 — Data-circle UI from the owner's Figma frames (dark)

Owner's frames (Figma JBM9BM0Zo6JHgj7NCjxcRt, node 1214:805) and answers: dataset as a circle,
empty circle opens a native picker, points as little squares that merge when they overlap,
clipped with zoom and pan, labels off by default, region outlines on hover, gaps as marks in the
circle, menus to the right of the circle that swap in place with Back, the old sidebar kept,
one circle per project, Martian Mono, dark mode only for now.

### Changes
- **Shell.** Sidebar as in the frames: logo, project search, Projects, Import, Index, Map,
  Settings, plus the running job. Projects is a card grid with New Project. A project opens
  the circle with a 414 px menu beside it; Import, Index and Map (sidebar or menu) replace the
  menu with their page and a Back button; Search, Gaps, gap detail and item detail stack the
  same way.
- **Data circle** (`src/DataCircle.tsx`, canvas). Items are 4 px squares at their map
  positions; squares within one 10 px screen cell merge into one larger square (log-sized,
  capped inside the cell) that splits as you zoom; clicking a merged square zooms to it,
  a single one opens the item. Clipped to the circle; wheel zoom, drag pan, reset. Region
  outlines and names are hidden by default (outline on hover; toggles in Map). The top 12 gaps
  are rings; a search places an accent square at an estimated position (between its two
  nearest regions weighted by similarity, else the mean of its nearest items) and says so.
- **Loading (both A and B).** While a job runs the rim fills as a progress arc with the real
  phase and counts in the centre; placeholder squares appear in proportion to progress and are
  labelled as placeholders; when the map arrives they glide to real positions.
- **Empty and new project.** The empty circle is the add-dataset button; it opens the system's
  file dialog (`tauri-plugin-dialog`, owner-approved download; CSV/TSV/Parquet filter, or a
  folder). On the New Project page, picking a dataset creates the project (named after the
  file unless named) and goes straight to the column step in the menu.
- **Pipeline.** A fresh import now runs index → map (regions) → gap scan → title calibration
  in one job, so the circle fills and search has a verdict. Mapping, scanning and calibration
  failures are reported in the job's report, not fatal (small collections may not form
  regions); mapping and calibration are skipped when nothing changed and they exist.
- **Type and colour.** Martian Mono (variable, bundled from `@fontsource-variable`, OFL);
  tokens from the frames: #121212, #1e1e1e, #f7f3eb, #dedede, #c1c1c1, accent #6edcaa. Icons
  are the frames' SVGs in `src/assets/icons/`.
- Removed the standalone Overview, Search, Gaps and Map screens (their behaviour moved into the
  menu pages and the circle). Batch verification is now "Verify top 5" of the shown gaps.

### Checks (arm64 macOS only)
- The end-to-end Rust test now imports 240 items and asserts regions, a gap scan and a
  calibration after the pipeline, besides the earlier re-import, failure and retry steps.
- Browser checks with the stub backend: Projects grid, New Project → circle click (stubbed
  dialog) → columns → import → progress arc and placeholders → glide to the map; demo project
  home menu, search (verdict above rankings, accent query square, top item outlined), Index,
  Gaps → gap detail, Settings. No console errors.
- `cargo fmt`, clippy, workspace tests, `npm run build` and the release app build pass.

### Unverified / limitations
- Not driven in the native window: the real file dialog, drag-and-drop and canvas performance
  on 591k points are untested by an agent. Merging is per frame over all points (O(n)).
- Region names come from the LLM labels when present, else c-TF-IDF terms.
- Light mode is not implemented (owner will design it).

### Decisions and spec conflicts to review
- SPEC §8 lists a WebGL scatter with colour by region/role/date: the circle is monochrome
  squares on a canvas, with colour reserved for the search accent.
- The import pipeline now also maps, scans and calibrates automatically (SPEC §4 lists these
  as separate user steps).

## 2026-10-29 — Target product documented (agent edition, full app, MCP)

- Added [docs/target-product.md](docs/target-product.md): the owner-approved final form. An
  AI agent does the legwork through an MCP server (`invectus mcp`, stdio, no network port);
  people review and decide in the desktop app or CLI. One engine, two editions: a headless
  agent edition with no local language model, and the full app with an optional one. The
  embedding model is required in both. Covers the tool list and guardrails, agent-written
  descriptions/paraphrases/region names that pass the existing checks, decision provenance
  (person vs agent, suggestions vs decisions), the DuckDB single-writer plan for app and agent
  on one project, a built/planned status table and roadmap A1–A8.
- SPEC.md gains §7.8 (editions and agent interface, marked planned) and milestone P9; README
  links the document. Documentation only: no code changed, nothing built or measured.
- Decisions and spec conflicts to review: SPEC §1.3 lists "not a RAG chatbot" and §7.2 shows
  only the desktop shell over the crates; the MCP server is a new front end over the same
  engine, not a chatbot. §14.3 (which local LLM by default) narrows: the agent edition has
  none, and a small model for the full app needs measuring against Gemma 12B first.

## 2026-10-29 — Instrument visual direction (retro mission control)

- Owner's references (radial charts, technical cards with dials, line-diagram icons, an annotated
  orbital plot, wireframe geometry) replace the terminal look with an instrument-panel one.
  Added a final theme layer in `src/App.css`: near-black ground, white line work at a few
  strengths, a system sans for reading and Martian Mono only for codes, numbers and small
  tracked labels; open panels with hairlines instead of boxes; coded tabs (`01 Search`) with a
  tick under the active one; verdicts as a dot and label; similarity as large light numerals;
  thin meters and one-pixel year bars; rounded project cards.
- The data circle is drawn as an instrument: dial ticks on the rim (every 3°, long every 30°)
  that light clockwise as progress while loading, dashed axes and two range rings fixed to the
  circle, fainter item squares, the search as an accent crosshair labelled QUERY, marked results
  as accent × with their similarity, and gap rings with codes (`G-01`…; labels on the top six,
  all in the tooltip) that match the gap list.
- `src/LineArt.tsx`: thin SVG diagrams (sphere, Venn, rings, orbit, grid) for empty states; they
  name a concept and show no data.
- Checked in the browser with the stub backend (workspace, search, gaps); TypeScript and Vite
  build pass; the desktop app was rebuilt and relaunched. Not reviewed in the native window by
  an agent.

## 2026-10-29 — Plain ring again; gap charts animate in

- Owner reverted the dial: the circle is a plain ring again (no ticks, axes or range rings),
  with the progress arc while loading. Coded gap rings and labelled search marks stay.
- Gap detail charts now enter with Angrove's motion: the score-term bars fill one after another
  with its connector curve (ease-out quint, 0.55 s, 110 ms apart); the per-year bars spring up
  from the baseline left to right on a short stagger using Motion.springRelaxed (response 0.5 s,
  damping 0.8), simulated in `src/motion.ts` and handed to CSS as a `linear()` easing (733 ms,
  ~1% overshoot; cubic-bezier fallback). Replays for each gap; off with reduced motion.
- Browser check: bars and meters measured mid-animation and settled (meters staggered, first
  year bar at 1.011× then 1.0); build passes; desktop app rebuilt and relaunched.

## 2026-10-29 — Panel polish: no tab scrollbar, flicker-in marks, page crossfade

- Removed the vertical scrollbar on the tab row: the active-tab underline sat 1 px below the
  row, which overflowed a scrolling container; it now sits inside and the row never scrolls
  vertically.
- Accent marks on the circle (QUERY crosshair, result ×, green region ring) join the dot
  flicker, so they flicker in whenever they reappear, e.g. on returning to Search.
- Search: placeholder "/ to Search"; the line drawing in the empty state is gone.
- Page changes inside the working panel (e.g. a gap opened from Gaps, and Back) fade the old
  page out (150 ms), swap it and fade the new one in (220 ms), scrolled to the top. Visited pages
  stay mounted, so search results and filters survive. Off with reduced motion.
- Browser check with the stub backend: tab row 0 px scrollbar, placeholder text, crossfade class
  sequence in → out → in with the detail swapped in; marks present after returning to Search.
  Build passes; desktop app rebuilt and relaunched.

## Next
1. Agent interface, roadmap A1–A3 in [docs/target-product.md](docs/target-product.md): shared
   orchestration crate, `invectus mcp` read tools, then write tools with single-writer handling.
2. Decision provenance (A4) before agents can suggest decisions.
3. Owner's UI work in progress: map display toggles, light mode, font size; the idle 84%
   indexing preview is intentional for tuning and must be removed before release.
4. The gap ranking still loses to the baselines on the full backtest; adjudicate a sample of
   verdicts and gaps before trusting thresholds and weights.
5. Run the three-OS CI once the repository is public (AGENTS.md: no CI while private).


## 2026-10-27 — Multimodal baseline, runtime evidence and preparatory contracts

Scope: review and execute `docs/multimodal-implementation-plan.md` through feasible evidence
work. **Image retrieval is not implemented in the product.** The human-judgment, platform and
packaging gates are still open. Details, reproducible commands and limitations:
[`docs/multimodal/README.md`](docs/multimodal/README.md).

### Baseline and preservation
- Read this ledger, SPEC, parent repository instructions and relevant engine/UI/CI code. There
  is no local AGENTS/CLAUDE file. Started at `b5f6d71`; only the supplied plan was untracked.
  No overlapping benchmark/ablation processes were running. Full benchmark and ablation are
  complete and recorded; their gate failure is unchanged. Corrected the obsolete Next item.
- Unchanged workspace baseline: **75 tests passed**, including actual BGE and CLI checks.
  No full-benchmark query, clustering, backtest or re-embedding was performed.
- Captured a metadata guard of benchmark projects/results, existing text models and reference
  DB/vectors. Repeated checks match. This is path/size/mtime evidence, not a resolution of the
  older unexplained reference fingerprint. Existing data/index/calibration files were not written.
- BGE default, Nomic availability, all legacy identity serialization/IDs, text encoder/search,
  DuckDB schema, Lance metadata and calibration behavior remain unchanged.

### Phase 0/1 evidence
- Added a frozen, reproducible 12-source-group CC0/public-domain diagnostic set, with source
  commit, license evidence, checksums, captions/proposed relevance, near duplicates and resized
  image queries. Development/holdout split is by source group (8/4), with all groups serving as
  distractors. **Expectations are agent-authored and not human adjudicated.** An independent,
  harder human-reviewed retrieval set and untouched calibration split are still required.
  Also inspected official Winoground (gated; unauthenticated parquet request HTTP 401;
  research-only access agreement not accepted) and MMEB-eval (ungated labels, separate 7.13 GB
  image ZIP; no bulk download on this low-headroom host). The latter is a route for a bounded
  subset after checking constituent source terms; this is not a claim that no public labels exist.
- Verified actual model repository file lists, configs, revisions, small-file Git blob hashes
  and weights SHA-256. All spike downloads were explicit, task-local and isolated from the
  installed text model store. Inference used verified local paths and offline library flags.
- **EmbeddingGemma 2**: released `google/embeddinggemma-2@914f7f89…`, ungated Apache-2.0 card,
  safetensors and multimodal processor exist; **no ONNX artifact in that revision**. Actual
  text/image inference ran with torch 2.14.1 / transformers 5.21.0 / sentence-transformers 6.2.0,
  CPU FP32, audio encoder omitted. The processor also required torchvision 0.29.1; installed
  it and soundfile 0.14.0 in the isolated venv. No BF16/FP16/accelerator measurements. Model
  docs reject FP16. Video/audio only inspected, not executed or evaluated.
- **Alternative: SigLIP 2 base patch16 224**. Official checkpoint `75de2d55…` (Apache-2.0) ran
  through PyTorch; export `onnx-community/…@ba1f3b08…` has verified FP32/INT8 text and vision
  ONNX artifacts, both executed via ORT 1.31.1 CPU. Export card lacks its own license field/file;
  distribution must preserve upstream terms/notices and verify export provenance.
- The same short-text diagnostics also ran existing BGE and Nomic (12 documents each; no
  benchmark re-embedding). All six configurations had hit@1/recall@5/MRR 1.0 on applicable
  diagnostic tasks in both source splits. These ceiling results **do not choose a model**.
- M4 Pro, 24 GiB, macOS 27.0.1 arm64, 8 CPU threads, batch 1, warm query median of 5 repeats:

  | configuration | text query ms | image query ms | artifacts MB | peak process RSS MB |
  |---|---:|---:|---:|---:|
  | BGE ORT FP32 | 1.98 | — | 133.8 | 429.8 |
  | Nomic ORT FP32 | 4.90 | — | 548.0 | 1369.1 |
  | EmbeddingGemma 2 PyTorch FP32 | 27.61 | 694.85 | 1525.8 | 4571.7 |
  | SigLIP 2 PyTorch FP32 | 17.17 | 32.82 | 1539.5 | 2492.2 |
  | SigLIP 2 ORT FP32 | 8.16 | 33.19 | 1539.9 | 4536.5 |
  | SigLIP 2 ORT INT8 | 7.28 | 19.60 | 416.6 | 1910.6 |

  Inputs were decoded before timing; preprocessing/encoding included. RSS includes frameworks
  full-file checksum verification and load peak, not just weights. Separate downloads could overlap disk/network I/O; no other
  benchmark/compiler competed with the timed models. These are tiny smoke measurements, not
  a 100k benchmark, p95 latency or long-text performance comparison. Python adds ~1 GB here.
- FP32 export parity vs PyTorch on 12 original images/queries: min cosine 1.000000 at report
  precision, maximum cross-modal score delta 3.19e-7, text→image top-1 agreement 1.0. INT8:
  min cosine **0.99556 text / 0.96141 image**, score delta 0.01406; top-1 unchanged only on
  these easy cases. INT8 cannot reuse FP32 vectors or calibration.
- Native Rust `ort 2.0.0-rc.15` CPU preflight executed both SigLIP towers on a verified fixture
  and matched Python ORT (cosine ≥ 0.999999). Actual outputs are `last_hidden_state` and
  `pooler_output`; use the projected pooled output, not the existing text pooling path. This
  is a one-source native preflight with Python preprocessing, not native preprocessing or
  complete image retrieval. Raw JSON and example code are retained.
- Verified actual CPython 3.12 direct-dependency wheel filenames/hashes for macOS arm64,
  Windows x86_64 and Linux x86_64. **Only macOS executed**. Added a manual three-OS CPU/native
  probe workflow; nothing pushed or dispatched. Other architectures, accelerators and
  multimodal installers remain unverified.

### Independent foundation work (phase 2 preparation only)
- Added typed text/local-media inputs, explicit supported modalities, a multimodal interface
  and a legacy `TextAdapter`. The adapter forwards text/prefix mode unchanged and rejects an
  entire mixed media batch before inference. No existing command uses a new model by default.
- Added a separate versioned media identity envelope: pinned artifact manifest, processor/
  modality policy, legacy text policy, dimensions/precision/runtime. Sorted maps make hashes
  deterministic; byte-content hashes and query/document mode govern per-input invalidation.
  It deliberately cannot be registered in the current legacy text tables/calibration APIs.
- Added four meaningful compatibility/contract tests, including exact BGE/Nomic/imported-Nomic
  IDs read from the old small projects, serialization and temporary-project reopening, policy/
  artifact/runtime hash changes and unsupported-media rejection before model work.
- No persisted media schema or source-copy policy implementation, safe image decoder,
  thumbnail pipeline, resumable media job, native preprocessor or desktop/CLI image feature
  was added. Those remain gated implementation work, not silently claimed complete.

### Decisions, checks and next steps
- Updated the plan with the completed benchmark baseline, verified artifact/runtime corrections,
  phase statuses and material decisions. SigLIP's direct native path and speed justify further
  evaluation; EmbeddingGemma's broader advertised capabilities do not justify a default swap.
  There is **no production model winner** on these unreviewed ceiling-level quality results.
- **79 workspace tests passed**, including real-model/CLI checks; 5 model-free Python evaluation/
  installer/offline tests passed. Rust fmt, clippy across all workspace targets, Python Ruff
  lint/format and frontend build passed. macOS `tauri build --no-bundle --ci` passed (7m14s
  initial rebuild); final incremental rebuild also passed. The known upstream Apple
  linker `__eh_frame` size warning remains. No Windows/Linux build or execution was claimed.
- Next: human-adjudicate realistic semantic/visual retrieval and hard negatives; run the three-OS
  probes; choose runtime/precision with that evidence; resolve native preprocessing and license/
  notice packaging. Then add versioned media storage, bounded import/EXIF/alpha handling,
  resumable isolated indexing and CLI+desktop image retrieval with original inspection.
- **Phases 3–5 stay gated.** No image absence verdict or multimodal calibration/gaps enabled.
  The existing text calibration key lacks modality pairs; any media calibration needs an
  explicit collection/space/modality-pair/preprocessing scope and separate held-out validation.
  Video/audio need actual decoding and labeled temporal/audio evaluation before implementation.

- Fixture regeneration with network disabled and cached originals reproduced every frozen
  checksum/expectation. The task's Python bytecode caches are ignored locally. Benchmark,
  reference and text-model metadata guards continue to match after all tests/builds.

## 2026-10-29 — Sidebar width and click-to-zoom polish

- Reduced the desktop sidebar from 325 px to 285 px, preserving the narrow-window layout.
- Grouped map-item clicks now ease into the existing 2.5× zoom over 360 ms, anchored at
  the clicked location. Pan, wheel, pinch, reset, resize and unmount cancel pending motion;
  reduced-motion users receive the immediate view update.
- Frontend TypeScript/Vite build and macOS release `tauri build --no-bundle --ci` passed.
  The existing Apple linker `__eh_frame` warning remains. Relaunched the rebuilt desktop
  binary with the existing dev model store. Visual verification is left to the owner as
  requested; no frame-rate measurement or Windows/Linux verification claimed. No CI run.

## 2026-10-29 — Project tabs and sidebar icons

- Added persistent Explore, Search, Gaps and Collections tabs below the project header,
  with muted labels, a subtle active background and arrow/Home/End keyboard navigation.
  Projects and Settings remain in the sidebar; import and indexing controls live under
  Collections. Explore combines the project summary with the existing map controls.
- Standardized sidebar/project-search icons using locally bundled Lucide React SVGs
  (18 px, 1.5 px stroke); added a subtle sidebar selection background.
- Tabs have separate navigation histories and map highlights. Visited panels remain mounted,
  preserving search results, gap filters and collection controls across tab changes; the
  shared map keeps its zoom. Search and gap state reset when collection/embedding scope
  changes. Search results also survive opening an item and returning with Back.
- Five local navigation tests passed (`npm run test:navigation`, Node 25.9.0): destination
  routing, cross-tab history preservation, detail return paths, root replacement and keyboard
  wrap/Home/End behavior. Frontend build and macOS release `tauri build --no-bundle --ci`
  passed; the existing upstream Apple linker `__eh_frame` warning remains. Relaunched the
  rebuilt binary with the existing dev model store. Interactive visual verification is left
  to the owner as requested; Windows/Linux remain unverified. No GitHub CI dispatched.
- Decisions and spec conflicts: this changes navigation grouping while retaining the map,
  search, import, collections and gap evidence controls; no new spec conflict identified.

## 2026-10-29 — Approved terminal-inspired visual pass

- Applied all seven owner-approved visual proposals: 24 px page titles; 14 px body and
  semibold section headings with 1.5 body line height; sentence-case labels; warm charcoal
  surfaces (#141414/#202020/#292929), off-white text and warm gray metadata; continuous
  result/collection lists; tighter spacing; and restrained controls with mint primary actions.
- Main horizontal insets are now 32 px, panels 16 px, and result/collection rows use 12 px
  vertical padding. Search and gap results share one bordered surface with separators and
  full-row hover/focus backgrounds. Titles wrap with metadata beneath them. Summary/evidence
  cards remain distinct; project summary cards are smaller. Corners remain small (4 px).
- Project/index summary indicators now include visible Indexed/Not indexed text. Updated the
  canvas ink/background to the same palette. Existing status wording accompanies color in
  collection tables; keyboard focus indicators remain visible.
- Frontend build, five existing navigation tests and macOS release
  `tauri build --no-bundle --ci` passed. Calculated contrast for the principal token pairs:
  body/panel 13.60:1, secondary/selection 5.93:1, primary button 10.95:1. These are palette
  calculations, not a full rendered accessibility audit. Existing upstream Apple linker
  `__eh_frame` warning remains. Relaunched the rebuilt binary with the dev model store;
  interactive visual review is left to the owner. Windows/Linux remain unverified; no CI run.
- Decisions and spec conflicts: the owner's approved palette/typography replaces the earlier
  Angrove/Figma-derived styling. No feature or engine behavior changed; no new spec conflict.

## 2026-10-29 — Sidebar project list and panel-contained workspace

- Added a persistent, searchable recent-project list to the sidebar with current-project
  highlighting, full-name/path tooltips, empty/filter states and independent list scrolling.
  New/Open project actions sit above it; Settings and job status stay at the bottom. Project
  switches and opens respect running project jobs and pending open operations.
- Read Angrove's `MacSidebarLayout.minimumWidth` (280 pt) and `BrandTypography.pointSize`
  (13 pt for desktop sizes up to 18) from the canonical checkout without modifying it.
  Matched the sidebar to 280 CSS px and 13 px regular labels, with 16 px icons. Reduced
  semibold/bold interface weights to medium, retaining regular sidebar/tab labels.
- Moved the project title, path, network switch, warnings and tabs into a bordered right-hand
  working panel. The panel has its own scrolling body and takes 54% of the workspace, bounded
  at 400–620 px; the map remains alongside it at normal desktop widths. Below 1040 px window
  width, the workspace stacks and scrolls. Existing tab histories/state and keyboard behavior
  are retained. The Projects overview remains accessible from the sidebar.
- Frontend build, five existing navigation tests, diff checks and macOS release
  `tauri build --no-bundle --ci` passed. The known upstream Apple linker `__eh_frame` warning
  remains. Relaunched the rebuilt app with the existing dev model store. Visual review is left
  to the owner; no live responsive-layout verification or Windows/Linux validation claimed.
  No GitHub CI dispatched.
- Decisions and spec conflicts: the owner requested the right panel as the primary project
  workspace and the sidebar as the project switcher; no new spec conflict identified.

## 2026-10-29 — Restore project heading above the workspace

- Moved the project name and path from the right panel back to the top-level workspace
  header, restoring the existing 24 px title and 13 px path styles. Tabs and project controls
  remain inside the right panel. Removed the superseded console title/path styles.
- Frontend TypeScript/Vite build and diff checks passed. Visual review remains with the owner;
  Windows/Linux remain unverified. No GitHub CI dispatched.
- macOS release rebuild passed with the existing upstream `__eh_frame` linker warning;
  relaunched the rebuilt binary with the existing dev model store.

## 2026-10-29 — Restore Offline switch to the top header

- Moved the existing Offline/Network on switch beside the project heading at the top of the
  workspace. Removed the now-empty console header and its CSS; tabs lead the right panel.
  Network-switch behavior is unchanged.
- Frontend and macOS release builds passed, as did diff checks. The known upstream Apple
  `__eh_frame` linker warning remains. Relaunched the rebuilt app with the dev model store.
  Visual review remains with the owner; Windows/Linux unverified. No GitHub CI dispatched.

## 2026-10-29 — Search panel from the revised Figma frame

- Implemented Figma file JBM9BM0Zo6JHgj7NCjxcRt, node 1227:166 using the design-to-code
  context and screenshot. Kept the existing warm palette, desktop text sizes and lighter
  weights as requested. Search now has an edge-to-edge input/filter row, an inline scoped
  assessment, and flat similarity rankings with metadata beneath each title. Calibration,
  probability, search scope, exhaustive/approximate status and notes remain available under
  Assessment details. Empty results and stale calibration do not receive opportunity copy.
- Removed Explore and its obsolete page route. Opening a project now starts at Search;
  existing map-highlighting actions keep the current evidence panel visible. Map-display
  options await the owner's map-side icon design; no replacement icon was invented.
- Moved the native filter into the right side of the search bar. All items is the default;
  Coverage/Demand remain available when those roles exist. Removed the silent all-items
  fallback for empty roles. Changing the filter after a search reruns it; filters are disabled
  while searching. Added the displayed slash-to-focus shortcut without intercepting typing
  inside inputs, editors or selects. Search results/filter state survives tab changes.
- Added a shared 3×3 loading matrix: one full-opacity square, eight at 50%, cycling down
  the left column, up the middle, down the right (row-major order 0,3,6,7,4,1,2,5,8).
  CSS uses 120 ms per square / 1080 ms per loop and supports reduced motion. The tab-row
  indicator uses actual active local job progress when available. Per the owner's request,
  idle animation remains permanently visible with an 84% preview and matching bar, identified
  as preview in its tooltip/accessibility label. Removing the explicit preview prop restores
  automatic hiding when idle. No indexing job was started for the preview.
- Downloaded the frame's chevron and divider SVGs unchanged into src/assets/figma, verified
  non-empty roots/dimensions and their rendered slots, and forced local file emission to
  respect the existing desktop image CSP. No temporary remote asset URLs remain in runtime
  code. Applied the same flat surfaces, spacing and typography to Gaps and Collections.
- Fourteen local UI tests passed: three-tab routing/keyboard histories, loader order and
  preview/real/indeterminate states, scoped assessment copy, and empty/stale safeguards.
  Frontend TypeScript/Vite and macOS release build passed; the existing upstream Apple
  linker __eh_frame warning remains. No GitHub CI dispatched; Windows/Linux unverified.
- Visually checked an isolated browser fixture at the desktop window width and a wider
  layout: search/filter behavior, tab-state preservation, Gaps/Collections controls, local
  SVG geometry, one bright/eight half-opacity loader squares changing over time, and an
  84% bar. Saved the sample-data panel screenshot under .spike-data/ui-preview/ (ignored).
  Closed the preview and stopped its localhost server; relaunched the rebuilt desktop app
  with the existing dev model store. These fixture checks are not new real-model benchmarks.
- Decisions and spec conflicts: adopted the owner's revised navigation and positive but
  collection-scoped assessment wording. The percentage/animation is temporarily previewed
  while idle by explicit request; production idle hiding remains a documented follow-up
  once the owner finishes tuning. No new engine or absence-claim scope was introduced.

## 2026-10-29 — Loading-matrix fade trail

- Each matrix square still lights at the existing 120 ms snake cadence, then fades linearly
  from full to half opacity over 500 ms instead of switching instantly. The 1080 ms cycle
  and reduced-motion fallback are unchanged; fading squares form a short trail.
- Frontend TypeScript/Vite build and diff checks passed. No native rebuild or relaunch,
  as requested. Live animation review remains pending the owner's next launch.

## 2026-10-29 — Narrower sidebar

- Reduced sidebar width from 280 px to 240 px and horizontal padding from 24 px to 16 px;
  retained 24 px vertical padding and 13 px regular sidebar text. Updated the old width comment.
- Frontend TypeScript/Vite build and diff checks passed. The requested desktop rebuild also
  includes the previously saved 500 ms matrix fade. Visual review remains with the owner.
- macOS release rebuild passed with the existing upstream Apple __eh_frame linker warning.
  Launched the rebuilt app with the existing dev model store (no old app process was running).
  No GitHub CI dispatched; Windows/Linux remain unverified.

## 2026-10-29 — Quiet search input focus

- Removed the project search input's focus outline so typing shows the caret without a
  surrounding highlight. Other controls retain their existing keyboard focus indicators.
- Frontend TypeScript/Vite build and diff checks passed. No native rebuild or relaunch,
  as requested; visual review awaits the owner's next launch.

## 2026-10-29 — Clockwise 3×2 loading matrix

- Changed the shared loader to three columns and two rows, looping clockwise across the
  top row and back across the bottom (row-major indices 0,1,2,5,4,3). Renamed the former
  snake-specific component/styles to MatrixLoader; cell count derives from the traversal.
- Kept the 120 ms step interval and 500 ms fade to half opacity; adjusted the cycle to
  720 ms. Reduced-motion fallback remains. Updated the traversal regression test.
- All 14 UI tests, frontend TypeScript/Vite build and diff checks passed. No native rebuild
  or relaunch; live animation review remains pending the owner's next launch.

## 2026-10-29 — Staggered flicker entrance for map dots

- New visible map squares and loading placeholders receive an independent uniform random
  delay of 0–250 ms followed by a 150 ms stepped-opacity flicker, then settle at their usual
  opacity. Surviving squares retain timing across redraws; off-screen squares are pruned,
  and reappearing/new-layout squares get a fresh entrance. Existing placeholder motion stays;
  real squares no longer depend on the old global fade to become fully visible.
- Cached screen grouping/merging separately from animation ticks so the entrance frames
  reuse visible cells rather than rescanning every stored point. Animation work stops after
  the last entrance completes, cancels on unmount, and is skipped for reduced-motion users.
  Appearance state is bounded by current visible squares/placeholders, not collection size.
- All 18 local UI tests passed, including exact delay/flicker boundaries, independent random
  timing, redraw stability and pruning/reappearance. Frontend TypeScript/Vite build and diff
  checks passed. No performance/framerate measurement or live animation inspection claimed.
- macOS release rebuild passed with the existing upstream Apple __eh_frame linker warning;
  relaunched the rebuilt app with the dev model store as requested. This also includes the
  pending quiet search focus and clockwise 3×2 loader changes. Visual review remains with
  the owner; Windows/Linux unverified. No GitHub CI dispatched.

## 2026-10-29 — Detail list line height and Back arrow

- Explicitly set plain-list items (including Central items) to line-height 1.5, matching
  the existing root default. Replaced the Back button's small SVG image with a 16 px
  inline left arrow that inherits the button color, retaining its 8 px label spacing.
- Frontend TypeScript/Vite build and diff checks passed. macOS release rebuild passed
  with the existing upstream Apple __eh_frame linker warning; relaunched the rebuilt app
  with the dev model store and confirmed its process running. This also includes the
  pending taller 2×3 loader. Visual review remains with the owner; Windows/Linux unverified.
  No GitHub CI dispatched.

## 2026-10-29 — Taller 2×3 loading matrix

- Changed the shared loading matrix to two columns and three rows. Clockwise traversal is
  now row-major indices 0,1,3,5,4,2; six-cell timing and the 500 ms fade are unchanged.
- All 18 UI tests, frontend TypeScript/Vite build and diff checks passed. No native rebuild
  or relaunch, as requested; live visual review awaits the next launch.
