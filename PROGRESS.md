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
1. Agent interface (A1–A8 built 2026-11-03/04 except the small-model evaluation, which needs
   an approved download). App UI still to design with the owner: A4 suggestions (built, to
   refine), A6 Settings → Connect an AI agent, A7 live agent activity. Real-client checks
   (Claude Code / Desktop / Cursor, `.mcpb` install) need the owner's go-ahead.
2. Decide the agent paraphrase guidance (see the 2026-11-04 A5 entry): agents that follow
   "in your own words" literally drop technical terms and calibrate worse than titles.
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

## 2026-10-29 — App bundle relaunch correction

- The earlier standalone background process had exited by the next check; the initial
  process check did not establish a persistent visible window. Built the macOS app bundle
  successfully, launched it through Launch Services with the dev model store, and confirmed
  its bundle process persisted. Closed the duplicate standalone process. UI inspection was
  unavailable because Computer Use was not approved; window visibility remains unverified.

## 2026-10-29 — Uniform Projects empty-state padding and original launch profile

- Restored 24 px padding on all four sides of the bordered Projects empty state; the
  later generic empty-state rule had overridden its horizontal padding to zero.
- Read existing WebKit project history without modifying it: the standalone executable's
  `invectus-app` profile contains demo.invectus (10,000 items); the macOS bundle uses
  a separate `app.invectus.desktop` profile. The empty recent list was a profile difference.
- Frontend and macOS release builds passed, with the existing upstream Apple __eh_frame
  linker warning. Launched the updated standalone binary with the dev model store in a
  retained terminal session. Visual window/content verification remains with the owner.
  No CI dispatched; no reference data changed.

## 2026-10-29 — Saved gap loading and staggered list entrance

- Saved gap reads now retry on entry to the Gaps tab. Previously the scope/region effect
  was the only automatic read, and a rejected read silently cleared the results. Loading
  failures now show their error and a saved-results retry button, retaining prior rows.
  Request ordering prevents older reads overwriting newer results after a scope change.
- Explicit post-scan reloads read persisted gaps without relying on the pre-refresh
  `scanned` flag captured by the scan callback. Successful empty scans and empty type
  filters now have distinct messages. A first scan remains an explicit action.
- Gap rows flicker in over 300 ms while translating vertically from -24 px to zero,
  starting at 0 ms with 150 ms added per subsequent row. Entrances replay on tab/detail
  return and filter changes. Reduced motion skips animation; keyboard-focused rows
  become visible immediately.
- Frontend TypeScript/Vite build, all 18 existing UI tests and diff checks passed.
  Original missing-list behavior and new animation have not been reproduced/verified
  through the live UI; the error/retry path makes further failures visible.
- macOS release rebuild passed with the existing upstream Apple __eh_frame linker
  warning. Restarted the standalone app with its original project history and dev model
  store; its process is running. No CI dispatched; Windows/Linux remain unverified.

## 2026-10-29 — Horizontal gap-row entrance

- Removed the gap-row opacity flicker and changed the entrance from vertical movement
  to translateX(-24px) → translateX(0). Kept the 300 ms ease-out duration, 150 ms
  per-row stagger, reduced-motion fallback and immediate keyboard-focus visibility.
- Frontend TypeScript/Vite build and diff checks passed.
- macOS release rebuild passed with the existing upstream Apple __eh_frame linker
  warning. Relaunched the standalone app with its existing project history and dev model
  store. Live visual review remains with the owner; no GitHub CI dispatched.

## 2026-10-29 — Gap entrance fade, offset source lines and map focus

- Gap-row slide now fades smoothly from opacity zero to one over the existing 300 ms,
  with the 150 ms per-row stagger unchanged. Result lists and their console no longer
  use overflow:hidden. The menu retains vertical scrolling with a 24 px horizontal
  gutter, including narrow layouts, to accommodate the -24 px slide.
- Shared source links use a separate 1 px bottom border with 3 px padding below the
  text, replacing text-decoration. Cloned inline decorations preserve the line beneath
  every wrapped segment. Existing source opening behavior is unchanged.
- Show on map previously reapplied the region mark already set on detail entry. It now
  sends a repeatable explicit request that centres and fits that region on the canvas,
  clears hover/active zoom motion and focuses/scrolls the map into view. The detail
  remains open. Requests are consumed once and cleared on collection/map changes, so
  subsequent resize/data redraws do not undo manual panning. Coordinates remain display-only.
- All 20 UI tests passed, including new off-centre region centring, outline fitting and
  zoom-bound checks. Frontend build and diff checks passed. Live layout, wrapped-link
  appearance and button interaction remain unverified through the UI.
- macOS release rebuild passed with the existing upstream Apple __eh_frame linker
  warning. Relaunched the standalone app using its existing project history and dev
  model store. No CI dispatched; Windows/Linux remain unverified.

## 2026-10-29 — Shorter gap-row stagger and slide

- Reduced the gap-row entrance stagger to 50 ms per row and the horizontal starting
  offset to -8 px. The smooth fade and 300 ms ease-out duration remain unchanged.
- Frontend and macOS release builds and diff checks passed, with the existing upstream
  Apple __eh_frame linker warning. Relaunched the standalone app with its existing
  project history and dev model store. Live visual review remains with the owner.
  No CI dispatched.

## 2026-10-29 — Back returns directly to Search

- The visible panel Back button now switches directly to the existing Search root,
  preserving its query and mounted results instead of stepping through selected points.
  It clears detail history in Search and resets the originating tab to its list/root.
  Import wizard step Back, cancellation and completion keep their existing behavior.
- All 23 UI tests passed, including repeated point selections, returning from Gaps,
  preserving the Search query and creating Search when not previously visited.
  Frontend TypeScript/Vite build and diff checks passed; live navigation review remains
  with the owner.
- macOS release rebuild passed with the existing upstream Apple __eh_frame linker
  warning. Relaunched the standalone app with its existing project history and dev
  model store. No GitHub CI dispatched.

## 2026-10-29 — Gap-map emphasis

- While the Gaps tab is active (including its detail views), real, merged and placeholder
  map squares fade to an absolute 25% opacity over 250 ms. Leaving restores their usual
  opacity over the same interval; interrupted fades continue from the current value.
  Square grouping remains memoized separately from animation frames.
- Visible gap rings and their labels now join the existing DotFlicker tracker on Gaps
  entry: independent uniform 0–250 ms delays followed by the existing 150 ms flicker.
  Leaving prunes their entrance keys so returning replays it; camera redraws retain
  timings for surviving rings. The existing top-12 marker limit is unchanged.
- Gap-ring stroke is now 4 CSS px; edge visibility padding accounts for its outer radius.
  Reduced motion skips both flicker and opacity transitions.
- Frontend TypeScript/Vite build, all 23 existing UI tests and diff checks passed.
  Live animation/appearance and frame-rate behavior remain unverified.
- macOS release rebuild passed with the existing upstream Apple __eh_frame linker
  warning. Relaunched the standalone app with its existing project history and dev
  model store. No GitHub CI dispatched; Windows/Linux remain unverified.

## 2026-10-29 — Fainter squares and selected gap rings

- Lowered Gaps-tab square opacity from 25% to an absolute 15%, retaining the 250 ms fade
  and restoration on leaving. Random ring entrance timing is unchanged.
- Gap rings now default to 1 CSS px. The currently open gap's ring transitions to 4 px
  over 250 ms and uses the existing green accent (#6edcaa), as does its rank label.
  Selection comes from the active detail page, including a gap opened from Search;
  returning to a list/Search removes selection and transitions the stroke back to 1 px.
  Other rings stay at 1 px. Reduced motion applies the target width immediately.
- A selected gap outside the top 12 is additionally rendered and hit-tested, with its
  original rank code, so opening a lower-ranked gap can still show its selected ring.
- All 23 existing UI tests passed; frontend TypeScript/Vite build and diff checks passed.
  Live selected-ring appearance and animation remain unverified through the UI.
- macOS release rebuild passed with the existing upstream Apple __eh_frame linker
  warning. Relaunched the standalone app with its existing project history and dev
  model store. No GitHub CI dispatched.

## 2026-10-29 — All gap rings thicken on Gaps

- Corrected the previous interpretation: every visible gap ring transitions to 4 CSS px
  while the Gaps tab is active, including its detail views. Outside Gaps, rings return
  to 1 px except for a selected gap, which stays at 4 px. The selected ring remains
  green; square opacity remains 15% on Gaps. Uses the existing 250 ms transition and
  reduced-motion fallback.

## 2026-10-29 — Remove Reset view button

- Removed the map's conditional Reset view button and both of its CSS rules. It no
  longer appears after panning, zooming or Show on map. Automatic map initialization
  and region focus remain unchanged.
- Frontend and macOS release builds and diff checks passed for both this removal and
  the all-rings-on-Gaps correction, with the existing upstream Apple __eh_frame linker
  warning. Relaunched the standalone app with its existing project history and dev
  model store. Live visual review remains with the owner; no CI dispatched.

## 2026-10-29 — Content left, circle right

- Reordered the project workspace and new-project setup so content comes first and the
  circle second: content on the left and circle on the right on desktop, content above
  the circle in the existing narrow stacked layout. DOM and keyboard traversal match
  that visual order. Existing sizing and scrolling rules are unchanged.
- Updated the import placeholder to reference the import panel without its former
  right-pointing direction.
- Frontend and macOS release builds and diff checks passed, with the existing upstream
  Apple __eh_frame linker warning. Relaunched the standalone app with its existing
  project history and dev model store. Live layout review remains with the owner;
  no GitHub CI dispatched.

## 2026-10-29 — Pan and zoom to a selected gap

- Opening a gap now automatically focuses its map location, retaining the green selected
  ring and 4 px strokes on Gaps. Coverage/stale gaps focus their region; bridge gaps
  focus the actual midpoint and fit both region outlines. If results/map arrive later,
  focusing waits until the selected gap's geometry is available.
- Camera motion pans the world-space view centre and zooms geometrically with smoothstep
  easing over 450 ms. A new focus or pointer/scroll gesture cancels prior zoom motion;
  reduced motion applies the final view immediately. Show on map uses the same animation.
  Selection focus is consumed once per gap/run, preserving subsequent manual panning.
- All 25 UI tests passed, including bridge midpoint/bounds and camera endpoint/centre
  interpolation tests. Frontend TypeScript/Vite build and diff checks passed.
  Live interaction and frame-rate behavior remain unverified through the UI.
- macOS release rebuild passed with the existing upstream Apple __eh_frame linker
  warning. Relaunched the standalone app with its existing project history and dev
  model store. No GitHub CI dispatched.

## 2026-10-29 — More breathing room for Central items

- Increased Central items source-list line height from 1.5 to 1.8 and added 8 px between
  consecutive items, leaving room for the offset source-link borders and inline metadata.
  Scoped to that source list rather than all plain lists.
- Frontend and macOS release builds and diff checks passed, with the existing upstream
  Apple __eh_frame linker warning. Relaunched the standalone app with its existing
  project history and dev model store. Live spacing review remains with the owner;
  no GitHub CI dispatched.

## 2026-10-29 — Source-link hover dimming

- Shared source links now transition to 75% opacity on hover over 150 ms, returning to
  full opacity over the same duration on pointer exit. The text and offset border dim
  together. Reduced motion applies the opacity change immediately.
- Frontend and macOS release builds and diff checks passed, with the existing upstream
  Apple __eh_frame linker warning. Relaunched the standalone app with its existing
  project history and dev model store. Live hover review remains with the owner;
  no GitHub CI dispatched.

## 2026-10-29 — Header Back to Projects

- Replaced the static Project eyebrow above the project name with a Back to Projects
  button and a right-pointing arrow after the label. It opens the existing Projects
  route and retains the eyebrow typography, with hover and keyboard-focus feedback.
- Frontend and macOS release builds and diff checks passed, with the existing upstream
  Apple __eh_frame linker warning. Relaunched the standalone app with its existing
  project history and dev model store. Live appearance/click review remains with the
  owner; no GitHub CI dispatched.

## 2026-10-29 — Open workspace with vertical divider

- Removed the content console's enclosing border, corner radius and background, plus
  the full-width line beneath the toolbar. Tabs remain at the top with their active
  underline; the content keeps its existing independent scroll area.
- Added a 1 px vertical divider midway through the 24 px content/map gutter. Its 16 px
  inset within the console plus the desktop stage's 32 px padding leaves 48 px from
  the top and bottom edges of the workspace below the project header. The divider is
  decorative and ignores pointer input; it is hidden in the existing stacked layout
  at viewport widths of 1040 px and below.
- Frontend and macOS release builds and diff checks passed, with the existing upstream
  Apple __eh_frame linker warning. Relaunched the standalone app with its existing
  project history and dev model store. Live layout review remains with the owner;
  no GitHub CI dispatched.

## 2026-10-29 — Resizable content/map split

- Replaced the decorative divider with a draggable, focusable separator. Pointer capture
  keeps resizing active across both panes; release/cancel/lost capture ends the drag.
  A 24 px hit area surrounds the 1 px line; its desktop endpoints remain 48 px from
  the workspace top and bottom. Hover/focus/drag highlight the divider.
- Content and map each have a 400 px minimum in the side-by-side layout; removed the
  content's former 740 px maximum. The preferred ratio survives window resizes during
  the mounted workspace, with both pane widths clamped to their minima. Layout now
  uses measured workspace width rather than a viewport breakpoint, stacking when
  400 + 24 + 400 px cannot fit. The narrow stacked layout remains responsive.
- Left/right arrows resize by 16 px (Shift: 64 px); Home/End move to the pane limits.
  The separator exposes orientation and current/min/max sizes to accessibility tools.
- All 28 UI tests passed, including split minima, exact stacking threshold and window
  resizing constraints. Frontend TypeScript/Vite build and diff checks passed.
  Live dragging, keyboard interaction and layout remain unverified through the UI.
- macOS release rebuild passed with the existing upstream Apple __eh_frame linker
  warning. Relaunched the standalone app with its existing project history and dev
  model store. No GitHub CI dispatched.

## 2026-10-29 — Leading left arrow for Back to Projects

- Moved the header navigation arrow before Back to Projects and changed it to point
  left. Navigation behavior and existing spacing/typography are unchanged.
- Frontend and macOS release builds and diff checks passed, with the existing upstream
  Apple __eh_frame linker warning. Relaunched the standalone app with its existing
  project history and dev model store. Live appearance review remains with the owner;
  no GitHub CI dispatched.

## 2026-10-29 — Back from a gap returns to the gap list

- The panel Back button on a gap detail now returns to the Gaps list (dropping any
  stacked gap details) instead of switching to Search. Item details still return to the
  existing Search. Helper renamed `returnFromDetail`; navigation test updated.
- UI tests (28), type check and frontend build pass. Not rebuilt or relaunched as a
  macOS app, at the owner's request; unchecked in the native window. No CI dispatched.

## 2026-10-29 — Uniform tab padding

- Removed the project workspace's inherited stage padding and both toolbar padding
  declarations. Each tab now owns a uniform 24 px of padding on every side, replacing
  the former 16/14/14 px tab padding layered over workspace and toolbar insets.
- Aligned active-tab underline endpoints to the new horizontal padding and left a
  matching 24 px right inset for the indexing indicator. Content scroll-area padding
  remains independent. New-project setup retains its existing stage padding.
- Changed divider margins from 16 px to 48 px so removing workspace padding preserves
  the requested 48 px endpoints. Pane sizing continues to measure actual available space.
- Frontend and macOS release builds and diff checks passed, with the existing upstream
  Apple __eh_frame linker warning. Relaunched the standalone app with its existing
  project history and dev model store. Live spacing review remains with the owner;
  no GitHub CI dispatched.

## 2026-10-29 — Closer animated tab underline

- Replaced per-tab active pseudo-elements with one shared underline, positioned 6 px
  below the active tab's text line box rather than at the bottom of its 24 px padding.
  The tab hit areas and equal padding are unchanged.
- The underline animates horizontal position and width over 200 ms when selection
  changes, including keyboard and programmatic navigation. Measurements refresh on
  active-tab changes and ResizeObserver updates to the tab strip/buttons, including
  resizing and font-width changes. It moves with the scrollable tab strip and ignores
  pointer input; reduced motion switches position immediately.
- All 28 existing UI tests, frontend and macOS release builds and diff checks passed,
  with the existing upstream Apple __eh_frame linker warning. Relaunched the standalone
  app with its existing project history and dev model store. Live underline motion and
  spacing remain unverified through the UI; no GitHub CI dispatched.

## 2026-10-29 — Inline project renaming

- Active project-card titles now use the normal text colour instead of the green accent.
- Sidebar project names and the header title below Back to Projects are shared inline
  editors: click the name to type, Enter or blur to save, Escape to cancel. Empty names
  are rejected; save errors stay visible beside the editor. Folder icons in sidebar
  rows open projects, separating navigation from name editing without nested buttons.
- Added a local rename command and core metadata operations. Display names persist
  atomically in project.json; project folders, database contents and embedding identity
  stay in place. Open-project config is updated only after a successful write; inactive
  project renaming reads/writes only metadata without opening a database or switching
  the active session. Format validation and non-empty/single-line validation are enforced.
- Successful edits update recent-project storage, sidebar, cards and the open header.
  Async completion checks the latest active project before updating its state, avoiding
  reopening an old project if the user navigated elsewhere during a rename.
- Validation: all 28 existing UI tests and 5 focused Rust project tests passed (three
  new tests cover persistence/path stability, metadata-only rename and invalid/failed
  writes preserving the old name). Core/app Clippy across all targets with warnings
  denied, workspace formatting and diff checks passed.
- Frontend and macOS release builds passed with the existing upstream Apple __eh_frame
  linker warning. Relaunched the standalone app with its existing project history and
  dev model store. Live editor interaction/appearance remains unverified through the UI.
  No GitHub CI dispatched; Windows/Linux remain unverified.

## 2026-10-29 — Cream light text, Inter and hidden content scrollbar

- Unified the light-text ink/text/text-2 tokens at #F7F3EB, retaining muted grey labels
  and semantic accent colours. Map ink and label/outline alpha colours use the same
  cream palette.
- Bundled @fontsource-variable/inter 5.3.0 and made Inter Variable the primary reading
  font, ahead of system fallbacks. Inter is sans-serif; interpreted the owner's named
  font choice as the intended font despite the word serif. Monospace codes/labels
  continue using the existing Martian Mono. Font assets load locally/offline.
- Hid native scrollbars on the left content menu and nested pages through Firefox
  and WebKit scrollbar styles, retaining scrolling and keyboard access.
- All 28 existing UI tests and frontend TypeScript/Vite build passed. npm installation
  reported zero vulnerabilities. Live typography/scroll review remains with the owner.
- The owner then requested changes only and no relaunch. Stopped the in-progress
  native rebuild (exit 130); no native build success is claimed for this change, and
  the running app was left untouched. No GitHub CI dispatched.

## 2026-10-29 — Streamlined gap assessment

- Gap detail: one heading ("Why it's flagged") instead of an eyebrow plus heading; the
  engine's sentence is muted; demand, coverage, activity (and last coverage / items in
  between when present) are a row of figures with mono labels; the score terms sit under
  a "Score breakdown" line carrying the total. Term labels now map the engine's actual
  term names (the old map used weight keys, so raw names like "demand per coverage (log)"
  showed).
