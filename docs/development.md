# Development

inVectus is a Tauri 2 desktop app and a Rust CLI over the same engine crates.
These instructions describe source development, not an end-user installer.

## Prerequisites

- Rust through rustup; `rust-toolchain.toml` pins 1.99.1, rustfmt and Clippy.
- Node.js 24 with npm (`npm ci` uses the committed lockfile).
- A native C/C++ compiler, CMake, and `protoc` (tested with Protocol Buffers 36.2).
- [Tauri's platform prerequisites](https://v2.tauri.app/start/prerequisites/).
- Python 3 for the optional synthetic demo and publication tools. Both use only the standard library.

On macOS, install Xcode and its command-line tools. On Windows, Tauri requires
MSVC Build Tools and WebView2; on Linux it requires WebKitGTK 4.1 and platform
packages. Windows/Linux instructions and builds are not yet verified for this app.
The manual [desktop workflow](../.github/workflows/build.yml) records intended CI setup.

LanceDB and DuckDB include native builds. Check free disk space before compiling;
local debug build trees have exceeded 20 GB. Prefer the release profile when its
artifacts already exist. If `protoc` is outside `PATH`, set `PROTOC` to the absolute
executable path before running Cargo. Models are not required to compile.

## Build and run

From the repository root:

```sh
npm ci
npm run build
cargo build --workspace --release --locked
npm run tauri dev
```

The release CLI is `target/release/invectus` (`invectus.exe` on Windows).
The Vite-only `npm run dev` preview does not host the Rust engine; use the native
app to exercise imports, search and projects.

To produce a macOS app bundle:

```sh
npm run tauri build -- --bundles app
```

Signing, notarization and tested Windows/Linux installers are not part of the
current publication milestone. A local build does not imply distribution readiness.

## Embedding models

BGE-small is the current default. Its pinned ONNX weights, tokenizer and config
are about **134 MB** in total. Choose one explicit installation route:

```sh
# Use files already downloaded from the pinned source; no network:
target/release/invectus --offline models install bge-small-en-v1.5 --from /absolute/path/to/model-files

# OR deliberately download the pinned files from Hugging Face:
target/release/invectus models download bge-small-en-v1.5
```

The exact revision and file hashes are in the [registry](../crates/embed/src/registry.rs).
Installation checks those files. Model licenses are separate from inVectus's MIT license.

Models live in the OS application-data directory. `INVECTUS_MODELS_DIR` or the CLI's
`--models-dir` flag selects another directory. The desktop app must use that same
location to load a CLI-built project's model. No weights or datasets belong in Git.

`INVECTUS_OFFLINE=1` or `--offline` refuses network operations. Import, embedding,
search, calibration and clustering do not require network access once files are
installed. PubMed/OpenAlex verification is optional and requires an explicit network
switch or CLI `--network` option. Optional local language models are not required
for the default calibration or synthetic demo.

## CLI workflow

The [demo](demo.md) is the shortest reproducible path. For your own CSV:

```sh
target/release/invectus --offline init .spike-data/my-project
target/release/invectus --offline import -p .spike-data/my-project.invectus papers.csv \
  -c papers --id id --title title --text abstract --date date
target/release/invectus --offline embed -p .spike-data/my-project.invectus -c papers
target/release/invectus --offline calibrate -p .spike-data/my-project.invectus -c papers
target/release/invectus --offline check -p .spike-data/my-project.invectus -c papers "an idea to investigate"
target/release/invectus --offline search -p .spike-data/my-project.invectus -c papers "an idea to investigate" --exhaustive
target/release/invectus --offline regions run -p .spike-data/my-project.invectus -c papers
target/release/invectus --offline gaps scan -p .spike-data/my-project.invectus -c papers
```

Map columns to the actual source schema. Role rules can map a source column to
`demand`, `coverage`, or `neutral`; see `invectus import --help`. A useful gap scan
needs a meaningful distinction between demand and coverage.

New collections default to 448-token chunks with 48-token overlap using the
installed BGE tokenizer. Reimports keep their stored policy. Explicit character
chunking (`--chunk-chars 2000 --chunk-overlap 200`) supports model-free import,
but character counts do not guarantee that chunks fit an embedding model's cap.
Use `--space` to choose a table when a collection has multiple embedding spaces.

## Local verification

```sh
npm run test:ui
npm run build
cargo fmt --all --check
cargo test --workspace --release --locked
cargo clippy --workspace --all-targets --release --locked -- -D warnings
```

Real-model tests print a skip notice when their local files are unavailable.
Passing model-free tests is not evidence that inference was exercised. Local
checks do not establish Windows/Linux support.

Both GitHub workflows are `workflow_dispatch` only. Keep them manual-only and
follow [AGENTS.md](../AGENTS.md): do not dispatch any CI while the repository is private.

## Research and vendored code

The [spike workspace](../spikes/Cargo.toml) is separate from the desktop workspace.
Its reproduction scripts can require explicit model/dataset downloads; read the
[experiment notes](spikes/README.md) before running them. They are not quick-start steps.

The root workspace uses a [documented local-only LanceDB patch](../third_party/README.md).
Do not enable hosted/remote features to work around a local build issue.
