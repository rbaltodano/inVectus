# Beta release checklist

This is a release gate, not a declaration that beta is ready. Local macOS checks do not
verify Windows, Linux, an older macOS release, or an installation on another machine.

## Current setup and update path

Use the existing explicit-download path for beta preparation: installers contain no model
weights. In the desktop app, create/open a project, enable its network switch, then use
Settings → Embedding models → Download…. The confirmation gives the model, source and size.
Install `bge-small-en-v1.5` (about 134 MB from the pinned Xenova conversion on Hugging Face),
then switch the project offline. Imports, embedding, search and gap scans run locally.
`INVECTUS_OFFLINE=1` refuses even an explicitly requested download.

The agent edition supports `./install.sh --model` with its own confirmation, or
`invectus models download bge-small-en-v1.5`. Models are shared across projects. The local
language model is optional; agent-written descriptions do not require it.

BGE remains the provisional implementation default. SPEC §7.3 names Nomic; the owner must
resolve that spec conflict before a final default is advertised. P2 compared calibration
and title retrieval, but broader human-judged retrieval quality is still unverified.

There is no automatic updater. A small beta can use manual replacements: quit inVectus
and disconnect agent sessions, copy a backup of each `.invectus/` project while closed,
replace the app/binary, reconnect clients if its path changed, and reopen a disposable copy
of a previous project before using the originals. Test that process for every beta build.
Do not advertise automatic updates or automatic project migration/rollback.

## Before distributing a build

- Run the local checks in [development.md](development.md). Save the exact commit, build
  version, OS/architecture, passing checks, and any skipped model-dependent tests.
- With owner authorization, push and manually dispatch `build.yml`. Record the run and
  commit in PROGRESS.md. Keep both workflows manual-only. Do not claim Windows/Linux until
  their CI and installer smoke checks pass. The last inspected run, `42146609686`, failed
  on all three systems; the local fixes still need a new run.
- Build the actual distribution artifact. Verify the bundled `invectus` CLI runs from
  inside it and that Connect an AI agent registers that installed path. Current Tauri
  config bundles the CLI into the macOS app; Windows/Linux packaging still needs validation.
- Complete the dependency notice review below and include the resulting notices with both
  editions and the extension bundle.
- Sign and notarize the macOS distribution using the owner's Developer ID and notarization
  credentials. Verify the downloaded artifact with Gatekeeper on a clean Mac. Signing
  credentials and an actual notarized artifact remain outstanding.
- For any Windows/Linux beta, test the actual installer on those systems, including runtime
  libraries, writable data directories, client paths, uninstall and upgrade. Windows app
  hand-off is not implemented: close the app before an agent opens the same project.
- Use a unique version per distributed beta and include its limitations and feedback steps.

## Dependency notice review

After the locked Rust sources and npm packages are installed, run:

```sh
node scripts/collect-release-notices.mjs
```

This runs Cargo metadata offline and collects source license/notice texts, versions and
hashes into `target/release-notices/THIRD_PARTY_LICENSES.txt` and `inventory.json`. It includes
Rust build/test/other-platform packages conservatively, plus non-dev npm packages and the
repository's font/icon notices. Generated files remain untracked. A missing source or npm
version mismatch fails the collector; it never fetches missing files itself.

Review every `review_required` entry. Separately review the exact prebuilt ONNX Runtime
distribution and its third-party notices, statically linked native libraries, system
components and any model redistribution terms. Source collection alone does not establish
that binary distribution obligations are complete. The first local inventory collected
829 packages and identified 61 entries needing license metadata/file review.

## Clean-machine smoke test

Use a separate machine or fresh OS account with no development model store, Rust, protoc,
Homebrew or custom PATH. Record the build commit, artifact checksum, OS and architecture.

1. Install from the actual downloaded artifact. Launch from the normal app launcher, then
   run the packaged CLI. Confirm neither requires the repository or developer libraries.
2. Create a project. Before installing a model, try importing a small fictional CSV. Verify
   the model requirement is explained and no download starts automatically.
3. Try installing while offline; verify refusal. Enable network, inspect and cancel the
   download confirmation. Confirm a subsequent explicit download completes and validates.
4. Turn network off. Index the collection, run a search and idea check, inspect a gap, save
   a decision, quit and reopen. Verify persisted results and collection-scoped wording.
5. Cancel an indexing job and restart it. Check that the project remains usable and jobs
   do not remain running after restart. Test a second import for duplicates/idempotence.
6. Test Light, Dark and Match system, including the map, native dialogs, keyboard navigation
   and reopening. Review clipped text and unreadable labels at the default window size.
7. Connect each client being advertised. Verify real tool calls, write suggestions, human
   acceptance, app refresh, and reconnection after upgrade. Grok requires owner sign-in;
   a handshake alone does not verify a model session. Test Claude Desktop's `.mcpb` dialog.
8. Upgrade with a disposable existing project using the procedure above. Uninstall and
   document which project/model files remain. Never delete a tester's collections silently.

Report failures with the build version/commit, OS/architecture, exact steps, expected and
actual results, and model readiness. Use fictional data; omit credentials and private projects.
