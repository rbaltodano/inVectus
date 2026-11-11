# Beta QA log

Feature-by-feature test pass before the beta. No earlier QA log existed; this merges the
open items already recorded in [beta-release.md](beta-release.md) and PROGRESS.md with a
fresh check. Status: **pass**, **fixed** (bug found and fixed in this pass), **open**
(not fixed), **owner** (needs an owner decision or credentials), **unverified** (not
testable here).

Environment: arm64 macOS 27.0.1 (M-series, 24 GB), commit `69c47be` plus this pass's
changes. Data: the fictional Urban Futures demo (540 records, `scripts/create_demo.py`)
and small hand-made TSV/CSV/text-folder fixtures. Models: BGE-small, Nomic, Gemma 4 12B
from the dev store. Native window not driven (owner asked for no computer use); the UI was
checked by code review, unit tests and the type checker.

## 2026-11-10 pass

### Engine and CLI

| Feature | Status | Notes |
|---|---|---|
| `init`, `status` | pass | |
| Import CSV / TSV / text folder | pass | Empty-text rows skipped and counted; unicode file names/text fine |
| Import preview, re-import idempotence | pass | Second import: 0 new, 2 unchanged |
| Token chunking of long texts | pass | 3 × 1,500-word docs → 33 chunks; search reports "via chunk N" |
| Import errors | pass | Unknown column, missing file, unsafe collection name all refused clearly |
| Two spaces in one collection | pass | Nomic added beside BGE; search refuses to guess and asks for `--space` |
| Project lock | pass | Second process refused clearly (the message also includes raw DuckDB lock text — cosmetic) |
| `embed` | pass | Re-run skips all 540 embedded; parent texts over 512 tokens reported as truncated |
| `search` (ANN, `--exhaustive`, `--role`) | pass | |
| `calibrate` (no LLM) | pass | Too-small collections refused with the reason |
| `check` verdict | **fixed** | With a chance-level calibration (demo: AUC 0.50) every query, even nonsense, was "NEAR · Related items exist" with no warning. Now adds an explicit note below AUC 0.6 (`WEAK_AUC`) |
| `regions run/list/map` | pass | <20 items refused with the reason |
| `gaps scan/list/show/verdict` | pass | Bad verdict / unknown gap id refused |
| `gaps describe` (Gemma) | pass | Accepted on first try, ~20 s incl. load. Cosmetic: llama.cpp Metal log spam on stderr and Rust debug text (`Region { region: 0 }`, `Some(0)`) in CLI output — open, low |
| `gaps verify` offline | pass | Refused with project switch off and with `INVECTUS_OFFLINE=1 --network` |
| `gaps export` md/csv | pass | HTML export (SPEC §4 step 8) not implemented — open, spec gap |
| `models list/verify/download` | pass | Download refused offline |
| `llm list` | pass | |
| `parity` | pass | n=200, mean cos 1.000 |
| `connect clients/status/add` (plan only) | pass | Relative custom path refused |
| `connect bundle` | pass | 110 MB `.mcpb` |

### Agent (MCP) tools, via a live `invectus mcp` session

| Tool | Status | Notes |
|---|---|---|
| open_project, project_status | pass | |
| check_idea, search | pass | |
| list_gaps, gap_detail, scan_gaps | pass | |
| suggest_decision | pass | Stored as suggestion, decision unchanged |
| submit_region_names, region_context | pass | |
| submit_description, gap_context | pass | Checked and stored |
| paraphrase_sample | pass | submit_paraphrases not run (needs ≥30 written paraphrases) |
| export_gaps | **fixed** | File name was `gaps-<collection>-regions:.csv`: the `:` breaks on Windows and the name was the same for every run. Now `gaps-<collection>-<run timestamp>.<ext>` with unsafe characters replaced; unit test added. Also affects the app's export button |
| verify_gap (offline) | **fixed** | Refusal told agents and app users to "pass --network" (a CLI flag). Now names the project's network switch as well |

### Desktop app (code review + unit tests; native window not driven)