- Checked in the browser stub harness only; not rebuilt or relaunched as the macOS app.
  UI tests (28), type check and frontend build pass. No CI dispatched.

## 2026-10-29 — Relaunch latest typography changes

- On the owner's subsequent relaunch request, completed the previously interrupted
  macOS release rebuild. Frontend and native builds passed with the existing upstream
  Apple __eh_frame linker warning. Relaunched the standalone app with its original
  project history and dev model store, including bundled Inter and cream text.
- Live typography/scrollbar appearance remains for owner review. No CI dispatched.

## 2026-10-29 — Owner-provided logo and app icon

- Moved Desktop/invectus-logo.png and Desktop/invectus-icon.png into
  src/assets/branding, removing the Desktop copies as requested. Original PNGs and
  transparency are preserved. The sidebar now displays the supplied full logo at its
  natural aspect ratio, replacing the bordered text placeholder.
- Regenerated the existing desktop icon asset set with Tauri's icon tool, using contain
  fitting to square the 179×174 source without cropping or stretching. Updated PNG,
  macOS ICNS, Windows ICO and existing Appx sizes; mobile outputs stayed in temp storage.
- Added a macOS-only AppKit setup hook to apply that same ICNS to the running app's Dock
  icon, including standalone release launches that retain the existing WebKit profile.
  Uses existing Objective-C runtime dependency versions and verifies the main thread.
- Visually inspected source/generated PNGs and verified alpha/square dimensions. All
  28 existing UI tests, frontend build and diff checks passed.
- macOS release executable and .app bundle builds passed with the existing upstream
  Apple __eh_frame linker warning. Verified the bundle declares icon.icns and its
  resource matches the generated source byte-for-byte. Formatting/diff checks passed.
  Relaunched the standalone app with its existing project history and dev model store;
  live sidebar/Dock appearance remains for owner review. Windows/Linux are unverified;
  no CI dispatched.

## 2026-10-29 — Revised sidebar logo and squircle app icon

- Replaced the branding logo with the owner's new Desktop PNG (593×174), moving the
  source into src/assets/branding and removing the Desktop copy. Capped the sidebar
  logo at 175 px wide while retaining responsive width and its natural aspect ratio.
- Added a 1024 px SVG icon composition: a charcoal gradient continuous squircle with
  transparent exterior margins and the supplied PNG mark centred inside. The original
  mark is embedded unchanged; the new surrounding frame is code-defined vector geometry.
- Regenerated the existing desktop icon formats from app-icon.svg and visually checked
  the rendered PNG. Bundle and standalone Dock hooks consume the same generated ICNS.
- Frontend, macOS release executable and .app bundle builds passed with the existing
  upstream Apple __eh_frame linker warning. Verified the bundled ICNS matches its
  generated source; diff checks passed. Relaunched the standalone app with its existing
  project history and dev model store. Live sidebar/Dock review remains with the owner;
  no CI dispatched.

## 2026-10-29 — Updated sidebar logo (884×246)

- Replaced src/assets/branding/invectus-logo.png with the owner's new Desktop PNG,
  removing the Desktop copy. Existing width-based sidebar CSS keeps its natural aspect.
- Frontend and macOS release .app bundle builds passed (existing upstream linker warning).
  Relaunched the standalone app with the dev model store; live appearance remains for
  owner review. No CI dispatched.

## 2026-10-29 — Collections tab in the instrument theme

- Restyled src/screens/Collections.tsx to match the Gaps tab and gap assessment: a coded
  section header with a text "Add collection" action, a line-art empty state, light sans
  collection names, a figures row (items, texts, indexes), a 2 px role split with keys,
  index rows on hairlines with dot status marks (ink instead of mint), and text-button
  actions. The details panel uses labelled calibration and regions lines. Behaviour is
  unchanged. The sidebar logo is now a fixed 100 px wide.
- `npm run build` (tsc + Vite) and the macOS release .app bundle build passed (existing
  upstream linker warning). Relaunched the standalone app with the dev model store; live
  appearance remains for owner review. No CI dispatched.

## 2026-10-29 — More spacing on the Collections tab

- Increased spacing on the Collections tab: 40 px between collections, 24 px between groups
  inside one, roomier figures, index rows, actions and details lines, and 1.6 line height.
- The macOS release .app bundle build passed. Relaunched the standalone app with the dev
  model store; live appearance remains for owner review. No CI dispatched.

## 2026-10-29 — Reserved Collections tab

- Removed from each collection: the indexed status (shown only when not fully indexed),
  source path and import time, the per-model index list with vector counts, and the role
  bar. Figures are now items, demand and coverage (neutral only when present). An index
  row appears only when it needs action (resume, retry, index changes). Model details
  stay under Details. Rows are inset 16 px like the gap assessment's sections.
- The macOS release .app bundle build passed. Relaunched the standalone app with the dev
  model store; live appearance remains for owner review. No CI dispatched.


## 2026-10-29 — Saved gaps and decision log

- Added Saved between Gaps and Collections, using numbered sections, ruled records and
  the existing instrument typography. Includes project-local gap bookmarks, latest human
  decisions and notes/dates, status filters, selected-first ordering, and a small highest-ranked
  unreviewed gap section. A collection/index selector makes the scope explicit.
- Added an additive gap_bookmarks table in project.duckdb and shared engine operations.
  Bookmarks use gap kind and core-item identity within a collection and embedding space,
  matching the verdict carry-over threshold (Jaccard >= 0.5); scan-local gap IDs are only
  navigation targets. Repeated saves are idempotent. Removing a bookmark preserves decisions.
  Saved snapshots and unmatched decisions remain listed when the latest scan has no match.
- Gap assessment now offers Save gap / Remove bookmark separately from Save decision.
  Saving a decision refreshes the shared gap list. Evidence opens inside Saved, Back returns
  there, and map highlighting follows the opened gap. Saved refreshes on entry; its overview
  and current-gap reads run in sequence. Background map/gap operations queue on the session
  mutex instead of falsely reporting normal concurrent reads as a running job.
- Native visual review caught clipped navigation at the 436 px pane width. Added a console
  container query to keep all four tabs visible and move the indexing indicator onto its own
  row below 620 px. Wide panes keep the existing toolbar layout.
- Decisions and spec conflicts: owner approved the Saved tab and first-version scope;
  updated SPEC §8. LLM discussion and detailed change tracking remain deferred. No chat or
  network behavior added.
- Validation: 27 core/gaps release tests and 29 UI tests passed locally on macOS, including
  reopen persistence, idempotency, independent bookmarking, rescan matching, collection/space
  isolation and exact 63-bit bookmark identities across the JavaScript boundary. Frontend
  build, formatting and diff checks passed. Targeted core/gaps/app Clippy passed with warnings
  denied. First native bundle built; live bookmarking, removal and Saved evidence/Back paths
  passed on Research Test without changing its decision. Final native bundle rebuild passed
  with the existing upstream Apple __eh_frame linker warning. Relaunched and verified all four
  tabs fit the 436 px console, the existing decision and highest-ranked unreviewed gap appear,
  status filtering works, and switching Gaps/Saved loads the 23-gap scan without false busy
  errors. Saved displays UTC database timestamps in local time (verified 9:49 PM, Oct 29).
  Left the app on Saved. No CI dispatched;
  Windows/Linux remain unverified.

## 2026-10-29 — One macOS app launch path

- The loose target/release/invectus-app executable and the .app bundle used separate
  WebKit profiles (invectus-app versus app.invectus.desktop), explaining the populated and
  blank recent-project lists. Project databases were not duplicated by these launch paths.
- Owner requested one version. Release macOS loose executables now open the sibling .app
  bundle through Launch Services when it exists, preserving explicit model-store/offline
  environment overrides. Future relaunches use the bundle's stable application identity.
- Stopped both old app processes and migrated the populated WebKit Default profile into
  the previously empty bundle profile, retaining both originals under the git-ignored
  .spike-data/launch-profile-backup-20261029-221753/. Verified the migrated recent list contains
  Research Test (10,000 items) and Medical School Research Topic; project folders stay put.
- Linked ~/Library/Application Support/inVectus/models to the existing dev model store so
  Finder/Dock launches use the same verified local files. No model or dataset copies/downloads.
  First native launch visibly showed both migrated projects. Starting the loose executable
  while the bundle was open exited successfully and kept exactly one app process (the bundle).
  Final rebuilt bundle was relaunched and visibly retained both projects. Repeated the loose
  launcher check with no environment overrides: exactly one bundled app process remains.
  The app is left open on Research Test → Saved.

## 2026-10-29 — Token chunking by default

- New desktop and CLI imports use the verified, installed BGE-small tokenizer with a
  448-token budget and 48-token overlap. Character chunking remains an explicit option
  for model-free imports; no model is downloaded implicitly. The ingest crate's legacy
  `ChunkPolicy::default()` and stored hashes remain unchanged.
- Reimports retain the collection's recorded chunk policy. The desktop preview now counts
  actual chunks with that policy instead of estimating from character length. Folder
  imports expose the same chunk controls as table imports.
- Added a reproducible local comparison in `crates/cli/examples/chunking_eval.rs`: up to
  5,000 local CSV papers, separate parent/chunk truncation counts, and exact cosine search
  collapsed by parent using the final 24 words of the first 100 abstracts over 2,000
  characters. These verbatim queries diagnose coverage, not general semantic relevance.
- Local macOS validation: frontend build and formatting passed; clean full workspace
  release test run passed (98 tests). An earlier workspace run passed runtime tests but
  exited abnormally during doc tests; a separate doc-test run and the clean full rerun
  passed. Existing upstream macOS linker warning about compact unwind offsets persists.
- Corpus comparison: Apple M4 Pro, 24 GiB RAM, macOS, BGE ONNX CPU, 4 threads,
  batch 32, existing 5,000-paper CSV (SHA-256
  `a263b0d857ee176dd472d7ed636481e7127598cb656ce08aae1abc0f16fe91b5`).
  Character mode produced 947 chunks, **120 truncated chunks**, and 464 truncated parents;
  token mode produced 1,366 chunks, **0 truncated chunks**, and the same 464 truncated
  parents. Every chunk was actually embedded with its title; parent and chunk counts were
  measured separately. Thus no chunk truncation occurred on this sample.
- For the same 100 abstract-ending queries, character/token recall@1 was 0.85/0.81,
  recall@10 was 0.94/0.94, and MRR was 0.8837/0.8541. This diagnostic did **not** show a
  ranking improvement. Token sizing improves measured text coverage; relevance gains remain
  unvalidated. Raw output: `.spike-data/p1/chunking-eval-2026-10-29.json` (ignored).
  Measured run times were 366.6/312.3 s with concurrent compilation; these are not clean
  throughput comparisons. The comparison reads the local CSV copy and writes no project data.
- Full-workspace release Clippy (`--all-targets -- -D warnings`), final frontend build,
  formatting and diff checks passed. The standalone app bundle was not rebuilt or
  relaunched in this change. No CI dispatched.

### Decisions and spec conflicts
- Owner approved token chunking as the default for new imports. SPEC §3 requires chunk and
  parent vectors but does not prescribe character sizing; no spec change is needed.
