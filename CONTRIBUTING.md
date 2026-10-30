# Contributing

inVectus is an experimental local-first research tool. Focused bug reports,
reproducible examples, documentation fixes and small improvements are welcome.

Read [the development guide](docs/development.md) and [SPEC.md](SPEC.md) first.
The spec describes intended behavior; [PROGRESS.md](PROGRESS.md) records what
was actually implemented and measured. Plans and spikes are not shipped features.

## Reporting a problem

Include the app/commit version, OS and architecture, steps to reproduce, expected
behavior, actual behavior, and whether a model was installed. A small synthetic
fixture is preferable to a real research collection. The [demo](docs/demo.md) can
provide a common starting point.

Do not attach credentials, model weights, private datasets or `.invectus/` project
folders. Review screenshots and logs for personal paths, source content and notes.
Security reports should follow [SECURITY.md](SECURITY.md), not a public issue.

## Changes

- Keep engine behavior shared by the CLI and desktop; avoid duplicating algorithms in the UI.
- Preserve full embedding identities. Do not compare incompatible spaces or silently change a collection's policy.
- Keep core workflows offline. Network use and downloads must be explicit.
- Keep human decisions distinct from engine scores. Limit absence claims to the collection.
- Describe the problem, the resulting behavior, verification, and material limitations in a pull request.
- Record meaningful behavior changes and measured outcomes in `PROGRESS.md`. Note the hardware and sample for benchmarks.

Run the relevant [local checks](docs/development.md#local-verification). State when
real-model tests were skipped or a platform was not exercised. Do not equate a
synthetic demonstration with a relevance or scalability evaluation.

Both workflows are manual-only. Keep their triggers unchanged. While the repository
is private, do not dispatch, re-run or schedule CI. See [AGENTS.md](AGENTS.md) for
repository rules, including downloads, data and local commits.

Original contributions are under the repository's [MIT license](LICENSE).
Preserve third-party copyright and license notices when modifying vendored code.
