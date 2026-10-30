# Public repository handoff

Prepared on 2026-10-30. This is a source/portfolio publication milestone, not a
signed binary release, security certification or cross-platform support claim.

## Prepared locally

- Public README with actual native screenshots, current behavior, engineering
  choices, limitations and links to deeper documentation.
- MIT license for original code, package license metadata, preserved font/icon
  notices, and the missing upstream Apache-2.0 license for vendored LanceDB.
  Vendored modifications carry notices; third-party licenses remain separate.
- Standard-library-only fictional demo generator and an offline CLI builder.
  Generated CSV/project/model files stay outside tracked source.
- Development, demo, architecture, contribution and security-reporting guides.
- Bug-report and feature-request templates. Workflow triggers remain manual-only.
- A reproducible, offline heuristic [publication audit](../scripts/audit_publication.py).

## Local history review

The pre-publication audit inspected 120 commits reachable through all local Git refs,
768 unique text blobs, and tracked/non-ignored current files. It skipped 52 binary
blobs; no history text blobs exceeded its 2 MB per-file limit. It found no suspicious
credential/model/project artifact paths.

Five distinct credential-pattern hits occurred in two vendored LanceDB files:
`src/remote/oauth.rs` and `src/remote/token_cache.rs`. Each file matched the pinned
crates.io archive byte-for-byte. The hits are synthetic access/refresh token values
inside upstream test-server helpers. They are not user credentials. The scanner
retains them as candidates for transparent manual review rather than suppressing them.

These counts describe the initial reviewed snapshot, before the publication commit;
new commits naturally change the counts. Reproduce the current report with:

```sh
python3 scripts/audit_publication.py > .spike-data/publication-audit.json
```

This is pattern matching, not an exhaustive secret scanner. It does not inspect
binary contents, unreachable objects, remote-only refs, GitHub Actions logs/artifacts,
or other hosting metadata. No history was rewritten. Commit author identities and
local development paths in older documentation remain part of the public history.
The owner should decide whether that public identity is acceptable before publishing.

## Owner actions

1. Review the README/screenshots and the commit author identity. Most images show
   the fictional demo; the four `ophthalmology-*` screenshots show real PubMed paper
   titles. Confirm the presentation and branding are what you want public.
2. Push the reviewed commits, then change repository visibility to Public in GitHub
   Settings. Visibility has not been changed and no push is performed by this preparation.
3. Set the repository's About description and topics, enable private vulnerability
   reporting, and pin the repository on your GitHub profile. Suggested About text:
   **Local-first research workspace for semantic search, collection gap analysis and saved decisions. Built with Rust, Tauri and React.**
   Suggested topics: `rust`, `tauri`, `react`, `semantic-search`, `local-first`, `research-tools`.
4. Once public, manually dispatch the **Desktop build** workflow and review each OS
   result. Keep Windows/Linux marked unverified until their checks actually pass.
   The separate multimodal workflow performs large explicit downloads and is not
   needed for this text-workflow publication.

A concise portfolio description:

> Built a local-first desktop research tool with a shared Rust CLI/engine, React/Tauri
> interface, ONNX embeddings, DuckDB/LanceDB persistence, resumable indexing, and
> evidence-backed collection gap review. Documented evaluation limits and tradeoffs.

## Follow-up release work

Cross-platform validation, clean-machine setup verification, complete binary dependency
notices, signed/notarized installers, performance budgets and broader human-judged
relevance evaluation remain separate release milestones. None should block presenting
the current app honestly as a working prototype.