- A fixed 448-token body budget reserves space for titles and prefixes, but unusually long
  titles or another model's tokenizer can still exceed its cap. No universal no-truncation
  guarantee or general search-quality improvement is claimed.


## 2026-10-30 — Unified Add to Dataset entry point

- Removed the disabled PubMed query, Steam catalogue and gapmap import tiles from the
  import wizard. Removed the idle 84% indexing animation preview and its preview-only API;
  genuine job progress and the active-job loader remain.
- The project header now pairs its editable dataset/project title on the left with one
  Add to Dataset button on the right, using flex justify-content: space-between. The shared
  import wizard opens in Collections from any tab; the action is disabled during a running
  project job or while the wizard is already open. Folder/file picking and existing-collection
  updates continue through that wizard. Removed duplicate collection-add buttons and the
  second local import flow from Collections; empty collections direct users to the header.
- Decisions and spec conflicts: owner requested the unified header action and removal of
  unavailable-source placeholders. Updated SPEC §8; no connectors or import behavior added.
- Validation: frontend TypeScript/Vite build, all 29 existing UI tests and diff checks
  passed. On the owner's subsequent relaunch request, rebuilt the macOS .app bundle
  successfully (existing upstream Apple __eh_frame linker warning) and launched the canonical
  bundle. Accessibility inspection verified Add to Dataset in the project header and no idle
  indexing preview. Current recent-project history was retained; one bundled app process
  remains. Screenshot inspection timed out, so pixel-level layout review remains with the
  owner. No CI run.
- Follow-up (owner correction): moved Add to Dataset out of the project header into the
  Collections tab's own header row (label left, button right, space-between); restored the
  original project header. Disabled while a project job runs. Updated SPEC §8. Frontend
  build and 29 UI tests passed.

## 2026-10-30 — Public repository presentation and MIT licensing

- Owner requested public-facing preparation and then explicitly chose open source;
  applied MIT to original code, root/npm metadata, all in-house workspace packages and
  the separate spike package. Preserved third-party terms. Restored the missing
  LanceDB Apache-2.0 license verbatim from upstream v0.40.0, retained its copyright,
  and added prominent modification notices to the three patched vendored files.
  Copied the installed Inter/Martian Mono OFL and Lucide/Feather notices byte-for-byte.
  THIRD_PARTY_NOTICES.md distinguishes these from a future full binary-license inventory.
- Rewrote README.md around the product, actual workflow, four native screenshots,
  engineering choices and experimental limits. Added development, architecture, demo,
  publication handoff, contribution and security-reporting guides, plus issue templates.
  Corrected the old README's implied Windows/Linux/CI validation. Workflows remain
  manual-only and were not dispatched; no push or visibility change was performed.
- Added scripts/create_demo.py: standard-library-only original fictional urban-research
  records, deterministic IDs/CSV, optional offline CLI project build and clear missing-model
  failure. Final fresh project has 540 items in 12 topics (CSV SHA-256
  b06cdb1a503e67b70faedaeebf3708968a80d309a6f1941b86cfa1bbd3a4b321).
  Used the already installed BGE model. No model/dataset download or real collection text
  is required. Generated inputs, project and reports stay in ignored .spike-data/public-demo/.
- Demo pipeline on Apple M4 Pro / 24 GiB macOS: import, embeddings, no-LLM calibration,
  regions and gap scan completed offline; the synthetic scan emitted 3 coverage, 5 stale
  and 5 bridge candidates. These demonstrate mechanics, not discovery accuracy.
  During fixture iteration, reimport retained removed source rows as designed; the first
  preview project was kept under initial-preview.invectus and a fresh final project was
  created. The guide documents that reruns are not destructive resets.
- Built the current macOS .app bundle (existing upstream compact-unwind linker warning),
  reopened it, and captured actual workspace/evidence/search/Saved views with only the
  demo visible in the filtered sidebar. Created a real demo bookmark and a clearly
  illustrative Selected decision through the UI. Screenshots are authorized public
  presentation assets; no source dataset/project/model is committed. Exported PNGs are
  3024×1898; removed EXIF/text metadata while preserving compressed pixel data.
- Added scripts/audit_publication.py and checked all reachable local history/current
  non-ignored files offline: 120 commits, 768 text blobs, 52 binary blobs skipped, no
  oversized history text blobs or suspicious model/project/credential file paths. Five
  distinct literal-token candidates in two unchanged vendored test files were verified
  byte-for-byte against the pinned crates.io archive (whose SHA-256 also matches).
  The report retains these candidates for review. It is a heuristic, not a comprehensive
  secret scan; binary contents, unreachable objects and remote hosting metadata are outside
  its scope. Owner review of public commit identity remains in docs/publication.md.
- Validation: 29 UI tests, frontend build, native macOS bundle, Cargo metadata/lock checks
  (root and spike workspace), formatting/diff checks and local Markdown targets passed.
  Verified deterministic 540 unique IDs, 12 topics, explicit fictional content, no external
  source links, and the builder's missing-model failure without downloads/tracebacks.
  Full Rust tests were not repeated for documentation/license/comment changes; the actual
  demo exercised the existing Rust CLI and the native app. The app is left on demo Search.
- Decisions/spec conflicts: MIT supersedes the owner's brief rights-reserved choice in
  this conversation. Public presentation does not expand platform support, claim a signed
  release, validate synthetic relevance, or change SPEC behavior. Publication, profile/About
  settings and first public manual CI runs remain owner actions in docs/publication.md.

## 2026-10-30 — README brand header

- Matched the Angrove iOS README's centered brand stack: the existing app icon at
  128 px, then the full inVectus wordmark at 360 px, above the project heading.
- Dark themes use the original light-text PNG. Light themes use an SVG that embeds
  the same logo with a constant dark-color filter; geometry and transparency are
  preserved. No AI-generated replacement or app asset changes.
- Verified local image references and SVG structure; diff checks passed. No build,
  app relaunch, CI or push was needed for this documentation-only change.

## 2026-10-30 — Separate ophthalmology research project

- Owner selected the same 5,000-paper medical sample used by Research Test and asked
  to keep Urban Futures Demo in the sidebar. Created a separate local project at
  ~/inVectus Projects/Ophthalmology Research.invectus with one papers collection;
  did not import the duplicate character/token test collections.
- Imported the existing local papers-5k.csv copy (SHA-256
  a263b0d857ee176dd472d7ed636481e7127598cb656ce08aae1abc0f16fe91b5).
  Preserved the old mapping: PMID IDs, title/abstract/pub_date, Systematic Review
  and Meta-Analysis as coverage, Journal Article as demand, otherwise neutral.
  Result: 5,000 parents (4,922 demand / 66 coverage / 12 neutral), 1,366 token-sized
  chunks using the pinned BGE tokenizer (448 tokens, 48 overlap).
- Completed offline BGE embedding (6,366 vectors), no-LLM calibration (300 sampled
  items), regions (71 fine / 8 broad), and a scan (20 coverage / 3 stale candidates).
  These are collection outputs, not validation that the candidates are real gaps.
  Existing local model files were used; source CSV fingerprint is unchanged.
- Native verification: opened the project, confirmed both it and Urban Futures Demo
  in the sidebar, verified a semantic query returns medical-paper matches, checked
  collection counts/calibration/readiness, and left the app on its 23-gap list/map.
  Project files and generated reports are local user data and are not committed.
  No app code/build changes, network downloads, CI or push were needed.

## 2026-10-30 — Ophthalmology Research screenshots

- Added four owner-supplied captures of the native app on the Ophthalmology Research
  project to docs/screenshots/ (ophthalmology-search, -gap, -item, -collections). They
  are copies from the Desktop; the existing screenshots and README are unchanged.
  They show the real 5,000-paper collection, not the synthetic demo data.

## 2026-10-30 — README wordmark rendered blank on GitHub mobile

- The README's light-theme wordmark was an SVG embedding a base64 PNG through an SVG
  colour filter; the GitHub iOS app showed it as a broken image. Replaced it with a plain
  PNG (docs/branding/invectus-logo-dark-text.png, 884×246) made from the existing logo
  by keeping its alpha and setting the ink to the same near-black the SVG filter used.
  The dark-theme source (src/assets/branding/invectus-logo.png) is unchanged.
- Not verified on GitHub itself: the repository is private and nothing was viewed there
  after the push. Checked only that the file is a valid PNG and renders correctly locally.

## 2026-10-30 — Screenshot caveat removed; repository stays private

- Owner chose to keep the four Ophthalmology Research screenshots and drop the README
  caption saying all records are synthetic. Removed that caption; the demo docs still
  describe the Urban Futures data as fictional. docs/publication.md now notes the
  ophthalmology screenshots show real PubMed titles.
- Owner asked about making the repository public, then chose to hold off pending a
  review of the commit author identity and local paths in history. Visibility is
  unchanged (private); nothing was changed on GitHub settings.

## 2026-10-30 — Repository made public

- Owner approved publishing with the existing history. Set the global git email to the
  GitHub noreply address for new commits; the 97 earlier commits still carry the owner's
  Gmail address (no history rewrite; the same address is already in other public repos).
- Pushed main (150261f) and changed rbaltodano/inVectus visibility to Public with
  `gh repo edit`. Confirmed visibility reads PUBLIC afterwards.
- Not done: no workflow was dispatched (both stay manual), so Windows/Linux remain
  unverified. About text, topics, vulnerability reporting and the GitHub email-privacy
  settings are for the owner to set in the web UI.

## 2026-10-30 — Current screenshots and repository presentation

- Located and visually reviewed the four owner-supplied ophthalmology captures already
  in docs/screenshots/. Updated the public README to use the current gap view as its
  hero and link the current search, source-item and Collections captures. Kept the
  synthetic Saved example labelled separately; added collection-level evidence context.
- Kept AGENTS.md/CLAUDE.md and the root ledger in place: these are active contributor
  instructions and the documented source of truth, rather than disposable clutter.
- Checked local README image/link targets and diff formatting. No application code or
  workflow triggers changed. Public repository topics/About and private vulnerability
  reporting are handled separately from source changes; no CI was dispatched here.
- Confirmed the GitHub About description and six relevant topics are set, and private
  vulnerability reporting reads enabled=true. Remaining owner choices are a personal
  motivation paragraph, a short recording, profile pinning, and the first manual CI run.

## 2026-10-30 — Native macOS glass sidebar and integrated window controls

- Added a macOS-only Tauri window override: transparent webview, native Sidebar visual
  effect following window activation, dark appearance, overlay title bar with hidden
  title, and native traffic lights positioned at (16, 20) inside the sidebar.
- Scoped transparent document/sidebar backgrounds to native macOS. The main pane stays
  opaque. Reserved 64 px above sidebar content and added a 48 px draggable top strip
  with the window start-dragging capability. Browser/Windows/Linux styling retains its
  existing background and title-bar configuration.
- Frontend build, 29 UI tests, diff checks and macOS release .app bundle passed with
  the existing upstream compact-unwind linker warning. Relaunched the bundle and
  visually verified the material sidebar, traffic lights and absence of a separate
  title strip. Both recent projects remained available and the medical map opened.
- A native drag check was interrupted by user interaction; dragging and full-screen
  transitions remain for owner verification. No full Rust suite or cross-platform
  execution was needed for this configuration/CSS change. No CI or push performed.

## 2026-10-30 — Sidebar glass tint and window-control spacing

- Owner requested matching 24 px top/left spacing. Native macOS sidebar content now
  has 24 px side insets, and traffic lights start at x=24. Tao's y setting sizes the
  title-bar container rather than setting the button's top directly: y=33 compensates
  for the native button's 9 px origin to give a 24 px top inset on this Mac.
- Applied a 45% near-black translucent tint above the native Sidebar material for a
  darker frosted appearance. Native blur and activation behavior remain in use.
- Frontend, macOS release bundle, 29 UI tests and diff checks passed. Existing upstream
  compact-unwind linker warning persists. No CI or push performed.
- Relaunched and visually confirmed the controls' 24 px top/left inset, matching
  sidebar content alignment, and darker material while opening the medical project.

## 2026-10-30 — Rounded sidebar selections

- Chose a 4 px radius for sidebar navigation and project-row hover/selected backgrounds.
  The instrument theme's general control radius remains 2 px; 4 px gives these larger
  selection targets a restrained softness without the stronger 8 px rounding.
- Frontend and macOS release .app builds passed (existing upstream compact-unwind
  linker warning). No application behavior or workflow changes; no CI or push.
- Relaunched and visually verified the 4 px New project selection in the native app.

## 2026-10-30 — Consistent control corners and project navigation

- Set the shared control radius to 4 px, including sidebar search, buttons and fields.
- Added Command-K (Control-K also supported): inside a project it restores the Search
  tab's root without discarding its query, focuses/selects the query field, and works
  from other tabs/details. Outside a project it focuses Find a project. The existing
  slash shortcut is retained; composing input is ignored.
- Project-name single clicks open/select the project; double clicks edit its name.
  A short deferred mouse click allows double-click detection without opening first.
  Keyboard activation opens the sidebar project, F2 renames, and Enter/Escape/blur
  editing behavior is preserved. Header names use double-click/F2 for editing too.
- Frontend, native macOS release bundle and 29 UI tests passed (existing upstream
  compact-unwind linker warning). Relaunched the bundle and confirmed single-click
  project opening and Command-K navigation. Owner requested no further computer-use
  verification and will review visual/interaction behavior directly. No CI or push.

## 2026-10-30 — Global project search and Command-number navigation

- Corrected Command-K to focus/select the sidebar's global Find a project field on
  every screen, without switching project tabs. Control-K remains an alternative;
  the slash shortcut still focuses collection search when its page is active.
- Holding Command displays right-aligned 1–9 hints for the first nine visible sidebar
  projects. Command plus that number opens the corresponding visible project using
  existing opening/job guards. Repeated keydown events do not repeatedly navigate;
  hints clear on Command release, window blur and visibility changes. Filtered list
  numbering follows the displayed order, with accessible shortcut metadata on rows.
- Frontend build, macOS .app bundle, 29 existing UI tests and diff checks passed.
  Native interaction verification is left to the owner as requested; no computer-use
  actions, CI, or push were performed. Restart the app to load the rebuilt bundle.

## 2026-10-30 — More robust Command shortcut hints

- Owner reported that project numbers were not appearing. Confirmed the built assets
  include project-shortcut labels; native visibility was not independently reproduced.
- Explicitly recognize Meta/MetaLeft/MetaRight keydown, rather than depending solely
  on the modifier flag for the modifier's own event. Keyboard handlers now run in
  capture phase; mouse movement/clicks recover Command state if held before focus.
  Keyup, window blur and visibility changes still clear the hints.
- Rebuilt the macOS bundle and passed 29 existing UI tests and diff checks. Following
  the owner's preference, no computer-use verification/relaunch was performed; the
  updated bundle needs a full quit/reopen before testing. No CI or push performed.

## 2026-11-03 — Shared orchestration crate (roadmap A1)

- New crate `invectus-orchestration` (`crates/orchestration`), free of Tauri: the session
  (open project, cached query encoder, local model), the job board (moved from
  `src-tauri/src/jobs.rs` unchanged), project readiness (`project_info`, `index_status`,
  `plan_index`), the import → index → map → gap scan → calibration pipeline, calibration and
  region jobs, model lists/downloads, search and check. Job changes go through an `Emit`
  callback, so the MCP server (A2) can reuse it without a window.
