# inVectus — Technical Specification

**Version:** 0.1 (draft, 2026-10-26)
**Owner:** Ryan Baltodano
**Working name:** inVectus (product); **unSearch** (the core "negative search" interaction)
**Platforms:** desktop: macOS, Windows, Linux
**Stack:** Tauri 2 (Rust core) + React + TypeScript front end

---

## 1. Purpose

Search engines answer *"what exists?"* inVectus answers the opposite question: **"what is missing?"**

You give it a collection: papers, games, products, support tickets, patents or notes. It embeds every item into a vector space and then:

1. **Finds** gaps: places where the collection *should* have something and doesn't.
2. **Fills** them: describes what would sit in each gap.
3. **Verifies**: checks the collection, and optionally outside sources, before claiming that something doesn't exist.

The interaction at the centre is **unSearch**. You type an idea, and instead of a list of matches you get a verdict: *does it exist, how close is the nearest thing, and which way does the gap point?*

### 1.1 Origin and evidence

The approach comes from the Optometry Evidence Gap Map (`~/Documents/Medical Paper Vector Database`, package `gapmap`). That prototype:

- embedded 591,484 PubMed abstracts locally, clustered them into 656 topics, and ranked topics by "primary studies since the last systematic review";
- produced a ranked list that, after manual verification, held 3 real open review topics out of 25 (chalazion treatment, lamina cribrosa morphology in glaucoma, acute corneal hydrops);
- showed that **the weak point is coverage detection.** The prototype missed existing reviews in several topics (posterior capsule opacification is the clearest case), and the human verification step found them.

A first **midpoint experiment** on the same data (224 eligible topics, 228 candidate bridge pairs, top 14 inspected) showed three failure modes, and the design below addresses all of them:

- **Most midpoints already exist.** The papers nearest the midpoint were often real, on-topic work: GLP-1 drugs × diabetic retinopathy, myopia-control contact lenses, IOL power after LASIK. A low "bridge count" between two clusters doesn't mean nothing is there. Bridging items are often less similar to *both* centroids, so a two-threshold rule misses them. **The density / novelty check at the midpoint (§6.3 step 4) is mandatory, not optional.**
- **Generic collapse.** Near-identical parent and child topics (congenital glaucoma × glaucoma) produced midpoints whose nearest items were titled just "Glaucoma." Exclude hierarchy-related pairs and down-weight generic items.
- **False bridges from shared vocabulary.** Two genetics topics (CYP1B1 glaucoma × retinitis pigmentosa) scored as close because of words like "mutation" and "variants", not shared meaning. Bridge candidates need a relevance or semantic check beyond centroid similarity.

inVectus generalizes that pipeline to any collection and puts the lessons at its core: **gap claims must carry evidence and a confidence, and they must be checkable.**

### 1.2 Goals

- Import a collection (files, tables or connectors) and build a local vector index.
- unSearch: a free-text query returns an **absence verdict** with evidence.
- Gap scan: automatically rank gaps of several types (§5), each with a plain-language explanation.
- Midpoints: generate a description of what would fill a gap, and check that the description itself is novel (§6.3).
- Verification: check candidate gaps against the collection and against outside sources (PubMed, OpenAlex, Steam, …).
- A visual map of the collection with gaps as a layer.
- Measure accuracy with **backtests** (§9): freeze the collection in the past and test whether the tool predicts which gaps were later filled.
- Local-first and private by default.

### 1.3 Non-goals (v1)

- Not a general search engine or a RAG chatbot. Positive search exists only to support verification.
- Not a cloud service. There are no accounts and no server; optional cloud LLM calls happen only on explicit opt-in.
- Not an idea generator detached from data. Every suggestion is grounded in the user's collection.
- Not mobile. Angrove covers the phone.

### 1.4 Success criteria

1. On the optometry benchmark (§9.2), the gap score beats random and "raw item count" baselines on backtest precision@10.
2. unSearch absence verdicts are **calibrated**: among queries the tool calls "exists (high confidence)", at least 90% have a true match on manual review.
3. A collection of 100k short items imports, embeds and indexes on a 2023+ laptop in under 30 minutes (CPU), and unSearch answers in under 300 ms.
4. Builds and runs on macOS, Windows and Linux from one codebase.

---

## 2. Users and first use cases

