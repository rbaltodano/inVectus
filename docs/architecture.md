# Architecture and tradeoffs

## Two interfaces, one engine

The native desktop shell is Tauri 2 with React 18, TypeScript and Vite. Its commands
call the same Rust workspace used by the `invectus` CLI. UI behavior and engine
algorithms can evolve independently without maintaining two search implementations.

| Layer | Crates / location | Responsibility |
| --- | --- | --- |
| Project model and persistence | `invectus-core` | Items, identities, DuckDB metadata, project files and jobs |
| Input and embeddings | `invectus-ingest`, `invectus-embed` | Mapping, chunking, verified model files and ONNX inference |
| Vector retrieval | `invectus-index` | LanceDB tables, indexing, exact search and ANN with exact re-scoring |
| Structure and candidates | `invectus-regions`, `invectus-gaps` | Region graph, map, gap scores, evidence and saved decisions |
| Collection-level assessment | `invectus-unsearch` | Calibration and the `exists / near / open` assessment; internal crate name |
| Optional descriptions and checks | `invectus-midpoint`, `invectus-llm`, `invectus-verify` | Candidate descriptions, local generation and explicit verification connectors |
| Evaluation | `invectus-eval` | Backtesting gap rankings against later collection evidence |
| Interfaces | `invectus-cli`, `src-tauri`, `src/` | Terminal commands, desktop command boundary and UI |

## Data flow

Import maps each source row or file to an item. Long text can also produce overlapping
chunks. Both parents and chunks have vectors; search results collapse matching chunks
back to their parent item and preserve which chunk supplied the evidence.

Each `.invectus/` project contains a DuckDB metadata database, Lance vector tables,
local caches, exports and project settings. Project folders and model files are user
state, not repository assets. No server or account is required for the core workflow.

Embedding jobs reuse unchanged input hashes and checkpoint completed batches.
A cancelled job can resume without recomputing its completed vectors. The desktop
runs one background job at a time and associates it with its project.

## Why embedding identity is explicit

Vectors with the same dimension can still belong to incompatible spaces. A table's
identity includes the model revision, weights and tokenizer checksums, prefixes,
text template, pooling, normalization, token cap, dimensions, precision and runtime.
The query encoder must match that space. Imported vectors from another runtime
require a recorded parity check before a compatible local encoder is used.

This avoids a subtle failure mode: plausible-looking similarity scores produced by
comparing vectors that should never have been compared.

## Retrieval, gap scoring and the map

Nearest-neighbor retrieval is evidence of similarity, not proof of absence. Exact
search compares every stored vector; ANN can accelerate retrieval while exact
re-scoring checks returned candidates. Calibration estimates how similarity behaves
within a collection and reports its method and quality.

Regions are built from the embedding-space similarity graph. Coverage/stale gaps
compare demand and coverage activity; bridges examine relationships between regions.
Scores and supporting items remain available for inspection. Human decisions and
notes are stored separately from engine scores.

The 2-D map is a browsing aid. Its coordinates do not determine regions or gap
scores. A visually empty area is not automatically a validated gap.

## Network boundary

Import, embeddings, search, calibration, clustering and gap scans are local.
Models load from verified local files. Downloads are explicit; optional PubMed and
OpenAlex connectors require the network switch or `--network`. Offline mode refuses
network use. Connector results are cached in the project.

Local descriptions use llama.cpp and a separately installed model. The synthetic
demo and default no-LLM calibration do not need a language model.

## Current tradeoffs

- **BGE-small is the current CPU default.** That choice reflects documented local
  measurements, not a claim that it is the best model for every domain.
- **Token chunks improve input coverage.** The 448-token body budget reserves room
  for titles and prefixes. Very long titles or another model's tokenizer can still
  exceed a cap. The recorded sample did not demonstrate a ranking improvement.
- **Calibration is experimental.** The default title/text split is a proxy, not
  adjudicated relevance labels or a universal probability of novelty.
- **Scale evidence has boundaries.** Read measurements with their sample, hardware,
  route and failure notes. Large-corpus imports do not validate every search,
  clustering or gap workflow at that scale.
- **Cross-platform support is a target.** Local macOS results do not establish
  Windows/Linux behavior. Distribution and signing remain separate work.

See [SPEC.md](../SPEC.md) for intended behavior and [PROGRESS.md](../PROGRESS.md)
for measurements, failures, decisions and unresolved conflicts. Multimodal work and
the agent/MCP direction are not claims about the current text desktop workflow.