- `src-tauri/src/lib.rs` (1,849 → 584 lines) now adapts these to commands and the `job-event`;
  command names, arguments and JSON shapes are unchanged. Gap commands stay thin wrappers
  over `invectus-gaps` in the shell for now.
- The end-to-end test (import → automatic model → index → regions → gaps → calibration,
  outdated after re-import, failed job and retry) and the job-guard tests moved to the crate
  and pass against it with the dev BGE model (M4 Pro, 240 synthetic rows).
- Checks (arm64 macOS only): `cargo fmt --check`, workspace clippy `-D warnings`, 98
  workspace tests (release) and `npm run build` passed. The desktop bundle was not rebuilt or
  relaunched; behavior is expected unchanged but not checked in the running app. No CI or push.


## 2026-11-03 — Instrument palette from the shared Figma colours

- Retuned the instrument theme to the Figma "ESB Stuff 3" colours (node 1227:166, shared with
  ryanbaltodano.com): background #141414, surfaces #202020/#292929, lines #333333/#48453f,
  text #d1cbc2 with 50% for dim text. The data circle's ink, gap-dot fill and label
  backing follow the same values.
- `npm run build` passed. Not checked in the running app. No CI or push.

## 2026-11-03 — `invectus mcp` with read tools (roadmap A2)

- `invectus mcp` (`crates/cli/src/mcp.rs`): a Model Context Protocol server over stdio,
  newline-delimited JSON-RPC 2.0, written without an MCP library (no new dependency).
  Handles `initialize` (protocol 2025-06-18, 2025-03-26 or 2024-11-05), `ping`,
  `tools/list`, `tools/call` and batches; notifications get no reply; logs go to stderr
  only. No network port, and no tool writes to a project or uses the network.
- Tools: `list_projects`, `open_project`, `project_status`, `search`, `check_idea`,
  `list_gaps`, `gap_detail`, all over `invectus-orchestration`. Results come back as JSON
  text plus `structuredContent`; a failed tool is an `isError` result the agent can read.
  `check_idea` is exhaustive unless `fast`; verdict and gap answers carry a "this
  collection only" scope note, and `search` says its similarity is not a probability.
- `--models-dir` is now passed to the shared crate through `INVECTUS_MODELS_DIR`.
- Tests: protocol handshake, errors and argument validation; project listing/opening; an
  end-to-end run (import → index → map → calibrate through the shared pipeline, then
  `check_idea` returns a verdict, P(exists), calibration quality and nearest items;
  `list_gaps`, `gap_detail`); and a stdio test of the built binary. 102 workspace tests,
  fmt and clippy passed (arm64 macOS, M4 Pro).
- Smoke test on `gapmap.invectus` (2,000 papers, imported Nomic vectors): `check_idea`
  answered in about 0.3 s per process (verdict, held-out AUC 0.804 quality note, exhaustive
  evidence); `list_gaps` returned ranked gaps.
- Not done: registering with a real agent client (`claude mcp add …`) was not tried.
  `list_projects` scans one folder (default `~/inVectus Projects`); the app's recent
  projects live in the webview's storage and are not visible to the server.

### Decisions and spec conflicts to review
- Single writer: the server opens projects the same way the CLI does. DuckDB locks a
  database file for one process, so a project open in the app is refused with the store's
  "one inVectus process at a time" message. Target product §8 stage 1 says the server
  opens it read-only; a read-only open is also refused while another process holds the
  write lock, so stage 1 needs another approach (A3).

## 2026-11-04 — MCP write tools and single-writer handling (roadmap A3)

- `invectus mcp` adds `inspect_source`, `import_collection`, `job_status`, `cancel_job`,
  `scan_gaps`, `verify_gap` and `export_gaps` (14 tools). `open_project` takes `create`.
  Imports use the app's defaults (token chunks for the default model when installed, else
  2,000 characters; a collection that exists keeps its saved chunking) and start the
  index → map → gap scan → calibration job in the background; agents follow it with
  `job_status`. One job at a time; a second is refused with the running job's name.
- `verify_gap` never overrides the project's network switch (the app's per-call override
  is not offered to agents), and `INVECTUS_OFFLINE` / `--offline` refuse it.
- When the client disconnects, the server cancels a running job and waits up to 60 s, so no
  job record is left `running`.
- Gap scan and export moved into `invectus-orchestration`; the desktop commands call them.
- Fixed from A2: the role schema listed `primary`; the roles are `demand`, `coverage` and
  `neutral` (SPEC §3).
- Single writer (target product §8 stage 1, adapted): a project held by another process
  (the app or another `invectus`) is refused for all tools with the store's "one inVectus
  process at a time; close the desktop app…" message. Reads are refused too; see the
  decision under A2.