| Segment | Collection | Demand signal | Coverage signal | Output |
|---|---|---|---|---|
| **Medical students / residents (beachhead)** | PubMed query | Primary studies | Systematic reviews | Feasible review topics |
| Pharma / HEOR consultants | PubMed + trials | Trials, studies | Reviews, guidelines | Evidence gap analysis |
| Game / app designers | Steam / app-store catalogue + reviews | Player reviews, tags | Existing games | Unmade game concepts |
| Product teams | Catalogue + customer reviews | Complaints, requests | Products | Unmet needs |
| Writers / creators | Search queries, comments | What people ask | Published content | Topics nobody covers |
| Individuals | Personal notes or files | (none) | (none) | Bridges between your own ideas |

The architecture stays domain-general. v1 ships connectors and presets for **research (PubMed/OpenAlex)** and **games (Steam)**, plus generic file and table import.

---

## 3. Core concepts

| Concept | Definition |
|---|---|
| **Project** | One workspace on disk (§7.4). Holds one or more collections. |
| **Collection** | A set of **items** from one source, with a schema mapping (text, title, date, URL, role, metadata). |
| **Item** | One unit of meaning: a paper, game, review, ticket or note. Long documents are split into **chunks**, and both chunks and parent items have vectors. |
| **Role** | What an item *is* in gap terms: `demand` (something asking or needing), `coverage` (something answering or satisfying), or `neutral`. Assigned by schema mapping, rule (for example pub type = Systematic Review → coverage), or a small classifier. |
| **Region** | A cluster of items (two levels: broad and fine) with a label, centroid and statistics. |
| **Gap** | A scored, explained claim that something is missing. Types in §5. |
| **Absence claim** | The evidence behind "this doesn't exist": nearest items with distances, local density, calibrated confidence and verification results. Every gap and every unSearch verdict carries one. |
| **Midpoint** | A point in vector space (between regions or items) plus a generated description of what would sit there. |
| **Verdict** | A human decision on a gap: `open`, `confirmed_gap`, `already_exists`, `not_viable`, `selected`, with a note. Verdicts survive re-runs. |

---

## 4. User workflows

1. **Create a project → import a collection.** A wizard picks the source (folder, CSV/Parquet/JSON, PubMed query, Steam), maps columns to text, title, date, role and URL, and previews 20 items.
2. **Index.** A background job embeds, indexes and clusters, with progress, cancel and resume.
3. **Map.** A 2-D map of all items, coloured by region, role or date, with gaps overlaid as markers.
4. **unSearch.** Type an idea and get a verdict card (§6.1). Save it as a gap if interesting.
5. **Gap scan.** A ranked list of gaps by type, with filters and an explanation per row.
6. **Inspect a gap.** Evidence, nearest items, the regions on each side, the midpoint description, verification results, and the verdict buttons and note.
7. **Verify.** Run connectors (one click per gap, or batch), and see what outside sources found.
8. **Export.** Markdown, HTML or CSV report with titles and links only. Never export raw source text unless the user asks; much source text is copyrighted.

---

## 5. Gap types

Each type is a function of the vector space plus item roles and dates. All of them produce a score, the inputs to that score, and a template explanation.

### 5.1 Coverage gap (from gapmap "synthesis debt")
A region with many **demand** items and few or old **coverage** items.

```
last_coverage(r)    = max(date of coverage items covering r)
demand_since(r)     = count(demand items in r with date > last_coverage)
coverage_ratio(r)   = demand(r) / (1 + coverage(r))
```

"Covering r" includes coverage items **near** r, not just inside it: among a coverage item's 2 nearest region centroids, those where its similarity is at least the region's 25th-percentile member similarity. Gapmap showed that reviews often sit in a neighbouring cluster.

### 5.2 Stale coverage
Coverage exists, but demand has grown a lot since. This is the same inputs as 5.1, flagged when `demand_since / demand_total` is high. Use case: "which reviews or docs are out of date?"

### 5.3 Bridge gap
Two **active** regions that are close in space but have few items between them.

```
sim(a,b)       = cos(centroid_a, centroid_b)
bridge(a,b)    = #items with sim to a ≥ thr_a AND sim to b ≥ thr_b
bridge_ratio   = bridge(a,b) / sqrt(size_a · size_b)
score          = sim(a,b) · (1 − bridge_ratio) · (1 + momentum_a + momentum_b)
```

Exclude near-duplicate sibling regions (similarity above a high cutoff), which are usually one topic split in two.

### 5.4 Query void (unSearch)
A query point, or a cluster of logged queries, with no coverage item within the calibrated "same thing" distance (§6.2). With an imported search log, the tool ranks voids by how often people searched for them.