| Area | Status | Notes |
|---|---|---|
| Frontend → backend command wiring | pass | All 41 `invoke` calls match registered Tauri commands |
| UI unit tests (46), `tsc --noEmit` | pass | |
| Import with no model installed (first run) | **fixed** | **Beta blocker.** Token chunking, the default, needs BGE-small's tokenizer, so a new tester's preview/import failed with a CLI instruction. The import screen now detects the missing model, switches to character chunks and says why; indexing then offers the download as before. The agent import already did this |
| "Model not installed" message | **fixed** | Told users to run `models install <key>` without the required `--from`, plus a raw OS error. Now names Settings → Models, `models download` and `models install … --from <folder>` |
| Collections → calibration line | **fixed** | A near-chance calibration (AUC < 0.6) is now labelled "near chance: verdicts say little" |
| Gaps page filters | open, spec gap | Type filter only; SPEC §8 also lists region, verdict and score filters |
| Gap detail → Describe | **fixed** | Without the optional local LLM (the app cannot download it) the button always failed with a file-path error. Now disabled with a note pointing to agents |
| Gap detail → bridge `t` slider | **fixed** | Every slider step reloaded the gap and the Saved list; now waits until the slider rests (250 ms) |

### CI and release (from beta-release.md / PROGRESS.md, re-checked)

| Item | Status | Notes |
|---|---|---|
| CI run 42532458660 | **fixed** (unverified until CI re-run) | macOS and Linux passed; Windows failed in `connect_writes_only_when_asked_and_bundles`: the test asserted a Unix file mode that a Windows-built zip does not record. Assertion now Unix-only |
| Windows / Linux support | pass | `build.yml` run 42907902558 on `7defae2`: full suite passes on macOS, Windows, Linux |
| Windows / Linux installers | **fixed**, pass (`release-build.yml` run 42932092867, released as v0.2.0) | `build.yml` builds with `--no-bundle`, so no installer existed, and only the macOS bundle carried the `invectus` CLI that agent connections need. New manual `release-build.yml`: NSIS (Windows), .deb + AppImage (Linux), macOS app; installs each, runs the installed CLI offline, uploads the installers. The CLI ships as a Tauri `externalBin` (`tauri.cli-sidecar.json`), so it lands beside the app (Windows install folder, `/usr/bin` on Linux). First run: 42874674481 |
| Native-window pass | blocked | osascript is "not allowed assistive access" and screen capture is refused for this session; needs Accessibility + Screen Recording for T3 Code |
| Windows agent hand-off | open, known | Not implemented on Windows: an agent cannot open a project the app has open (lock error). Document for testers |
| AppImage + agent connections | open | The AppImage's CLI path changes on every launch, so a registered agent connection breaks; recommend the .deb for agent use |
| Developer ID signing + notarization | pass | v0.2.0 macOS app signed, notarized and stapled (`scripts/sign-macos.sh`); Windows installer still unsigned |
| Default embedding model (BGE vs SPEC's Nomic) | owner | Open spec conflict |
| Dependency notices (61 review entries, ONNX Runtime binary) | open | |
| Clean-machine smoke test | unverified | Needs a separate Mac or fresh user account |

### Checks after the fixes

`cargo test --workspace --release`: 136 passed, 0 failed. Clippy (`-D warnings`, all
targets), `cargo fmt --check`, `tsc --noEmit`, `npm run build` and 46 UI tests passed.
Re-ran the weak-calibration note, the offline verify message and the missing-model message
against the rebuilt CLI. Export file names are covered by a new unit test.

### Still open after this pass

- **Owner:** approve a push and a `build.yml` run to confirm the Windows test fix; decide
  whether the beta advertises Windows/Linux at all.
- **Owner:** Developer ID signing and notarization, or ship the beta via the curl installer
  only (a browser-downloaded unsigned app is blocked by Gatekeeper).
- **Owner:** default embedding model (BGE in code vs Nomic in SPEC §7.3).
- **Owner / unverified:** native-window pass and the clean-machine checklist in
  beta-release.md (this pass did not drive the app).
- Spec gaps, not blockers: HTML export; Gaps page region/verdict/score filters.
- Cosmetic: llama.cpp Metal logs and Rust debug text in `gaps describe` output; raw DuckDB
  text in the project-lock error.
- Dependency notice review (61 entries, ONNX Runtime binary).