- Test (A3 goal, over the built binary's stdio, dev BGE model, 240 synthetic rows): create
  a project, inspect and import a CSV with role rules, poll the job to completion, read
  readiness (indexed, calibrated, scanned), list and re-scan gaps, check an idea, refused
  verification offline, Markdown export inside `exports/`; refusal while the test process
  holds the project; and no `running` job left after a client disconnects mid-import.
  103 workspace tests, fmt and clippy passed (arm64 macOS, M4 Pro). No CI or push.
- Unverified: a real agent client session; a cancel arriving mid-calibration on a large
  collection (the 60 s wait may not suffice there).

## 2026-11-04 — Decision provenance and agent suggestions (roadmap A4)

- Store: `gap_verdicts` gains `author` (`person` or `agent:<client>/<model>`) and `entry`
  (`decision`, `suggestion`, `dismissal`), added with `ALTER … ADD COLUMN IF NOT EXISTS`;
  older rows read as `person` / `decision`. Saving validates both; only an agent can make a
  suggestion.
- Gaps: `GapView` adds `suggestion`, the newest matching entry when it is a suggestion (a
  later decision or dismissal settles it). `verdict` stays the person's newest decision and
  is never replaced by a suggestion. The Saved decision log lists decisions only. Exports
  are unchanged (decisions only). New `suggest` and `dismiss_suggestion`; `gaps show`
  prints a pending suggestion.
- MCP: `suggest_decision` (15 tools). Author = `agent:` + the client's `clientInfo.name`
  from `initialize` + `/model` when the agent names its model. Reasoning is required.
- App (built in the instrument style, to refine with the owner): gap detail shows an
  "Agent suggests" block above "Decision and note" with a dashed outline (mint stays
  reserved for the person's search), the agent's name, time, suggested decision and
  reasoning, and Accept / Edit before saving / Dismiss. Accept saves the person's own
  decision (the agent's reasoning goes into the note only when the note is empty). The gap
  list shows "agent suggests …"; Saved has an "Agent suggestions" section. New command
  `dismiss_gap_suggestion`.
- Fixed test gaps from A2/A3: `gap_id` is at the top level of a gap view (flattened); the
  MCP tests looked for `gap.gap_id` and silently skipped the gap-detail and verify checks.
  They are now mandatory and pass.
- Checks (arm64 macOS, M4 Pro): 103 workspace tests (including the suggestion flow in
  `gaps_flow` and over MCP stdio), fmt, clippy, `npm run build`, 29 UI tests; macOS .app
  bundle rebuilt. The new UI was not checked visually in the running app.
- Demo data: one suggestion seeded through MCP on `.spike-data/p1/work/gapmap.invectus`
  gap 1 ("pressure · effects · effect"), author `agent:claude-code/claude-opus-5-5`,
  reasoning marked as a demo; dismiss it in the app after review. No CI or push.

### Decisions and spec conflicts to review
- Target product §7 names the field `kind` (`decision` | `suggestion`); `gap_verdicts.kind`
  already holds the gap type, so it is `entry`, with a third value `dismissal` for "set
  aside without deciding".
- §7 also asks for `author` on descriptions, region names and calibration paraphrases.
  Only the local model writes those today; their provenance comes with A5, when agents can
  write them.
- Open question 3 (automatic decisions) is answered conservatively for now: suggestions
  never become decisions without the person.

## 2026-11-04 — Agent-written descriptions, paraphrases and region names (roadmap A5)

- **Descriptions.** `invectus-midpoint` splits into `prepare` (target point, prompt, evidence)
  and `assess` (on-target and novelty checks), shared by the local model's `describe` and a
  new `submit` for outside writers; `context` returns the same prompt and evidence (titles
  and roles only) plus the checks in words. `submit` validates the text, runs the checks once
  and stores the result **only if it passes**; rejections come back with reasons ("off target
  … lands nearest region 21", "already in this collection (P(exists) 0.81): …"). Results carry
  `author` (`local-model` or `agent:<client>/<model>`); verification already reuses accepted
  descriptions from the store, so agent descriptions feed its queries.
- **Paraphrases.** Calibration takes `Queries::{Titles, Model, Given}`; `run_calibration` is
  unchanged for the first two. `paraphrase_sample` returns exactly the items calibration uses
  (same order and filter); `screen_paraphrases` rejects paraphrases of items outside the
  sample, empty ones, ones that repeat the title (letters and digits compared) and ones over 80
  words. Given paraphrases are fitted as the local model's are; sampled items without an
  accepted paraphrase are left out (never padded with titles); fewer than 30 is refused. The
  method records the writer, e.g. `llm paraphrase (agent:claude-code/claude-opus-5-5)`.
- **Region names.** `region_context` (terms, central titles, the local model's prompt, current
  name and who wrote it) and `set_names` (cleaned like the model's replies, stored in
  `llm_label` with the writer in `llm_model`). `RegionRecord` now exposes `llm_model`. No
  check exists for names (none in the spec).
- **MCP (21 tools):** `gap_context`, `submit_description`, `paraphrase_sample` (paged; item ids
  as strings because they exceed JavaScript's exact integers), `submit_paraphrases`
  (background job), `region_context`, `submit_region_names`. `submit_paraphrases` is a
  write that replaces the collection's current calibration.
- **Tests:** given-paraphrase screening and fitting (`verdict_flow`), agent descriptions
  rejected off target and stored on target with the agent as author (`describe_flow`), and
  the stdio end-to-end test now covers all six tools (240 synthetic rows, dev BGE).
  104 workspace tests, fmt, clippy and `npm run build` passed (arm64 macOS, M4 Pro).

### Measured: agent (Claude Opus 5.5 via MCP) vs the local model (Gemma 4 12B)
Copy of `demo.invectus` (5,000 papers, BGE space `0b35fad4e4ec`) at
`.spike-data/p1/work/a5-eval/`; inputs, outputs and the call helper are kept there. Same
calibration for both writers' novelty checks; genres "research study" / "systematic review".

| Descriptions, top 10 gaps (all coverage/stale; the scan had no bridges) | first attempt | within 3 |
|---|---|---|
| Gemma 4 12B (`gaps describe`, ~5–7 s per gap) | **10 / 10** | 10 / 10 |
| Agent, from `gap_context` only, revised from the rejection reasons | 8 / 10 | **10 / 10** (2nd attempt) |

- Agent rejections: gap 1 off target (leaned on saccades; landed in "Oculomotor Control"),
  gap 8 judged `exists` (P 0.81) against "Artificial intelligence in retinal screening using
  OCT images", a different topic sharing imaging vocabulary: likely a **false `exists`** from
  the coverage-only check. Caveat: the agent saw Gemma's titles before writing.
- Region targets are easy to pass (Gemma never needed a retry); bridges, where the
  combination rule bites, were not measured because this scan has none.

| Calibration, same 300 items and held-out 60 | held-out AUC | balanced Brier | balanced accuracy |
|---|---|---|---|
| Gemma paraphrases (2026-10-27) | **0.952** | 0.092 | 0.858 |
| Titles, no LLM (2026-10-27) | 0.890 | 0.141 | 0.808 |
| Agent paraphrases (all 300 passed screening; fit 4.0 s) | 0.866 | 0.143 | 0.800 |

- Why: the agent applied "in your own words" literally and replaced technical terms with lay
  ones ("Müller glial cells" → "retinal support glia", "S-cone" → "blue-sensitive cones",
  "radial artery" → "wrist artery"); Gemma keeps the field's vocabulary and rephrases the
  sentence. Lay queries sit farther from the stored text, so exists/absent separate less. A
  higher AUC is not automatically a better-calibrated curve for real users (closer
  paraphrases make easier positives); SPEC §9.2's hand-labelled queries remain unmeasured.
- One writer, one pass, one collection; not a general result about agents.

### Decisions and spec conflicts to review
- **Proposal (not applied):** tell agents in `paraphrase_sample` to keep the field's technical
  terms and rephrase the sentence. Changing the shared `PARAPHRASE_RULE` would also change the
  local model's prompt and its 0.952 baseline, so it needs the owner's call. Until then an
  agent calibration can replace a better local-model curve: `submit_paraphrases` should
  perhaps refuse (or warn) when the new held-out AUC is below the current curve's.
- Description novelty uses coverage items only for region targets (unchanged from P5); the
  gap 8 case suggests it over-calls `exists` when a coverage item shares method vocabulary.
- Region names from agents replace an existing name; no check, as in the spec.

## 2026-11-04 — Connect an AI agent and `.mcpb` bundle (roadmap A6, backend)

- `invectus-orchestration::connect`: plan / apply / disconnect / status for **Claude Code**
  (runs `claude mcp add --scope user invectus -- <bin> mcp`; status read from
  `~/.claude.json` without running anything), **Claude Desktop** (macOS
  `~/Library/Application Support/Claude/claude_desktop_config.json`; Windows `%APPDATA%`;
  Linux `~/.config/Claude`) and **Cursor** (`~/.cursor/mcp.json`). A plan shows the exact
  entry, file and notes and writes nothing. Apply merges into `mcpServers` keeping every other
  key and server, copies the old file to `<file>.invectus-backup`, writes atomically, and
  refuses a file that is not a JSON object (left untouched). The registered binary is the CLI
  itself or the `invectus` beside the app's executable.
- CLI: `invectus connect add <client> [--yes] [--json]`, `connect remove <client> [--yes]`,
  `connect status [--json]`, `connect bundle [--out]`. Without `--yes` nothing changes.
- `.mcpb` (Claude Desktop extension): `manifest.json` (manifest_version 0.3, binary server
  `${__dirname}/server/invectus mcp`, platform of the build) zipped with the binary using the
  system `zip`. The unpacked bundle's server answered `initialize`. 110.5 MB zipped.
- App: the macOS bundle now ships the CLI (`bundle.macOS.files` → `Contents/MacOS/invectus`;
  `beforeBuildCommand` builds `invectus-cli`). Tauri commands `connect_plan`, `connect_apply`
  (recomputes the plan itself), `connect_remove`, `connect_status`, and `api.ts` bindings;
  **no Settings screen yet** (owner designs it).
- Tests (all against a temporary home; no real client config was read or written): merge and
  backup, other servers kept, idempotent re-plan, replacing another install's entry, broken
  JSON refused, Claude Code via a stand-in `claude` script, status, manifest; CLI test of
  add/remove/status/bundle. 109 workspace tests, fmt, clippy, 29 UI tests passed; app bundle
  rebuilt (arm64 macOS).
- Unverified: installing the `.mcpb` in Claude Desktop, and connecting a real Claude Code,
  Claude Desktop or Cursor (needs the owner's go-ahead to touch their configs). The
  manifest fields follow the published MCPB format as understood here, not a validator.
- Size: `invectus` is 361 MB (`strip -x` gives 281 MB; most is LanceDB/DataFusion code), and
  the app bundle carries it beside the 371 MB app executable. Options for later: strip
  symbols in release, or one multi-call binary for app and CLI.
- Windows/Linux: the bundle path, `.exe` names and config locations are coded but unverified.

## 2026-11-04 — App hand-off socket (roadmap A7, backend)

- The tools moved from `crates/cli/src/mcp.rs` into `invectus-orchestration::agent`
  (`Agent { session, board, emit, client, pinned }`), so `invectus mcp` and the app run the
  same code. `mcp.rs` keeps the JSON-RPC protocol and routing.
- `invectus-orchestration::handoff`: while the app runs it listens on a per-user Unix socket,
  `<OS data dir>/inVectus/agent.sock` (override `INVECTUS_AGENT_SOCKET`); no network port. The
  folder is 0700, the socket 0600, and each connection's peer uid (`getpeereid`) must be the
  app's. A stale socket is replaced; a live one (a second app) is refused. Requests: `status`
  (the open project) and `tool` (runs in the app's session and job board).
- Routing in `invectus mcp`: a call goes to the app when it names the app's open project, or
  names none and the agent has no local project; `job_status` / `cancel_job` follow the last
  routed call; `list_projects` / `inspect_source` stay local. Through the app the agent is
  **pinned** to the app's open project: it can never switch the person's project, and
  `open_project` elsewhere is refused there (other projects open in the server itself).
- App: listens at startup (`start_agent_handoff`); forwarded jobs emit the usual
  `job-event` (so they show in Activity), and each call emits `agent-activity` (client, tool,
  project, query/idea, gap id, ok/error, time) for the live-activity display. The window
  reloads project info and gaps after an agent's writes (suggestions, scans, descriptions,
  names, imports). No new visuals.
- This replaces target product §8 stage 1's "open read-only" (impossible while another process
  holds DuckDB's lock; see A2): with the app running, its project is reachable; without it,
  the server opens projects directly.
- Tests: socket permissions, stale/duplicate listeners, status, pinning and activity events
  (unit); a spawned `invectus mcp` reaching a project whose DuckDB lock this test process
  holds, `job_status` following it, another project opening locally, and the lock refusal
  without the app (integration). 111 workspace tests, fmt, clippy, 29 UI tests passed; app
  bundle rebuilt (arm64 macOS). Relaunch the app to load it.
- Unverified: the running app with a real agent client; Windows (named pipe not built; the
  server always opens projects itself there); a forwarded long job while the person also
  works in the window (both share one board, so the second job is refused as designed).
- Known edge: if the server opened a project before the app did, the app cannot open it
  until the agent session ends (the lock is the server's).

## 2026-11-04 — Headless agent edition (roadmap A8, first half)

- `invectus-llm` puts llama.cpp behind a `local-model` feature (default on in the crate; the
  workspace dependency turns it off and only the CLI's default feature and the desktop app
  turn it on). Without it `LlamaGenerator::load` fails with "this edition of inVectus has no
  local language model …; connect an AI agent …", so no caller changed.
- Agent edition: `cargo build --release -p invectus-cli --no-default-features`. Checked: no
  Metal framework linked (`otool -L`), `gaps describe` gives that message, `invectus mcp` lists
  all 21 tools. Size 352 MB vs 362 MB full: llama.cpp is not what makes the binary big.
- `scripts/package-agent-edition.sh` → `target/agent-edition/package/invectus-agent-0.1.0-
  darwin-arm64.tar.gz` (209 MB: binary, `install.sh`, README, LICENSE, `invectus.mcpb`).
  `install.sh` copies to `~/.local/bin` (no sudo); `--model` (≈134 MB embedding model) and
  `--connect <client>` each ask first unless `--yes`. Checked in a temporary home: declining
  writes nothing; `--connect cursor --yes` registers the installed binary; the installed
  server answers. Not built into CI or tests.
- 111 workspace tests, fmt, clippy (default and `--no-default-features`) passed (arm64 macOS).
- **Not done (needs the owner):** the small local model evaluation (A8 second half) requires
  downloading a 2–4 B GGUF model; Windows/Linux packaging is unverified; no code signing or
  notarization (the binary is unsigned, so Gatekeeper may block a downloaded copy).

## 2026-11-04 — Agent calibration reports the curve it replaced

- `submit_paraphrases` jobs now record the replaced calibration (`previous`: method and
  held-out AUC) and add a `warning` when the new curve's held-out AUC is lower. Report only:
  the new curve is still used and the old one stays in the history. Refusing worse curves,
  and the paraphrase guidance, remain the owner's decisions (A5 entry). Covered by the MCP
  end-to-end test. Not re-run on the demo.

## 2026-11-05 — Agent UI: connect screen, live activity, suggestion refinements

Owner asked for a first pass at the agent UI ("try your hand at those"); built in the
instrument style, dashed outlines marking agents' work throughout, for review together.
- **Settings → Connect an AI agent:** Claude Code, Claude Desktop and Cursor with their
  status (connected, connected to another install, not connected, not found). "Connect…"
  shows the exact change first (file and JSON entry, or the `claude mcp add` command, with
  notes) and makes it only on "Make this change"; Disconnect; copy-command fallback.
- **Live agent activity** (`src/agents.tsx`, from `agent-activity` events): a sidebar line
  above the job line while an agent is active (client, what it just did, when; clears 90 s
  after its last call), an "Agent active" chip in the project header, and the last eight
  calls under "Recent agent activity" in Settings. Pulse is off with reduced motion.
- **Refinements:** gap rows carry a compact "Agent suggestion" tag instead of a status that
  wrapped; the suggestion is its own panel above the decision panel, with a small tracked
  label and the suggested decision larger; Saved no longer lists a gap with a pending
  suggestion under "Next to review".
- Checked by rendering the real frontend in headless WebKit with engine JSON from the
  gapmap dev project and a stub for app calls (not committed); 29 UI tests, `npm run build`;
  app bundle rebuilt. Not checked in the native window. Circle marks for agents' searches
  are not built (agent calls do not return map positions yet).

## 2026-11-05 — Agents shown in the sidebar's project rows

- Owner's direction: sidebar projects are two lines, the name and a quiet line
  under it. The line shows an active agent's latest call (e.g. "checked “…”", "suggested a
  decision on gap 4"), else a running job's phase, else the item count. The agent client's
  mark sits at the right edge (Claude, OpenAI/Codex, Cursor; others get a generic bot icon),
  pulsing gently; the Command-number hint replaces it while Command is held. The sidebar
  box and header chip from the previous pass are removed; Settings keeps the recent list.
- The marks' SVG paths come from T3 Code (MIT, `docs/licenses/T3-Code-MIT.txt`); they are
  their owners' trademarks, used only to name the client.
- Agents in other projects now show too: when `invectus mcp` runs a call locally (not the
  app's project) and the app is running, it sends the app a one-way `activity` notice over
  the hand-off socket; the app only records it (nothing runs). The window reloads after an
  agent's writes only when they are in its open project.
- Tests: the hand-off integration test checks the local call's notice reaches the app. 111
  workspace tests, fmt, clippy, 29 UI tests, `npm run build`; bundle rebuilt. Checked in
  headless WebKit with simulated activity; not in the native window.

## 2026-11-05 — Letter-by-letter thinking shimmer on agent rows

- While an agent is mid-work (last call under 15 s ago) the sidebar row's agent line shimmers
  letter by letter: one bright peak per 2 s cycle travels strictly left to right, each letter
  lighting one equal step after its neighbour (60 ms, smaller for long lines so a pass fits in
  70% of the cycle), then a pause. Plain text afterwards until the agent clears (90 s). Off
  with reduced motion; screen readers get the plain text.
- The first version added an irregular per-letter offset and several peaks per cycle, so
  letters flashed out of order (owner caught it); replaced.
- Measured in headless WebKit by setting the animation clock: 24 letters peaked at 140, 200,
  255 … 1480 ms, strictly increasing. Frontend build passed. The app bundle was not rebuilt:
  another agent had uncommitted changes in the tree, which this commit leaves out.

## 2026-11-05 — Terminal-style stepped shimmer

- Owner: the ordered CSS wave still looked like an ordinary gradient shimmer. Cause: CSS
  interpolates each letter's opacity continuously at display rate, so neighbouring letters
  blend into a smooth band. Terminal shimmers repaint on a tick and snap each character cell
  to a few fixed colours.
- `Shimmer` now works the same way: an 80 ms tick (≈12 fps) moves the bright head exactly one
  letter; each letter takes 100%, 80% or 60% at distance 0, 1, 2 from the head, else 40%, with
  no transition; eight ticks of pause between passes. Reduced motion shows it still.
- Verified in headless WebKit with Playwright's clock paused between ticks: only the four
  levels occur, and the head advances by exactly one letter per tick (0…23, pause, repeat).
  An unpaused run showed two-letter jumps; that was the test clock flowing during screenshots.
  Frontend build passed. Bundle not rebuilt (another agent's uncommitted changes in the tree).
- Tuned with the owner (approved): 120 ms tick (≈8 fps), three levels (head 100%, direct
  neighbours 65%, rest 35%), six ticks of pause. Re-verified with the paused clock: only those
  levels, one letter per tick.

## 2026-11-05 — Codex as a connect client; each agent's sign-in shown

- Approach: inVectus never handles an agent's tokens. It asks the client's own CLI who is
  signed in and, if nobody is, tells the person to run `claude auth login` / `codex login`.
- `connect::Client::Codex`: `codex mcp add invectus -- <bin> mcp` / `codex mcp remove
  invectus`; status and plans read `$CODEX_HOME/config.toml` (default `~/.codex`) without
  running Codex, so an up-to-date entry is a no-op. Codex's `mcp add` replaces an existing
  entry (checked with a temporary `CODEX_HOME`). New dependency: `toml` 0.8 (already in the
  lockfile).
- `connect::account`: `claude auth status` (JSON) → "Claude Pro/Max/Team… subscription",
  "Anthropic API key", Bedrock/Vertex/Foundry; `codex login status` → "ChatGPT subscription",
  "OpenAI API key", or not signed in. Masked keys are never repeated. 20 s timeout; not run
  under `INVECTUS_OFFLINE`, since whether the CLIs use the network for this is unverified.
  Claude Desktop and Cursor have no CLI to ask; no sign-in is shown for them.
- Settings → Connect an AI agent lists Codex and a sign-in line under Claude Code and Codex
  (loaded after the fast status). `invectus connect status` prints it too.
- Checked on the owner's Mac (claude 2.1.311, codex-cli 0.166.0): Claude Code "Claude Pro
  subscription", Codex "ChatGPT subscription". Unit tests use stand-in CLIs; orchestration
  connect tests, the CLI connect test, clippy, fmt and `npm run build` pass. Unverified: a
  real Codex session calling invectus tools; the ChatGPT plan type (Plus/Pro), which
  `codex login status` does not print.
- **Decision (owner, 2026-11-05):** agents stay MCP-only, used from their own client (target
  product §5). No in-app chat panel that runs `claude`/`codex` for now.

## 2026-11-05 — Readable suggestion attribution

- Owner requested the sidebar's agent symbol and a normal model name, without the client
  slug or timestamp. Gap detail and Saved now share that attribution component; for example,
  `claude-opus-5-5-medium` displays as “Claude Opus 5.5 Medium”. Stored provenance is unchanged.
- Effort appears only when reported in the model string; existing records without it display
  “Claude Opus 5.5”, without guessing Medium. Missing models fall back to the client name.
- Frontend build and direct formatting assertions passed. Native bundle not rebuilt;
  layout not visually verified in the app.

## 2026-11-05 — Provider-grouped model selector layout for review

- Built a controlled `ModelSelector` component with the sidebar's Claude/OpenAI symbols:
  8 px from provider heading to model list, 8 px between options, 24 px between providers.
- Options are illustrative UI review entries, not a verified client capability catalog.
  Not wired to suggestion attribution yet: awaiting the owner's clarification of whether
  choosing a model is a prototype interaction or controls future work. Existing authorship
  must not be changed by selecting a different model.
- Frontend build passed. Rendered the actual component in headless WebKit and checked
  computed gaps (24/8/8 px); review HTML and PNG are in ignored `.spike-data/ui/model-selector/`.
  Native bundle not rebuilt.

## 2026-11-05 — Real agent sessions: Claude Code and Codex end to end

- Owner approved. Registered the dev CLI (`target/release/invectus`) in the real clients with
  `invectus connect add claude-code --yes` and `… codex --yes` (user scope; still connected).
  Status then read "Claude Pro subscription" and "ChatGPT subscription".
- Project: a copy of `demo.invectus` at `.spike-data/p1/work/agent-e2e.invectus` (papers, BGE
  space). App not running, so calls went to the server's own session. Transcripts:
  `.spike-data/p1/agent-e2e/{claude,codex}.jsonl`.
- Same five-step task in each, headless (`claude -p`, Claude Code 2.1.311, model
  claude-sonnet-5-5; `codex exec -s read-only`, codex-cli 0.166.0, model gpt-6): open and
  status, search + `check_idea`, `list_gaps` + `gap_detail` + `gap_context`,
  `submit_description`, `suggest_decision`.
  - Claude Code: 11 turns, 41 s, all calls succeeded. "flood resilient housing" → open,
    P(exists) 0.01, AUC 0.95. Gap 1 description passed both checks (region 9 at 0.90; P(exists)
    0.02), `stored: true`. Suggested `confirmed_gap`.
  - Codex: all calls succeeded, no shell commands. "retinal gene therapy outcomes" → exists,
    P(exists) 0.90. Gap 2 description passed (region 6 at 0.91; P(exists) 0.04), `stored: true`.
    Suggested `confirmed_gap`. ~260k input tokens (mostly cached), 1.3k output.
  - Independently with `invectus gaps show`: both suggestions are stored with provenance
    (`agent:claude-code/claude-sonnet-5-5`, `agent:codex-mcp-client/gpt-6`); both gaps' decisions
    are still unset. Both agents reported scope as "this collection" only.
- Findings: Codex identifies itself as `codex-mcp-client`, so provenance and the activity list
  show that name rather than "Codex". Fixed: the server records it as `codex`, and the app shows
  "Codex" (also for older `codex-mcp-client` records). Tool results are large (gap JSON carries 20-id `core`
  arrays and timelines); fine here but costly for long sessions.
- Not verified: live activity in the open app over the hand-off socket (needs a project open
  in the window), Claude Desktop (`.mcpb`), Cursor (not installed), interactive (non-headless)
  sessions.

## 2026-11-05 — Collapsible model selector and future-work preference

- Clicking a suggestion's model label opens the selector in gap detail and Saved. Model
  names align with their provider's text; provider headings independently collapse/expand.
  Spacing remains 8 px within groups and 24 px between providers. Escape, outside click,
  narrow viewports and saving are handled; failed saves leave the selector open with an error.
- The owner's choice is stored per project in `project.json` as `preferred_agent_model`;
  legacy projects default to none. The command checks the project root before saving.
  A “Next” line shows the request; existing suggestion authorship remains unchanged.
- MCP responses carry the saved request for future work, read afresh for existing sessions.
  **Decision:** keep the MCP-only execution model. Switching remains controlled by the
  external agent client; this preference does not launch a model or guarantee a switch.
  The listed options are requests, not a verified catalog of each client's availability.
- Verification on macOS: 117 workspace tests and 30 UI tests passed; workspace formatting
  and clippy passed; frontend build passed. Headless WebKit checks passed for indentation,
  24/8 px gaps, independent collapse, preference save, unchanged author, Escape/outside close
  and narrow viewport. Screenshots are in `.spike-data/ui/model-selector/` (ignored).
  Real client model switching is unverified; native bundle not rebuilt.

## 2026-11-05 — Current-model switch and complete GPT labels

- Replaced the reset link with a “Use agent’s current model” switch. On clears the requested
  model; off restores the last explicit selection. That selection is remembered per project
  in app storage, with an in-memory fallback. Before any explicit selection, choose a model
  from the list to leave current-model mode. Switch saves keep the dropdown open.
- ChatGPT options now say “GPT-6.1 Sol Medium/High”. Attribution formatting preserves and
  capitalizes reported Sol, Terra and Luna names and retains the GPT version hyphen; it does
  not infer a missing family or effort. No new model variants were added to the option list.
- Frontend build, 31 UI tests and the headless WebKit selector checks passed, including
  switching to current-model mode and restoring the previous choice. Native bundle not rebuilt.

## 2026-11-05 — Cursor-following help for the current-model switch

- Moved the explanation beneath the switch into a bordered tooltip shown while hovering the
  setting. It follows the pointer, stays within the viewport, hides on leave and renders
  outside the dropdown's scroll container. Keyboard focus also shows help; the switch keeps
  a screen-reader description. Touch does not produce a hover tooltip.
- Frontend build and headless WebKit selector checks passed, including cursor tracking,
  viewport bounds and hiding on leave. Native bundle not rebuilt.

## 2026-11-05 — Selected-model checkmark follows the switch color

- The last explicitly selected model retains its checkmark when current-model mode is on.
  The checkmark then uses the switch's accent color; with the switch off it uses neutral text
  color. This marks the remembered selection, not a verified model running in the client.
- Frontend build and headless WebKit checks passed: exactly one model stays checked after
  enabling the switch, its computed color matches the enabled switch, and it returns to
  neutral when disabled. Existing selector interaction checks passed. Bundle not rebuilt.


## 2026-11-05 — Compact MCP results

- Owner approved reducing repeated tool-result context. `invectus mcp` now defaults to
  compact, unindented JSON at the transport boundary, covering direct and app hand-off calls.
  Every tool advertises `detail: "full"` to recover all original fields; invalid detail values
  fail before executing a tool. CLI commands, engine results and persisted data are unchanged.
- Search hits keep identity, title, role, date, URL and a 200-character excerpt; `check_idea`
  keeps three hits while retaining verdict, calibration, scope, approximate/exhaustive flags,
  comparison counts and notes. Gap lists omit cores, timelines and verification matches' text;
  verification summary/counts remain. Gap detail retains central items and nonempty years.
  Suggested decisions return the saved suggestion rather than echoing the full gap. Status
  retains running-job data and two recent history entries. Compact item IDs use strings to
  avoid JavaScript precision loss. Other result metadata is retained where possible.
- Measured on Apple M4 Pro/macOS using the nine saved response shapes from the earlier real
  agent sessions (`.spike-data/p1/agent-e2e/shapes`): old pretty JSON 95,349 bytes, compact JSON
  25,986 bytes, **72.7% smaller** in aggregate, one response per tool. This is output size,
  not measured token usage or subscription savings; repeated calls and client caching vary.
- Local checks: 9 CLI unit tests and 6 integration tests passed, including stdio, app hand-off,
  real local BGE search, full-result fidelity and detail validation. Scoped release clippy
  (`invectus-cli`, all targets, warnings denied) and workspace fmt check passed. Linker emitted
  the existing large unwind-table warning. No CI or network model/data downloads ran.
- Unverified: fresh Claude/Codex session token counts, live activity in the desktop window,
  Windows/Linux, Claude Desktop and Cursor. No new spec conflict; computation is unchanged.

## 2026-11-05 — Live agent review and project/job guards

- Owner approved completing the desktop agent workflow. App hand-off calls now emit paired
  start/finish events with a call ID. The sidebar shimmers during an actual in-flight call;
  a project activity disclosure shows recent calls and error messages. Completed writes
  refresh gap details, Saved and readiness; region-name changes refresh map labels. Draft
  decision notes and choices survive these refreshes; stale detail/map reads are discarded.
- Accept/Dismiss check the pending suggestion's timestamp under the session lock before
  recording a response. A changed or already-settled suggestion is refused. Accept still
  records a person's decision; Dismiss leaves that decision unchanged. The original agent
  author/model and reasoning remain in the append-only verdict history (regression checked).
- A per-session transition guard spans the entire app tool call, including intervals between
  session locks. Switching projects during it is refused. MCP remembers the last app project,
  includes it in forwarded requests, and refuses implicit follow-up/status/cancel calls after
  the app changes projects or becomes unreachable. Explicitly opening a project remains valid.
  App review/table requests carry their project root and validate it after acquiring the lock.
- Cancellation now checks the displayed job ID and project atomically. MCP `cancel_job`
  requires `job_id` from `job_status`/`import_collection`; stale IDs fail without cancelling
  a replacement. App cancellation failures are shown beside the control.
- Regression coverage includes paired success/failure events, refusing switches during a
  request, stale app-project requests, stale queued review forms, replacement-job cancellation,
  changed-suggestion responses, retained provenance and project-specific UI refresh rules.
- Native verification found that gap detail never retrieved persisted descriptions; it only
  displayed a local-generation result held in React state. Added a detail read of the latest
  exact-target description in the same embedding space, written since the current region
  run. This prevents reusing text when later clustering reuses region IDs. The UI shows its
  writing, checks and agent attribution; compact MCP detail omits context-title/hash metadata.
- Native checks on Apple M4 Pro/macOS used a disposable demo copy at
  `.spike-data/p1/work/live-agent-review.invectus` (5,000 papers in the selected BGE space).
  A deterministic stdio MCP client, `live-check/test-fixture`, called the app's real socket;
  this did not consume Claude/Codex plan usage. Confirmed incoming suggestions in an already
  open gap and Saved page, preservation of a typed draft, Accept saving a person decision,
  Dismiss retaining it, and a failed tool's error in activity. The saved note was visible
  unchanged in Saved. A 5,000-row import started indexing; the native Cancel control stopped
  it at the next batch boundary, recorded `cancelled`, and re-enabled project switching.
  A persistent MCP client bound to the test project refused implicit project/job-status calls
  after a native switch to another project. Fixture replies are in ignored
  `.spike-data/p1/live-agent-review/`.
- Final local checks: 127 Rust workspace tests (including local BGE paths) and all doctests
  passed; 34 UI tests, frontend build, workspace release clippy with warnings denied, and
  workspace formatting check passed. The first overlapping app-build/test attempt failed a
  Tauri doctest dependency lookup; the sequential rerun passed. The linker still reports the
  pre-existing large unwind-table warning. No CI, push, downloads or new model evaluations.
- Windows/Linux and Claude Desktop/Cursor remain unverified. These native checks exercise
  the real desktop/MCP transport with fixture writing, not fresh Claude/Codex model sessions.
- Final macOS `.app` bundle rebuilt successfully. In that exact bundle, the existing stored
  description appeared with its author and both recorded checks. Submitting a second checked
  description through MCP changed the writing in the already-open gap without reopening it;
  a newly typed unsaved note remained intact. Native accessibility assertions confirmed all
  three (updated writing, preserved draft, visible checks). App left open on the disposable
  review project. Existing MCP processes need reconnecting to load the new tool schema.

## 2026-11-05 — Measured token savings, Claude Desktop, CI rule, later work

- **Compact results, real tokens (Claude Code only, one run each).** Same five-step task on a
  fresh demo copy (`.spike-data/p1/work/tokens-claude.invectus`), Claude Code 2.1.311 headless,
  claude-sonnet-5-5. Before (`agent-e2e/claude.jsonl`) → after (`agent-tokens/claude.jsonl`):
  tool-result text 72,142 → 23,919 chars (−67%); total input tokens 314,728 → 238,073 (−24%);
  cache writes 52,881 → 32,797 (−38%); reported API-equivalent cost $0.267 → $0.179 (−33%);
  29 s vs 40 s. The after run skipped one `project_status` call (its status came from
  `open_project`), so calls are 9 vs 10. Fixed per-turn context (system prompt, tool
  definitions) is most of the remaining input. n = 1; not a benchmark.
- Codex rerun not done: the ChatGPT plan hit its usage limit ("try again at 10:55 PM").
- **Claude Desktop:** connected with `invectus connect add claude-desktop --yes` (config file
  merged, backup kept) and launched. Its log shows `invectus mcp` started, `initialize` and
  `tools/list` succeeded. A chat session in it needs the owner (GUI); `.mcpb` install not
  tried (needs Claude Desktop's install dialog).
- **CI rule:** the repository is public, so AGENTS.md now allows owner-requested manual runs of
  `build.yml`; triggers stay `workflow_dispatch` only, and `multimodal.yml` still needs
  download approval.
- Cleanup: removed the reword backup branch and `agent-e2e.invectus`; `live-agent-review`
  kept while the app has it open.
- **Later (owner, 2026-11-05), not started:**
  1. Small local model evaluation (A8 second half): a 2–4 B GGUF model against Gemma 4 12B on
     the paraphrase calibration and description round-trip tests. Needs a download approval
     (file, source, size).
  2. Product questions (target product §12): bundle the embedding model in installers or
     fetch it on first run with consent; whether agent suggestions may ever become decisions
     automatically; pricing and licensing between the two editions.

## 2026-11-07 — GitHub star prompt

- After the 5th successful search or idea check, and 30 s without input, a bottom-right card
  asks once per launch: "Enjoying inVectus? Give it a star on GitHub". Dismissing or starring
  ends it for good (`star-prompt:*` in local storage; no storage → never asks).
- **Star on GitHub** stars in place through the person's own GitHub CLI login:
  `gh api --method PUT user/starred/rbaltodano/inVectus` (`GH_PROMPT_DISABLED=1`, 20 s
  timeout); inVectus never reads or stores a token. No network before the click (no
  "already starred" pre-check); offline mode refuses it. Without `gh`, a signed-out `gh`, or
  a refusal, the button becomes **Open GitHub** (system browser).
- CLI lookup (`connect::find_in_path`) also searches `/opt/homebrew/bin`, `/usr/local/bin`
  and `~/.local/bin` on Unix: an app opened from the Finder gets a minimal `PATH`. This also
  affects finding `claude`/`codex` for Connect an AI agent.
- Checks: 20 orchestration tests (stand-in `gh`), 34 UI tests, frontend build, clippy for
  orchestration and the app. The owner's `gh` has the `repo` scope; the repository was not
  starred, and the real star was not pressed. The card was not checked in the native window.

## 2026-11-08 — Grok Build as an agent client

- Owner approved installing xAI's Grok Build CLI (`curl -fsSL https://x.ai/cli/install.sh`,
  script read first; v1.0.77, macos-aarch64, into `~/.grok`, linked from `~/.local/bin`, PATH
  line added to `~/.zshrc`). No sudo.
- New `grok` client in Connect an AI agent (CLI `invectus connect add grok`, Settings row,
  model selector group). It registers with `grok mcp add --scope user invectus -- <bin> mcp`
  and removes with `grok mcp remove --scope user invectus`. Status reads
  `$GROK_HOME/config.toml` (default `~/.grok`) `[mcp_servers.invectus]` without running Grok;
  Grok's own `enabled = true` key is ignored, and a disabled entry counts as not connected
  (re-adding enables it, checked on 1.0.77). Grok also loads `~/.claude.json` servers, but its
  own config takes precedence (its docs), so only that entry counts. The app also finds
  `~/.grok/bin/grok` when it is not on PATH.
- Sign-in: Grok has no status command. `grok models` prints "You are not authenticated." when
  signed out (verified); otherwise it is read as signed in with no plan label (unverified: not
  signed in here).
- Provenance: Grok's MCP handshake name is `grok-shell-<server>` (`grok-shell-invectus`),
  seen from `grok mcp doctor` with a logging stand-in server; recorded as `grok`. Unverified
  for a full Grok session.
- Live: connected the owner's Grok Build to `target/release/invectus`; `grok mcp doctor
  invectus` reported handshake OK and 21 tools. No Grok model session was run (not signed in).
- Model selector lists "Grok 4.6 Medium/High" from `grok models` (grok-4.6 default, grok-4.5)
  and Grok's `--reasoning-effort`; effort support per model is unverified. Grok uses the
  generic agent icon (no xAI mark added).
- Checks (arm64 macOS only): workspace tests, clippy `-D warnings`, fmt, frontend build.

## 2026-11-08 — Light mode

- Settings → Appearance: Match system (default), Light, Dark; kept in local storage
  (`invectus.theme`). `src/theme.ts` sets `data-theme` on `<html>` before the first render and
  the native window theme (`setTheme`, new `core:window:allow-set-theme` permission), so the
  macOS sidebar material follows. The fixed `"theme": "Dark"` was dropped from
  `tauri.macos.conf.json`.
- Light palette from the website hero (`invectus-website/styles.css` `.light`): paper
  `#ebe7e0`, surfaces `#e2ddd5`/`#dad4ca`, lines `#d3cdc3`/`#bfb8ac`, ink `#1b1a18`. Raised for
  small UI text: dim 62% ink (4.5:1 on the background; the hero uses 55%, 3.7:1), accent
  `#0f7a50` (4.35:1; the hero's `#12895a` is 3.6:1), danger `#b3361f`.
- Hard-coded colours in `App.css` became tokens (same dark values). The map canvas reads the
  tokens and redraws on a theme change. Light mode uses the website's dark-text logo; the
  primary button's plus icon and the search chevron are filtered to match.
- Checks: frontend build, 34 UI tests, app clippy. Not checked visually in the native window
  or a browser.

## 2026-11-08 — Beta build fixes and release preparation

- Prepared fixes for the three failures in the previously inspected manual CI run
  `42146609686`: Tauri's macOS deployment minimum is explicitly 10.15 instead of its
  10.13 default (llama.cpp uses C++ filesystem APIs); Linux/Android use `SO_PEERCRED`
  instead of the BSD/macOS-only `getpeereid`, retaining the same-user check and refusing
  failed/incomplete credential reads; Windows connection setup respects `USERPROFILE`,
  so the CLI integration test can use a disposable profile rather than the OS account's
  real client configuration. The test also isolates APPDATA, CODEX_HOME and GROK_HOME.
- `connect bundle` now writes ZIP archives in Rust, streams the binary, preserves its
  executable permissions, chooses `invectus.exe` on Windows and replaces the output only
  after archive completion. No system `zip` or `unzip` dependency. The integration test
  reads the manifest, validates the platform entry and permissions, and checks the entire
  archived executable against the built CLI. Added locked ZIP/compression dependencies.
- Added `scripts/collect-release-notices.mjs`: offline source-license text collection with
  versions and file hashes, conservative across Rust build/test/platform dependencies and
  non-dev npm packages. Generated files stay under `target/release-notices/`. First local
  inventory: **829 packages, 61 license metadata/file review entries**, about 7 MB of
  notices. This does **not** complete binary-distribution review: prebuilt ONNX Runtime,
  native dependencies and the flagged source packages still need review.
- Added `docs/beta-release.md`: the existing consent-based embedding-model setup, manual
  upgrade procedure, artifact/signing gates, client checks and clean-machine acceptance.
  Updated CI guidance that still referred to the former private repository. BGE remains
  provisional; P2 already compared calibration/title retrieval, while the default-model
  spec conflict and broader relevance evaluation remain open. No model default changed.
- Validation on arm64 macOS 27.0.1: **130 workspace release tests passed**, including the
  connection/archive integration test and private socket tests; workspace/all-targets
  release clippy with warnings denied, formatting, all **34 UI tests**, frontend build,
  notice collection, JS/shell syntax and whitespace checks passed. Existing pinned BGE
  fixtures were present; no new LLM or retrieval-quality evaluation was performed.
- The normal `npm run tauri build -- --bundles app --ci` succeeded, including its CLI
  pre-build. The resulting macOS app is 700.28 MiB. Its Info.plist records minimum 10.15;
  the packaged CLI exactly matches the release binary and runs `--version`. `otool -L`
  reports only system libraries/frameworks for both executables (20 for the app, 11 for
  the CLI), with no development-library paths. The pre-existing large unwind-table linker
  warning remains. This is build/link verification, not a fresh-machine or native UI test.
- Freed about 4 GiB by removing the 26 completed workspace test executables after their
  successful run, to leave room for packaging; test logs remain under `.spike-data/`.
- Read-only keychain inspection found an Apple Development identity but no Developer ID
  Application identity. Distribution signing/notarization remains an owner setup step.
- No CI run, push, model/dataset download, Developer ID signing/notarization or client login.
  Windows/Linux execution, older-macOS compatibility, clean-machine installation and
  notarization remain unverified. The 10.15 deployment setting is a compilation minimum,
  not a claim of end-to-end support on that OS.

## 2026-11-08 — Owner-requested push and CI

- Pushed `main` through `3085e8b9a1d03d8ab14f47806448eabaec2b782d` and manually
  dispatched `build.yml` on that commit at the owner's request.
- Run [42532458660](https://github.com/rbaltodano/inVectus/actions/runs/42532458660)
  has started its desktop and spike builds on macOS, Windows and Linux. Results are
  pending; no additional platform verification is claimed. `multimodal.yml` was not run.

## 2026-11-08 — Website linked from GitHub

- At the owner's request, added `https://invectus.app` to the repository's About website
  field and near the top of README.md. The live HTTPS page returned HTTP 200, has the
  inVectus title, and links back to this repository. No website content changed or CI run.

## 2026-11-08 — T3-style settings, horizontal tabs and main sidebar

- Reworked Settings with seven horizontal tabs, search, app/project scope labels and
  rounded groups of rows using inVectus's light/dark tokens. The owner requested tabs
  instead of a second sidebar, then requested the original settings-sidebar styling for
  the app's main sidebar. The main navigation now uses that same solid surface, rounded
  hover/selection rows, quiet borders, compact spacing and search field. Project controls,
  activity indicators, job controls, keyboard shortcuts and the macOS titlebar drag area
  remain. Reference: T3 Code's settings route, sidebar, AppearanceSettingsPanel and
  SettingsGroup at upstream commit `920ffcdd043ef184d21fb11be7b052f628667142`.
  No upstream component code or dependencies were added.
- All existing settings remain: System/Light/Dark appearance; embedding-model status and
  consented downloads; local LLM status; all five agent clients, account status, connection
  plans/copy/apply/disconnect and recent activity; project offline switch/environment
  override notice; six gap weights/reset; both calibration modes and metadata; embedding
  identity/runtime/origin/vector details; project path. Agent explanations remain under
  “How connections work”. Panels stay mounted so changing tabs preserves pending plans
  and download confirmations without repeating account probes.
- Added searchable settings (including available model keys), Enter to open a result,
  Escape/clear, accessible tabs with Left/Right/Home/End navigation, theme preview cards,
  connection-state dots and no-project/empty-index states. Tabs scroll horizontally at
  narrow widths. Download consent points to Settings → Network. Preference values,
  backend actions and network policy are unchanged.
- Checks: frontend production build, **37 UI tests** (including three settings-navigation
  tests), and whitespace check passed. Isolated file-based Chrome renders with a fixture
  Tauri bridge covered light/dark and 800/850/1100 px widths, all settings categories,
  download/connection prompts and the selected main-sidebar project row. The 17 settings
  scenarios had no runtime errors or unexpected horizontal overflow; the extra project
  render checked the main sidebar. Eight interaction flows/16 assertions passed: theme
  persistence; search navigation/focus; tab wrapping/Home/one visible panel/no second
  sidebar; network toggle; weight change/reset; calibration request/job guard; offline
  download refusal/cancel; retained connection plans without applying. These are frontend
  fixture checks, not live client sessions or native backend writes.
- Native macOS bundle build passed on arm64 macOS 27.0.1 (700.28 MiB), with the existing
  upstream large unwind-table linker warning. The first normal build was stopped in its
  unchanged CLI pre-build. UI rebuilds reused the previously validated CLI, pinned
  MACOSX_DEPLOYMENT_TARGET=10.15 and overrode only beforeBuildCommand to `npm run build`.
  Relaunched the final bundle at the owner's request and left Settings open. Native
  accessibility confirmed the new seven-tab group, selected Appearance panel, settings
  search and current System/Light/Dark choices. No preference values were changed during
  native inspection.
- Proposed QoL preferences, not implemented: restore the last workspace/settings tab;
  text size and interface density; a reduced-motion override; local job completion/failure
  notifications; model/cache locations and disk usage; backup reminders before upgrades.
- No push, CI dispatch, model/dataset download, or real agent reconfiguration.

## 2026-11-08 — Settings tabs match project tabs

- Settings tabs now reuse the project tab styling: numbered labels, typography, spacing
  and a one-pixel indicator sized to the label. Extracted the existing measurement and
  ResizeObserver logic into `TabIndicator.tsx`, shared by project and settings tabs.
  Both use the same 200 ms transform/width/top transitions and reduced-motion rule.
- Settings reveals the selected tab when searching or navigating to a tab outside the
  visible scroll area. Existing project-tab behavior and all settings actions remain.
- Frontend build and all 37 UI tests passed. Isolated Chrome fixture checks confirmed
  label-width geometry, 200 ms transitions actually starting and moving in both tab bars,
  offscreen settings-tab reveal and the reduced-motion override; existing settings
  interaction checks also passed. Native macOS UI bundle build passed with the existing
  unwind-table warning, using the previously documented UI-only rebuild procedure.
- Relaunched the bundle and opened Settings before the owner requested no further
  computer use. Visual verification is left to the owner going forward. No preference
  values were changed, no push or CI dispatch, and no model/dataset downloads.

## 2026-11-09 — Agent client catalog plan (after T3 Code)

- Added `docs/agent-clients-plan.md`, a proposal that is not built. Phase 1 replaces the
  `Client` enum with one data-driven catalog. Phase 2 adds more MCP clients plus a custom
  entry. Phase 3 has inVectus launch agents through ACP.
- **Decision (owner, 2026-11-09):** Phase 3 waits until after launch. If it is built, build
  only the narrow version: agents already installed, no registry or `npx` downloads, no file or
  terminal access, deny other permissions by default, run in the project folder only, refuse
  under `--offline`. Agents launched by inVectus would conflict with SPEC §7.8 as written;
  update the spec before building Phase 3.

## 2026-11-09 — Agent client catalog, four new clients, custom client (plan phases 1–2)

- `invectus-orchestration::connect` now uses one table, `CLIENTS`, in place of the `Client`
  enum and its `match` arms (`docs/agent-clients-plan.md`). Each entry gives the client's id,
  name, handshake aliases, featured flag, checked flag, method (JSON file with key and entry
  shape, or the client's own `mcp add`) and account probe. The five existing clients keep their
  ids, order, commands and files. `agent::client_id` maps handshake names through the aliases:
  the same results as before for `codex-mcp-client` and `grok-shell-*`, now case-insensitive.
  Claude Desktop's `claude-ai` is still recorded as given.
- New clients, configured from their documentation and **unverified** (no session run through
  them):
  - Gemini CLI: `~/.gemini/settings.json`, `mcpServers`.
  - Kiro: `~/.kiro/settings/mcp.json`, `mcpServers`.
  - LM Studio: `~/.lmstudio/mcp.json`, `mcpServers`. The file exists on the owner's Mac.
  - VS Code: user `mcp.json` in the app settings folder, `servers`, entry
    `{type: "stdio", command, args}`.
  - Windows and Linux paths are unverified.
- Skipped:
  - opencode: the current docs show `mcp.servers`, while older configs use `mcp.<name>`.
  - Windsurf: its docs moved to Devin and give no path.
  - Zed: its settings file is JSONC.
  - Cline: its file sits in VS Code extension storage.
- **Other client:** `invectus connect add custom --file <abs .json> [--key mcpServers]` and an
  "Other client" row in Settings merge the standard entry into any JSON file. Only an absolute
  `.json` path and a plain key are accepted; files are backed up as for the listed clients.
- New `invectus connect clients [--json]` and Tauri `connect_clients`. Settings renders rows
  from the catalog: featured, found or connected clients first, the rest under "More clients",
  and an "Unverified" tag on clients not checked. Client names in the sidebar and attributions
  come from the catalog.
- Behaviour change: a JSON client now counts as found when its config file **or its folder**
  exists (before: the file only).
- Icons: Grok, Kiro and VS Code marks added from T3 Code (MIT, already credited). Grok
  previously showed the generic icon. Gemini CLI's mark is from Simple Icons (CC0) and LM Studio's
  from LobeHub Icons (MIT, `docs/licenses/LobeHub-Icons-MIT.txt`). Source credits were removed
  from code comments at the owner's request; the licence files stay.
- Checks (macOS, M-series): `cargo test --workspace --release` passed, with 6 new connect
  tests. Clippy `-D warnings`, `cargo fmt --check` and `npm run build` passed. Read-only CLI smoke
  test: `connect clients`, `connect status` (offline) and `connect add vscode` printed the
  expected plan, and nothing was written. The Settings screen was not checked visually.

## 2026-11-09 — Remember the last tab; document the client catalog

- Reopening a project restores its last section (Search, Gaps, Saved or Collections).
  The choice is per project in local storage (`invectus.tab.<root>`). A new project
  that starts on an import, and a search opened with a query, still land on that page.
  Detail pages are not restored. Settings restores its last category the same way
  (`invectus.settings.section`); a stored id that no longer exists opens Appearance.
- Settings search now matches Gemini CLI, Kiro, LM Studio and VS Code. README lists
  those clients, the other-client path, and that their sessions and Windows/Linux
  paths are unverified.
- UI tests cover the restore rules and the new search terms. Frontend build was run
  with those tests. Native window not rechecked.

## 2026-11-09 — Import and path cleanup for beta

- Column mapping no longer treats a short name as a substring. `width` and `valid` are
  not chosen as the id, and `context` is not chosen as the text. A match is the whole
  column name or one word in it (`paper_id`, `full_text`, `publication_date`).
- File names and new-project paths use the last segment of either `/` or `\`. Creating a
  project waits until a folder is set, so an empty location is not saved as `/<name>`.
- After import, “Go to the collection” opens that collection. The network switch reports
  pressed when the network is actually on.
- CLI `--offline` help and the network-gate comment now include model downloads, PubMed
  and OpenAlex checks, and client sign-in probes. Import, embedding, search, regions and
  gap scans stay local.
- Checks: 43 UI tests, `tsc --noEmit`, and `cargo fmt --check` for `invectus-core` and
  `invectus-cli`. Not checked in the native window, and not run on Windows.

## 2026-11-09 — Projects page matches Collections

- Projects are ruled rows instead of filled cards: the name in light sans, the folder in mono,
  then figures for items, collections, models and when the project was last opened
  (`src/ago.ts`). The open project carries an "Open now" accent mark; Remove sits at the end of
  the title line. Rows stagger in like gap results, and stay still under reduced motion.
  Unused card styles were removed (commit 81a40c1).
- Checks: 46 UI tests and `npm run build`. Not checked in the native window: the release
  bundle in `target/release/bundle` predates the change, and its rebuild was stopped.

## 2026-11-09 — First macOS preview release and curl installer

- Owner approved pushing the 8 pending commits (HEAD a55ec5e) and publishing a release.
  Tagged **v0.1.0** on a55ec5e: https://github.com/rbaltodano/inVectus/releases/tag/v0.1.0
  (not a prerelease, so `releases/latest` resolves to it).
- Built on the owner's Apple Silicon Mac: `cargo build --release --locked -p invectus-cli`,
  `npm run tauri build -- --bundles app`, binaries stripped, ad-hoc signed (`codesign -s -`;
  not Developer ID signed, not notarized). Archive `inVectus-macos-arm64.tar.gz`, 199,212,333
  bytes, SHA-256 `70e0e402ec97821b180c9513490e4f09bc8408abcb58e39be898a145cbb1eadd`.
- `https://invectus.app/install.sh` (in the invectus-website repo) downloads the latest
  release, verifies it against `SHA256SUMS`, installs `inVectus.app` to `~/Applications` and
  links `invectus` into `~/.local/bin`. No sudo, no model download, no agent connection.
- Verified: install, in-place upgrade, tampered-checksum refusal and the packaged CLI
  (`--version`, `connect status` lists the new clients) in an empty `$HOME`; the installed app
  process launched. Ran the live `curl -fsSL https://invectus.app/install.sh | sh` against the
  published release in a clean `$HOME`.
- **Unverified:** a fresh Mac or user account, Gatekeeper on a browser-downloaded copy
  (the curl route avoids quarantine; a browser download of the unsigned app would be blocked),
  Intel Macs, macOS older than the build host, Windows and Linux. The site says Windows and
  Linux are build-from-source; no CI run backs any claim for them.

## 2026-11-10 — Beta QA pass

- New [docs/beta-qa-log.md](docs/beta-qa-log.md): feature-by-feature test of the CLI,
  agent (MCP) tools and app wiring on the fictional Urban Futures demo and small fixtures,
  merged with the open items from beta-release.md. Native window not driven.
- Fixed: app import failed on a first run with no model (token chunking needs BGE; now
  falls back to character chunks with a note); export file names held a `:` (broke on
  Windows; now `gaps-<collection>-<run timestamp>.<ext>`); near-chance calibrations
  (AUC < 0.6) now warn that the verdict says little (the demo gave every query, even
  nonsense, "NEAR"); Describe disabled with a note when no local LLM is installed;
  missing-model and offline-verify messages name the app's settings and valid CLI flags;
  bridge slider debounced; Windows CI test failure from run 42532458660 (Unix file-mode
  assertion on a Windows-built zip) — fix unverified until CI runs again.
- Checks (arm64 macOS 27.0.1): 136 workspace release tests, clippy `-D warnings`, fmt,
  `tsc`, frontend build, 46 UI tests. No push, CI dispatch or download.

## 2026-11-10 — Integrations page (UI)

- New sidebar page **Integrations**, above Projects. It holds AI agent connections (moved
  out of Settings, which no longer has an "AI agents" tab) and a **Databases** section marked
  "Not available yet" that points to CSV/TSV/Parquet import.
- Sidebar project rows lost their folder icons; names are indented to line up with "Projects".
- **Spec conflict to review:** importing from an outside database is not in SPEC v1 (v1 has
  file and table import; connectors beyond PubMed and Steam are post-v1). A remote database
  source would also be network use and must follow the explicit-network rule. The section is a
  placeholder only.
- Checks (arm64 macOS): `tsc`, frontend build, UI tests. Native window not checked.

## 2026-11-10 — Projects page as a compact table (UI)

- The Projects page lists projects as a compact table (Name with folder beneath, Items,
  Collections, Models, Opened) instead of tall ruled rows with a figures strip. The whole row
  opens the project; Remove appears on hover; the open project shows an accent "Open" tag.
  Collections and Models columns hide below 760 px.
- Verified: `npm run build` passes. Not yet checked visually in the running app.

## 2026-11-10 — Data-source integrations: Excel, Google Sheets, Notion

- Plan and details: [docs/integrations.md](docs/integrations.md). Owner decisions: Sheets by
  shared link now (Google sign-in later), Notion by pasted internal token, Notion databases
  (not page trees) first.
- Engine: `Source::Excel` (calamine 0.38, local), `Source::GoogleSheet` and `Source::Notion`
  read a project snapshot under `sources/`; `invectus_ingest::remote::pull` is the only network
  step (gated by `net::ensure_online`; callers check the project network switch).
  `invectus_core::secrets` keeps the Notion token in the OS keychain (keyring 3).
- CLI: `invectus import` takes spreadsheets and `--sheet`; new `invectus pull` and
  `invectus integrations {status,notion-token,notion-disconnect}` (later renamed `token|disconnect <name>`).
- App: import screen reads spreadsheets (sheet menu) and pulled sources (Pull button, job
  progress); Collections "Pull and update"; Integrations page lists Excel, Google Sheets, Notion
  (token dialog) and a "Databases: coming later" card above the AI agent cards.
- Agents: `inspect_source` / `import_collection` accept `sheet` and pulled-source specs (they read
  an existing pull; agents cannot start a pull).
- **Unverified:** Google Sheets and Notion pulls have not run against the live services (no test
  sheet or Notion workspace used). Keychain storage not exercised in tests (needs a session
  keychain). Linux kernel-keyring persistence and Windows Credential Manager untested.
- **Spec conflict to review:** outside data connectors are beyond SPEC v1 (file/table import
  only; connectors beyond PubMed and Steam are post-v1). They follow the explicit-network rule.

## 2026-11-10 — Zotero and Airtable sources; integrations roadmap

- Roadmap of all proposed integrations (data sources, outside checks, exports) added to
  [docs/integrations.md](docs/integrations.md), at the owner's request.
- `Source::Zotero` (Web API v3, API key; library, collection or group) and `Source::Airtable`
  (personal access token; table or view), pulled into project snapshots like Notion. Token
  handling is now generic: keychain names `notion`, `zotero`, `airtable`; CLI
  `invectus integrations token|disconnect <name>`; Integrations page cards for each.
- **Unverified:** neither service was called (no test accounts). Unit tests cover link parsing,
  spec round trips and value flattening.

## 2026-11-10 — Typeform, Tally, Canny sources; Semantic Scholar and arXiv checks

- Data sources `Source::Typeform`, `Source::Tally`, `Source::Canny` (tokens `typeform`,
  `tally`, `canny` in the keychain), pulled into project snapshots; Integrations cards and
  import hints for each. Canny boards resolve by id, URL name or name.
- Outside checks: `invectus_verify::Source::{SemanticScholar, Arxiv}`; per-project choice
  (`ProjectConfig.verify_sources`, Settings → Network checkboxes, `set_verify_sources`); gap
  detail button names the chosen sources. Defaults unchanged (PubMed, OpenAlex).
- **Unverified:** none of these services was called. Tally's answer value formats and Semantic
  Scholar's bulk-search syntax are taken from their documentation, not from live replies.
- **Spec conflict to review:** extra verification connectors are post-v1 in SPEC ("every
  connector beyond PubMed and Steam is post-v1"); they are opt-in and off by default.

## 2026-11-10 — Productboard source; US patents (PatentsView) check

- `Source::Productboard` (`productboard:notes` or `productboard:features`, access token) pulled
  into project snapshots; Integrations card.
- `invectus_verify::Source::PatentsView`: USPTO PatentSearch API with a free key (keychain
  `patentsview`), opt-in per project ("US patents" in Settings → Network); Integrations page has
  a new "Outside checks" section.
- **Unverified:** neither API was called; PatentSearch query and reply shapes and Productboard
  paging come from their documentation.

## 2026-11-10 — Live checks of the integrations (no accounts); US patents removed

Run from this Mac (arm64 macOS) with the release CLI on scratch projects; owner away, so no
real accounts were used.

- **Google Sheets, verified live:** Google's public sample sheet
  (`1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms`, gid 0) pulled 30 rows; the snapshot and
  `.json` record were written; import under `--offline` read the snapshot (30 items). Refused
  with the network switch off, refused with `--offline`; a missing sheet gave the "no sheet at
  this link" error. A private (unshared) sheet was not tried.
- **arXiv and Semantic Scholar, verified live:** `gaps verify --sources arxiv,semanticscholar`
  on a copy of `demo.invectus` (gap 1) returned 4,423 and 59,487 matches, 20 fetched each,
  re-ranked locally (29 s, mostly arXiv's 3 s spacing). arXiv years match the feed's first-version
  dates.
- **Token services, endpoints reached with fake tokens:** Notion, Zotero, Airtable, Typeform,
  Tally, Canny and Productboard each answered with an auth error that maps to the intended
  message; a missing token gives the "not connected" message. Successful pulls (real data and
  paging) remain **unverified** for all seven.
- **Keychain (macOS), verified:** `integrations token zotero` refused a malformed key, saved a
  valid-looking one (visible to `security find-generic-password`), the pull used it, and
  `integrations disconnect zotero` removed it.
- **US patents removed:** `search.patentsview.org` does not resolve; PatentsView's search API was
  shut down on 2026-03-20 when it moved to the USPTO Open Data Portal. The PatentsView check was
  taken out the same day it was added. The ODP replacement is on the roadmap (its docs pages are
  JavaScript-only and could not be read here).
- Fixed: the Zotero refusal message wording; the CLI ends its progress line before an error.

## 2026-11-10 — Agents page

- AI agent clients moved off Integrations to their own sidebar page, **Agents** (under
  Integrations): client cards, more clients, how connections work, recent agent activity.
  Integrations now holds data sources and outside checks only. Checked: `tsc`, frontend build,
  46 UI tests; rendered in a browser preview with this Mac's real client list (native window
  not captured: no screen-recording permission in the agent session).

## 2026-11-10 — v0.2.0 beta released (macOS, Windows, Linux)

- **Windows and Linux verified in CI.** `build.yml` run 42907902558 on `7defae2`: full test
  suite, clippy and fmt pass on macOS, Windows and Linux. Earlier Windows-only failures fixed
  on the way (zip unix mode, DuckDB double-open in the app's `open_project`, unescaped paths in
  TOML fixtures, an unused `mut`).
- **Installers verified in CI.** `release-build.yml` run 42932092867 on `release/0.2.0`
  (`ab95fc6` = `7defae2` + model-smoke data fix): NSIS (Windows), .deb + AppImage (Ubuntu
  24.04), macOS app. Each installed on a fresh runner; the installed CLI ran offline checks,
  downloaded bge-small-en-v1.5 and ran import → embed → search → regions → gap scan.
- **macOS signed and notarized.** The CI-built app from that run, signed with Developer ID
  (team 7XG2DHY989), notarized (submission accepted), stapled; `spctl`: "Notarized Developer
  ID". `scripts/sign-macos.sh`.
- **Release:** GitHub release v0.2.0 from branch `release/0.2.0` (the integrations work on
  `main` after `7defae2` is not in it; its own `release-build.yml` run 42931420826 on `785a027`
  passed on all three OSes). invectus.app now links the Windows and Linux installers;
  `install.sh` installed v0.2.0 end to end.
- **Unverified / limits:** Windows installer unsigned (SmartScreen warning); Linux tested on
  Ubuntu 24.04 only; Intel Macs not built; no clean-machine test by a person yet.

## 2026-11-11 — Home page, New project, sources, integration marks

- **Home** is the default page and the first sidebar item. The last project opens quietly
  (not counted as opened until entered). A Continue card shows a slice of its data circle
  (centre on the card's right edge, radius 60% of its width) with readiness, then a camera tour
  to a random top-12 gap with a card that opens it; Top gaps (code, label, the engine's
  explanation, demand sparkline per year, score); Recent projects. Display only: map positions
  decide nothing.
- **Sidebar projects** open on one click anywhere on the row; double-clicking the name renames.
- **Integrations** cards show each service's mark (Simple Icons CC0, Excel from v10.0.0;
  Tally, Canny, Productboard from their sites; Typeform's symbol redrawn).
  `docs/licenses/Simple-Icons-CC0.txt`.
- **New project:** the heading is the name field; Project destination with a folder picker;
  Import existing data shows built-in sources and connected integrations only, several can be
  chosen (link sources ask in a dialog), and they import one after another; Agent for this
  project lists connected agents and the on-device model, labelled Cloud or Local, defaulting
  to the last chosen, else the last active, else on-device.
- **Add to Dataset** opens the same source picker; Collections show each collection's source.
- **Per-project agent:** `project.json` gains optional `agent` (a client id or `on-device`),
  set through `set_project_agent`. Stored and shown only; nothing routes work by it yet.
  *Spec note:* SPEC §7.8 describes agent and desktop editions but no per-project agent choice;
  recorded here for review.
- Checks: `tsc`; `cargo fmt --check`; `invectus-core` project-config test (agent field) passes;
  release app bundle built and launched on this Mac (arm64). Screens checked in headless
  Chrome with a stand-in backend and the real gapmap-full map export; the native window was
  not captured (no screen-recording permission). Not run: full workspace tests, clippy, CI.

## 2026-11-11 — UI iteration (agent pill, dropdowns, Import table view)

- Project header and New project: an agent pill (connected agents and the on-device model,
  "Add new agent" opens the connect screen); names and marks flicker on change. Offline pill
  removed from the header (the network switch stays in Settings).
- All native dropdowns replaced by one custom Select with per-option detail.
- Cards (integrations, agents, sources) flicker in after random per-card delays
  (`useFlickerDelays`). Shimmer tick 120 → 85 ms (also on invectus.app).
- Sidebar: finished jobs leave; a project's running job shimmers under its name.
  Home gap cards show on hover only. Pulls show the indexing bar (a percentage only where the
  source reports a total: Notion pages).
- Import table view (`TablePicker`): everything is imported; "Choose searched columns" picks the
  import's text columns (several are joined, already supported by the engine); labels set title,
  ID, date, URL or the roles column. Before importing it shows: the roles (never guessed: the
  owner chose to ask, since demand and coverage depend on the collection), rows the preview
  would skip for empty text, empty or repeated
  IDs in the sample, and updates to an existing collection. With every row neutral the button
  says "without roles". Counts come from the preview of the first rows (estimates).
- Dev mode: `scripts/live.sh` (hot reload) and a Live tag beside the logo.
- Checks: `tsc`, 46 UI tests; screens checked in headless Chrome with a stand-in backend.
  Not tested end to end: a real multi-source import, a real "Import all rows".

## 2026-11-12 — Integrations: column defaults, and choosing what to pull

- Import opens each integration with its own columns set (`src/source-defaults.ts`): Zotero,
  Canny, Productboard notes and features, Typeform and Tally (open-text answers only), Notion and
  Airtable (fixed id/date/url columns, the rest guessed by name). Roles are still not guessed.
- Source dialogs list what a connected token can pull, so no link is needed: Notion databases
  (`POST /search`), Zotero libraries and collections (tree order, item counts, group libraries),
  Airtable tables and their other views (needs the `schema.bases:read` scope; first 40 bases),
  Typeform and Tally forms, Canny boards (plus all boards), Productboard notes or features
  (no network). `invectus_ingest::remote::browse`, Tauri `source_choices`. Each choice is the
  same spec a pull takes; tests parse every listed spec back. The link field stays as a fallback.
- Checks: 4 Rust tests on recorded answer shapes, 5 defaults tests, 51 UI tests, `tsc`, clippy.
  Not tested against the real services: answer shapes follow each API's documentation.

### Decisions and spec conflicts to review

- Listing is a network call outside the per-project switch (SPEC §7.7: the switch enables
  connectors). It runs only when the person clicks "Show my … " in the dialog (New project has
  no project yet to hold a switch), offline mode refuses it, and nothing is pulled. The pull
  itself still needs the project's switch. Owner to confirm, or gate listing on the switch too.

### Integration check without accounts (2026-11-12, arm64 macOS)

- Real: CSV, TSV, Excel and folder imports through the CLI (skipped rows by reason, duplicate
  ids, repeated header names, Unicode, quoted newlines, idempotent re-import). Google Sheets:
  a public sample sheet pulled (30 rows) and imported; offline mode and the network switch refuse
  the pull; a missing sheet says so.
- Stand-in servers (`crates/ingest/src/remote/stand_in.rs`, test builds only; requests are
  routed there by a test-only hook): Notion, Zotero (own and group libraries, collections),
  Airtable (views, offsets), Typeform (1,002 responses over two pages), Tally, Canny (board by
  name, all boards), Productboard notes and features, and Google Sheets private and missing
  sheets. Each test runs the whole pull (paging, retries after 429, refused tokens, the CSV
  written) and the listing. Answers follow each API's documentation, not recordings.
- The pulled CSVs then imported through the CLI with the app's column defaults, and one was
  embedded and searched.
- Fixed on the way: Productboard notes now list linked features by id (the API gives no names);
  Canny listing reports a refused key like the pull does; a table whose column names do not say
  which holds text now opens with its prose columns searched (it opened with Import disabled).
- Not checked against the real services: Notion, Zotero, Airtable, Typeform, Tally, Canny,
  Productboard (no tokens on this Mac); Google Sheets private sheets (real redirect to sign-in).

## 2026-11-12 — Case study

- Added `docs/case-study.md`: decisions, measurements and limits, drawn from this ledger (no new
  measurements). Linked from the README's Engineering choices section.

## 2026-11-12 — Refreshed screenshots

- Recaptured native macOS window screenshots from a release build of commit 7c76d45 (bundled
  frontend), Ophthalmology Research project (5,000 papers), network off. Replaced the
  ophthalmology search, gap, item and collections images; added home, gaps list, new project,
  integrations and settings (network) images to the README. EXIF/XMP chunks stripped.
- Saved still uses the older Urban Futures demo capture: the ophthalmology project has no saved
  gaps or decisions, and none were created for the screenshot. The Agents page was not captured
  because it shows the signed-in account email.