### 5.5 Density hole (exploratory only)
Low-density regions inside the convex hull of active regions. In high dimensions most of the space is empty, so this type is **off by default** and shown only as an exploratory layer, never ranked alongside 5.1–5.4.

### 5.6 Momentum and feasibility modifiers
- **Momentum:** Poisson-regression slope of yearly item counts over a recent window, plus acceleration (as in gapmap §8.2).
- **Saturation penalty:** new coverage in the last N months, a heavy down-weight.
- **Relevance filter:** a project-defined rule (journal list, tag, metadata match) that keeps off-topic regions out of rankings without deleting them. Gapmap's broad scope pulled in coronary OCT and pigeon studies until it had one.
- **Feasibility (domain plug-in):** for research, the share of RCTs and comparative studies and outcome comparability; for games, review volume. Optional, and provided by presets.

### 5.7 Composite score
A per-type z-scored weighted sum, with weights stored in the project and tunable through backtests (§9). The explanation lists each term's contribution in plain language.

---

## 6. Algorithms

### 6.1 unSearch verdict
Input: query text `q`, an optional collection filter and an optional role filter (usually `coverage`).

1. Embed `q` with the collection's model (using that model's query prefix, if any).
2. Retrieve the top-k nearest items from the ANN index, then re-score them exactly against stored vectors.
3. Compute `d1` (nearest distance), the gap `d1 → d10`, and the local density around `q`.
4. Map `d1` to **P(exists)** with the collection's calibration curve (§6.2).
5. Find the regions nearest `q`: "your idea sits between *region A* and *region B*."
6. The verdict is `exists` / `near` / `open`, plus P(exists), the evidence list and a suggested next step ("verify externally", "save as gap", "see midpoint").

### 6.2 Calibration: what distance means "the same thing"?
Raw cosine distances mean different things for different models and collections, so each collection gets its own calibration curve:

