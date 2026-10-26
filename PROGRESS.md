# Progress ledger

## 2026-10-26
- Created the project and wrote `SPEC.md` v0.1. Stack: Tauri 2 + Rust core + React/TypeScript.
- Machine state: Node and npm installed; **Rust not installed** (`rustup` is needed for P0).

## Next (P0)
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
