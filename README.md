# inVectus

Negative search: find what is missing in a collection, describe what would fill the gap, and
verify it before claiming it does not exist. Local-first desktop app for macOS, Windows and Linux.

- Source of truth: [SPEC.md](SPEC.md)
- Ledger and next steps: [PROGRESS.md](PROGRESS.md)
- P0 measurements and decisions: [docs/spikes](docs/spikes/README.md)

Engine (P1–P2): `invectus import / embed / search` on CSV, Parquet and folder input, read-only
gapmap import, and a desktop shell over the same crates. `invectus calibrate` fits what similarity
means "already exists" for a collection, and `invectus check` gives the inVectus search verdict
(exists / near / open with P(exists) and the nearest items). Calibration currently uses a no-LLM
fallback, so P(exists) is approximate; `search` alone is never an absence verdict.

## Development

Install [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/),
[rustup](https://rustup.rs), and Node 24 LTS with npm. `rust-toolchain.toml` pins Rust and tools.
On macOS, Xcode and its command-line tools provide the native prerequisites. On Windows use MSVC
Build Tools + WebView2; on Linux install WebKitGTK 4.1 and the packages listed in CI.

```sh
npm ci
npm run tauri dev
```

```sh
npm run build
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run --locked --bin invectus -- --help
npm run tauri build -- --no-bundle --ci
```

LanceDB needs `protoc` at build time (`PROTOC=/absolute/path/to/protoc`; see
[docs/spikes](docs/spikes/README.md)). The workspace builds a locally patched LanceDB 0.40.0
without its `remote` feature; see [third_party/README.md](third_party/README.md).

## Engine quick start

```sh
invectus models install bge-small-en-v1.5 --from <folder with the pinned files>   # or: models download (network)
invectus init ~/inVectus\ Projects/demo
invectus import -p ~/inVectus\ Projects/demo.invectus papers.csv -c papers \
  --id pmid --title title --text abstract --date pub_date \
  --role-column pub_types --role-rules "Systematic Review=coverage,Journal Article=demand" --preview 20
invectus import -p … (same arguments without --preview)
invectus --offline embed -p ~/inVectus\ Projects/demo.invectus -c papers        # Ctrl-C keeps progress; re-run resumes
invectus --offline search -p ~/inVectus\ Projects/demo.invectus -c papers "idea" --exhaustive --role coverage
invectus --offline calibrate -p … -c papers                 # once per collection and model (re-run after changes)
invectus --offline check -p … -c papers "idea in a sentence or two" [--role coverage]
# Optional: size chunks in the model's tokens instead of characters (new collections only)
invectus import -p … notes/ -c notes --chunk-tokenizer bge-small-en-v1.5
invectus gapmap import -p … --source ~/Documents/Medical\ Paper\ Vector\ Database --limit 2000
invectus parity -p … -c gapmap      # required before searching imported gapmap vectors
```

Every vector table belongs to one embedding space: model commit and weights checksum, tokenizer,
prefixes, text template, pooling, normalization, token cap, dimensions, precision and runtime. Queries
are only compared with vectors from the same space, or, for imported vectors from another runtime,
after a recorded parity check. Models install into the OS app-data folder (`INVECTUS_MODELS_DIR` overrides);
only `models download` uses the network, and `--offline` / `INVECTUS_OFFLINE=1` refuses it.

The root Cargo workspace contains `src-tauri` (`invectus-app`) and `crates/{core,ingest,embed,index,
regions,gaps,unsearch,midpoint,llm,verify,eval,cli}` (packages prefixed `invectus-`). The CLI binary is
`invectus`. No model is bundled; models are installed explicitly. ML candidates are isolated in the
[spike workspace](spikes/Cargo.toml); see its reproduction instructions before running them.

CI compiles the desktop shell and spike binaries on all three OSes; benchmark data is local only.
The reference `~/Documents/Medical Paper Vector Database` is never modified. No push is needed for
local development.