- **Positive pairs** (should count as "exists"): an LLM paraphrase of a sampled item's title or summary, and the original item. 200–500 pairs.
- **Negative pairs** (shouldn't count): the paraphrase against the item's 20th–50th nearest neighbours, which are related but not the same.
- Fit isotonic regression on distance → P(same) and store it per collection and model. Re-fit when the model or collection changes.
- Without an LLM, fall back to title/abstract self-similarity splits, with lower quality, flagged in the UI.

### 6.3 Midpoint generation and the round-trip check
1. Pick the endpoints: two region centroids (bridge gaps), two items the user picks (Angrove-style), or a query void and its nearest region.
2. Interpolate along the great circle (slerp) for normalized vectors; a weight `t` lets the user slide between the ends.
3. **Decode:** retrieve the nearest items to the midpoint and to each endpoint. Prompt the LLM with those titles and short summaries to describe the item that would sit at the midpoint (title plus 3–5 sentences, in the collection's genre: a study, a game, a product).
4. **Round-trip check:** embed the generated description.
   - *On target:* it should land closer to the midpoint than either endpoint centroid. If not, the description drifted, so regenerate (up to 3 tries).
   - *Novel:* run unSearch on it. If P(exists) is high, the idea already exists, so report the match instead of a gap.
5. Store the midpoint with endpoints, `t`, the description, both check results and the model ids used.

### 6.4 Regions (clustering)
- kNN graph on item vectors (HNSW or IVF from the index, k≈30, cosine).
- Fine regions: **HDBSCAN** on a low-dimensional projection, or **Leiden** community detection directly on the kNN graph, chosen by the spike in §11 P0. Target region sizes are set per collection.
- Broad regions: Ward agglomeration of fine-region centroids (gapmap approach; the hierarchy always nests).
- Noise: attach to the nearest region and flag it; flagged items are excluded from demand counts by default.
- Stability: multi-seed runs and best-Jaccard matching. Only regions with stability ≥ 0.5 can produce ranked gaps.
- Labels: c-TF-IDF top terms, metadata lift (for example MeSH or tags), and an optional LLM label cached per region.

### 6.5 Map layout
2-D projection for display only (never used to decide gaps): UMAP or an equivalent, computed in the background, cached and re-used. Fallback for very large collections: a projection of region centroids plus a local layout within regions.

### 6.6 Verification connectors
A connector is a plug-in with `search(query, filters) -> [ExternalHit]` and a cache. In v1:

| Connector | Use | Notes |
|---|---|---|
| PubMed (E-utilities) | Reviews and studies | Rate limits (3/s, 10/s with key), retry with backoff; port from `gapmap.ingest.client` |
| OpenAlex | Papers across fields | CC0 metadata |
| Steam store search | Games | Public endpoints, cached |
| Local folder / other collection | Cross-check against another project | Offline |
| Web search (optional) | Anything | User-supplied API key; off by default |

The verification result is a list of hits with similarity to the gap description, re-ranked with the local embedding model, plus a summary ("3 external items look like matches; the closest is X, 2025"). Gapmap lesson: **keyword search plus a human read beats similarity alone.** The UI always shows titles and links for a person to judge.

---

## 7. Architecture

### 7.1 Technology choice

**Tauri 2 + Rust core + React/TypeScript UI.**

| Option | Verdict | Why |
|---|---|---|
| **Tauri 2 (Rust) + React + TS** | **Chosen** | Small native binaries on macOS, Windows and Linux; the Rust core does the heavy vector and ML work at native speed; the React UI uses the web ecosystem (WebGL maps, component libraries). React + TypeScript + Rust is a strong résumé combination. |
| React Native (macOS / Windows) | Rejected | No Linux, desktop support lags mobile, native modules for ML are hard to write and ship, and WebGL data-viz libraries don't run in it. |
| Electron + Node | Fallback | Mature, but heavy (~150 MB+), and Node is a poor host for embedding and ANN workloads without native add-ons anyway. |
| Python (PySide/Qt) | Rejected | Fastest to prototype (gapmap is Python), but cross-platform packaging of torch and native wheels is painful, and it adds no React or Rust experience. |

The core is a **Rust library with a CLI** (`invectus`), and the desktop app is a thin shell over it. That keeps the engine testable without the UI, allows scripted benchmarks, and leaves room for a server build later.

### 7.2 Components

```
┌──────────────────────── Tauri app ─────────────────────────┐
│  React UI (TypeScript, Vite)                               │
│   Projects · Import wizard · Map · unSearch · Gaps · Gap   │
│   detail · Verify · Export · Settings                      │
│        │ invoke(commands)            ▲ events (progress)   │
├────────┼─────────────────────────────┼─────────────────────┤
│  invectus-app (Tauri commands, job runner, state)          │
├────────────────────────────────────────────────────────────┤
│  Rust workspace (crates)                                   │
│   core      data model, project store, schema mapping      │
│   ingest    files, tables, chunking, connectors (import)   │
│   embed     ONNX Runtime models, batching, caching         │
│   index     LanceDB tables, ANN, exact re-score            │
│   regions   kNN graph, clustering, labels, stability       │
│   gaps      gap types, scoring, explanations               │
│   unsearch  verdicts, calibration                          │
│   midpoint  slerp, decode prompts, round-trip check        │
│   llm       local (llama.cpp / Ollama) + optional cloud    │
│   verify    external connectors, caching                   │
│   eval      backtests, benchmarks, metrics                 │
│   cli       `invectus` binary over the same crates         │
└────────────────────────────────────────────────────────────┘
          │                      │                     │
   project.duckdb         vectors/ (LanceDB)      models/ (app data)
```

### 7.3 Key libraries (to confirm in the P0 spikes)

| Need | Candidate | Notes |
|---|---|---|
| Desktop shell | `tauri` 2.x | Commands, events, file dialogs, auto-update later |
| Metadata / analytics | `duckdb` (bundled) | SQL, Parquet import and export, matches gapmap |
| Vectors / ANN | `lancedb` (Rust) | Embedded, file-based, IVF-PQ / HNSW; same store as gapmap |
| Embeddings | `ort` (ONNX Runtime), possibly through `fastembed` | CPU everywhere; CoreML, DirectML or CUDA execution providers when available |
| Default model | nomic-embed-text-v1.5 (ONNX), bge-small-en-v1.5 as a fast option | Apache-2.0 / MIT; the gapmap benchmark chose nomic |
| Clustering | `hdbscan` crate or a Leiden implementation; `linfa` for k-means and Ward | Spike: quality on the gapmap data vs the Python reference |
| 2-D layout | Rust UMAP crate, or an optional Python sidecar | Spike: maturity and speed; this is the riskiest library gap |
| Local LLM | `llama-cpp-2` (GGUF) or the Ollama / LM Studio HTTP API | Used for labels, midpoint descriptions and calibration paraphrases |
| Cloud LLM (opt-in) | Anthropic Messages API | Off by default; per-project toggle; shows what is sent |
| Async / parallel | `tokio`, `rayon` | Jobs, batching |
| Front end | React 18, TypeScript, Vite, TanStack Query, Zustand | |
| Map rendering | deck.gl `ScatterplotLayer` (WebGL) | Handles about 1M points |
| UI tests | Vitest, Playwright (through `tauri-driver`) | |

### 7.4 Project storage

```
<Projects folder>/<name>.invectus/
├── project.json          # version, collections, model ids, weights, settings
├── project.duckdb        # items, regions, gaps, verdicts, jobs, calibration
├── vectors/              # LanceDB tables (one per collection × model)
├── cache/                # connector responses, LLM outputs (hash-keyed)
└── exports/
```

Models live in the OS app-data folder, shared across projects, each with a pinned id and checksum. The default Projects folder is `~/inVectus Projects` (user-chosen). The app warns when it sits inside an iCloud, OneDrive or Dropbox folder, because offloaded files break the databases. Gapmap hit this risk.

### 7.5 Data model (DuckDB)

```sql
items(item_id BIGINT PK, collection_id TEXT, source_id TEXT, title TEXT, text TEXT,
      url TEXT, date DATE, role TEXT, meta JSON, parent_id BIGINT, input_hash TEXT)
regions(region_id INT, run_id TEXT, level TEXT, parent_id INT, label TEXT,
        terms TEXT[], size INT, stability DOUBLE, centroid FLOAT[])
item_regions(item_id BIGINT, region_id INT, run_id TEXT, prob REAL, is_noise BOOL)
calibration(collection_id TEXT, model_id TEXT, curve JSON, fitted_at TIMESTAMP, quality TEXT)
gaps(gap_id TEXT PK, run_id TEXT, type TEXT, region_ids INT[], query TEXT,
     score DOUBLE, terms JSON, explanation TEXT, absence JSON, created_at TIMESTAMP)
midpoints(midpoint_id TEXT, gap_id TEXT, endpoints JSON, t REAL, description TEXT,
          on_target BOOL, novel_p REAL, models JSON)
verifications(gap_id TEXT, connector TEXT, query TEXT, hits JSON, summary TEXT, at TIMESTAMP)
verdicts(gap_id TEXT PK, status TEXT, note TEXT, updated_at TIMESTAMP)
jobs(job_id TEXT PK, kind TEXT, params JSON, state TEXT, progress REAL, checkpoint JSON)
```

`run_id` is a hash of the parameters, so different settings never overwrite each other (gapmap convention). Verdicts are keyed on stable gap ids and survive re-runs.

### 7.6 Jobs
Long work (import, embed, cluster, scan, verify) runs as **resumable jobs**: checkpointed in `jobs`, cancellable, idempotent, with progress events streamed to the UI. The embed job skips items whose `input_hash` is already stored (gapmap pattern).

### 7.7 Privacy and network
- **Network is off by default** after model download. A visible per-project switch enables connectors and the cloud LLM, and every outbound call is logged in the UI.
- An offline mode hard-fails any network call (like gapmap's `--offline`).
- Exports include titles and links only, unless the user chooses otherwise.

---

## 8. User interface

| Screen | Contents |
|---|---|
| Projects | Recent projects, new project, storage-location warning |
| Import wizard | Source → mapping (text, title, date, role, URL) → role rules → preview → index |
| Map | WebGL scatter; colour by region, role, date or gap score; region labels; gap markers; lasso selection |
| unSearch | One search box. The verdict card shows exists / near / open, P(exists), the 5 nearest items with similarity, the regions it sits between, and actions (save, midpoint, verify) |
| Gaps | Ranked table by type; filters (type, region, verdict, score); the explanation in each row |
| Gap detail | Explanation, score breakdown, timeline (demand vs coverage per year), nearest items, midpoint with a `t` slider, verification results, verdict and note |
| Settings | Models, LLM provider, network switch, weights, calibration status |

Design rule from gapmap: **put the evidence and the verdict controls on the same screen**, so checking a gap never requires leaving the app.

---

## 9. Evaluation

### 9.1 Backtest harness (`invectus eval backtest`)
1. Choose a cutoff date T and drop all items dated after T.
2. Run the full pipeline and rank gaps.
3. Look forward over T+1 … T+Δ: a gap counts as **filled** if a coverage item appears that is "the same thing" by the calibrated distance (§6.2) to the gap's midpoint description or region.
4. Report precision@10/25 and ROC-AUC, against random, raw-count and density-only baselines. Repeat for several cutoffs and hold one out.

### 9.2 Benchmarks
- **Optometry (from gapmap):** 591k PubMed abstracts with roles (primary vs synthesis), plus the 25 manually verified candidates as a labelled set (17 already covered, 3 strong leads). Import straight from gapmap's DuckDB and LanceDB files.
- **Steam games:** a public dataset of game descriptions with release dates; "filled" = a later game near the midpoint.
- **unSearch calibration set:** 300 manually labelled (query, exists?) pairs per benchmark.

### 9.3 Quality gates
No release without the backtest beating baselines on at least 2 benchmarks, and unSearch calibration error (ECE) ≤ 0.1.

---

## 10. Performance targets

| Operation | Target (100k items, laptop CPU) |
|---|---|
| Import + embed | ≤ 30 min (≥ 60 items/s), resumable |
| ANN index build | ≤ 2 min |
| unSearch query | ≤ 300 ms (cached model) |
| Regions (one seed) | ≤ 5 min |
| Gap scan | ≤ 1 min |
| Map render | 60 fps pan/zoom at 500k points |

Gapmap reference on an M4 Pro: about 50–70 docs/s embedding (nomic, MPS, fp16) and 35 s for the kNN graph on 591k items.

---

## 11. Milestones

| # | Milestone | Deliverable |
|---|---|---|
| P0 | Setup and spikes | Rust toolchain, Tauri + React skeleton, CI builds on all 3 OSes; spikes on `ort` embeddings, `lancedb` Rust, clustering crate vs Python reference, UMAP crate — each with a go / no-go note |
| P1 | Engine CLI | `invectus import / embed / search` on CSV, Parquet and folder input; gapmap import; tests |
| P2 | unSearch | Calibration, verdict, CLI plus a first UI screen. **First demo** |
| P3 | Regions and map | Clustering, labels, stability, WebGL map |
| P4 | Gap scan | Coverage, stale and bridge gaps; scoring and explanations; Gaps screen |
| P5 | Midpoints | LLM provider layer, decode prompts, round-trip check |
| P6 | Verification | PubMed, OpenAlex and Steam connectors; verdict workflow; export |
| P7 | Evaluation | Backtest harness and benchmark results; weight tuning |
| P8 | Ship | Signed installers (macOS notarized, Windows, Linux AppImage), docs, demo video |

**Early win:** P2 alone, unSearch on an imported collection with calibrated verdicts, is a demo-able product. Build it before the map.

---

## 12. Risks

| Risk | Mitigation |
|---|---|
| False "doesn't exist" claims (the core credibility risk) | Calibration, evidence on every verdict, external verification, wording that says "not in this collection", measured ECE |
| Rust ML ecosystem gaps (UMAP, HDBSCAN maturity) | P0 spikes; Leiden on the kNN graph needs no projection; optional Python sidecar for the map only |
| ONNX Runtime binary size and GPU providers differ per OS | CPU baseline everywhere; GPU providers optional; download the model after install |
| Midpoints come out generic ("a game") | Bridge selection rules (§5.3), round-trip check, show the neighbouring items, not only the prose |
| Local LLM too heavy for some laptops | Small default model; Ollama / LM Studio integration; cloud opt-in |
| Source-text copyright | Local use; exports default to titles and links |
| Name conflicts ("unSearch" is in use elsewhere, "Invictus" marks exist) | Keep inVectus as the working name; trademark check before launch; unSearch as a feature name |
| Scope creep | Ship P2 first; every connector beyond PubMed and Steam is post-v1 |

---

## 13. Relationship to other projects

- **gapmap** (`~/Documents/Medical Paper Vector Database`): the Python prototype and the first benchmark. Its algorithms (coverage-gap scoring, nearby-synthesis linking, relevance filter, verdict sync) are the reference implementations to port and test against.
- **Angrove** (`~/Developer/Angrove-iOS-main`): a separate product, with no shared code (Swift vs Rust). Angrove's Midpoint lets the *user* pick two ideas from a conversation; inVectus lets the *data* pick them from a collection. An "automatic midpoint suggestions" experiment in Angrove's 51,836-passage library could feed lessons back.

---

## 14. Open questions

1. Default embedding model: nomic (better quality on gapmap) vs bge-small (about 3× faster)? Decide from the P0 speed spike.
2. Leiden on the kNN graph vs HDBSCAN on a projection for regions?
3. Which local LLM by default, and does midpoint quality need a larger model than labels do?
4. Steam data source and terms: official endpoints vs public datasets?
5. Licensing and distribution: open-source core with a paid app, or closed?
6. Final name, after a trademark check.
