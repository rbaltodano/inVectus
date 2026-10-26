# inVectus

Negative search: find what is missing in a collection, describe what would fill the gap, and
verify it before claiming it does not exist. Local-first desktop app for macOS, Windows and Linux.

- Source of truth: [SPEC.md](SPEC.md)
- Ledger and next steps: [PROGRESS.md](PROGRESS.md)
- P0 measurements and decisions: [docs/spikes](docs/spikes/README.md)

P0 scaffold only: the desktop shell connects to the Rust core; import and search are P1+.

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
cargo run --locked --bin invectus
npm run tauri build -- --no-bundle --ci
```

The root Cargo workspace contains `src-tauri` (`invectus-app`) and `crates/{core,ingest,embed,index,
regions,gaps,unsearch,midpoint,llm,verify,eval,cli}` (packages prefixed `invectus-`). The CLI binary is
`invectus`. No model is bundled or downloaded by the app. ML candidates are isolated in the
[spike workspace](spikes/Cargo.toml); see its reproduction instructions before running them.

CI compiles the desktop shell and spike binaries on all three OSes; benchmark data is local only.
The reference `~/Documents/Medical Paper Vector Database` is never modified. No push is needed for
local development.
