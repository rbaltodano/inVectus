# Agent instructions for inVectus

These rules apply to every agent working in this repository (Claude Code reads them through
`CLAUDE.md`). If they conflict with a general habit or default, these win; the owner's direct
instructions in a conversation win over these.

## Communication

- Respond in clear, precise language. Say exactly what's needed in as few words as possible, so
  the reader can catch up quickly; there is no fixed word limit.
- Default to very short replies: a few lines. Cut anything the reader can skip.
- Lead with status and decisions. Skip narration, filler, and restating what the reader already
  knows.
- The owner splits their attention across many agents: make every update self-contained and
  easy to scan.

## CI and GitHub

- **Run CI only when the owner asks.** The repository is public (2026-11-05), so standard
  runners are free, but every run is still started by hand: dispatch `build.yml` (`gh workflow
  run build.yml`) only on the owner's request. Never dispatch `multimodal.yml` without approval
  of its downloads (about 1.5 GB per OS). Do not add or change a workflow trigger so that it
  runs on `push`, `pull_request`, `schedule` or any other event; both workflows stay
  `workflow_dispatch` only.
- Do not claim Windows or Linux support from local macOS results. Claim them only from a
  passing CI run, and name the run and commit in PROGRESS.md.
- Commit locally at sensible checkpoints. Push only when the owner asks. Never force-push.

## Sources of truth

- [SPEC.md](SPEC.md) is the source of truth for what inVectus does. Spike or agent
  recommendations are proposals: flag conflicts with the spec instead of silently diverging,
  and record them under "Decisions and spec conflicts" in PROGRESS.md.
- [PROGRESS.md](PROGRESS.md) is a factual ledger: what changed, what was measured (with the
  hardware and sample), what failed, and what is unverified. Update it with every meaningful
  change. Never present extrapolations as validated results (e.g. 100k-item performance).
- Plans for larger work live under `docs/` (e.g. `docs/multimodal-implementation-plan.md`).

## Fixed stack

Tauri 2 desktop app; a Rust workspace of `invectus-{core,ingest,embed,index,regions,gaps,
unsearch,midpoint,llm,verify,eval,orchestration,cli}` plus `src-tauri`; the `invectus` CLI; React 18 +
TypeScript + Vite. One set of engine crates serves both the CLI and the desktop app. Targets:
macOS, Windows, Linux. `unsearch` is an internal crate name only; nothing user-facing may say
"unSearch" (the name is taken; the feature is "inVectus search").

## Engine rules

- **Embedding spaces never mix.** Every vector table carries its full embedding identity (model
  commit, weights and tokenizer SHA-256, prefixes, template, pooling, normalization, token cap,
  dimensions, precision, runtime). Never compare vectors from different spaces; a query encoder
  from another runtime is admitted only after a recorded parity check.
- **No hidden network.** Import, embedding, search, clustering, gap scans, descriptions and
  calibration run offline. Network use is explicit only: model downloads, and verification
  connectors with `--network` (or the project's network switch). `--offline` /
  `INVECTUS_OFFLINE=1` must refuse all of it. Load models from verified local files only.
- **Absence claims are careful.** Keep exact re-scoring and the exhaustive route; nearest
  neighbours are not proof of absence. Verdicts say "in this collection", carry their
  calibration quality, and never imply global novelty or legal clearance.
- **Jobs** are local-first, resumable, idempotent and cancellable; they cache by input hash.
- Map coordinates are for display only and never decide regions or gaps.

## Data, downloads and resources

- The reference project `~/Documents/Medical Paper Vector Database` (gapmap) is **read-only**:
  never modify its files or data; open its DuckDB read-only and scan its Lance tables only.
  Copies go to `.spike-data/` (git-ignored). The owner approved copying the full set for the
  backtest benchmark; keep fingerprinting the reference (`.spike-data/p1/reference-fingerprint-*`)
  before and after reads.
- **Ask the owner before** any download of a model or dataset beyond what was already
  approved, any system-wide install that needs sudo, and any outward action (pushing, posting,
  sending). State the file, source and size when asking.
- Never commit models, data, project folders (`*.invectus/`), `.spike-data/`, credentials or
  generated output.
- Disk is tight on the owner's Mac: check free space before large builds or downloads. The
  debug `target/` tree regrows to 20+ GB; deleting it is fine (regenerable). Prefer
  `cargo test --release` when the release tree already exists.
- The full-gapmap backtest needs ~10–13 GB of RAM per cutoff; run one cutoff per process and
  remove leftover temp projects (`$TMPDIR/.tmp*/backtest.invectus`) after a killed run.
- Follow the tool sandbox's permissions; never bypass approval prompts.

## Build and test

```sh
source ~/.cargo/env
export PROTOC=$PWD/.spike-data/protoc/bin/protoc          # LanceDB needs protoc at build time
export INVECTUS_MODELS_DIR=$PWD/.spike-data/models-store   # installed dev models
cargo test --workspace --release
cargo fmt --all --check
cargo clippy --workspace --all-targets --release -- -D warnings
npm run build
```

Dev projects live in `.spike-data/p1/work/` (`demo.invectus` with BGE and Nomic spaces,
`gapmap.invectus` with 2,000 papers, `gapmap-full.invectus` with 591,484). The local language
model is Gemma 4 12B QAT Q4_0 (GGUF) in the dev model store.
