# Vendored dependencies

## lancedb 0.40.0 (local-only patch)

Upstream `lancedb` 0.40.0 (Apache-2.0, crates.io archive SHA-256
`078c80d326ec656d53247ca8647222dab4707b623e795a4a2de1d1063ff78e88`) fails to compile without its
`remote` feature: `src/job.rs` constructs `Error::Http`, a variant gated behind `remote` (E0599 at
lines 102 and 112). P0 worked around this by enabling `remote`, which links the hosted-client code.

inVectus is local-first, so the workspace instead patches crates.io `lancedb` with this copy
(`[patch.crates-io]` in the root `Cargo.toml`) and builds it with **no features**. The source
changes are in [`lancedb-0.40.0-local-only.patch`](lancedb-0.40.0-local-only.patch): with `remote`
disabled, the two unreachable server-job branches in `src/job.rs` report `Error::Runtime` instead
of `Error::Http`, and `src/lib.rs` allows the resulting dead code (`cfg_attr(not(feature =
"remote"), allow(dead_code))`). With `remote` enabled the code behaves exactly as upstream.

Vendored files: `src/`, `README.md`, the upstream `LICENSE`, and `Cargo.toml` with its `[[example]]`/`[[test]]`/`[[bench]]`
target sections removed (their files are not vendored). Remove this copy when an upstream release
compiles without `remote`.


The Apache-2.0 license file is copied verbatim from the upstream `v0.40.0` tag:
https://raw.githubusercontent.com/lancedb/lancedb/v0.40.0/LICENSE.
The crates.io archive omits that file. Copyright notices in the upstream source
are preserved. The three modified files (`src/job.rs`, `src/lib.rs`, `Cargo.toml`)
carry a description of the inVectus changes. These attribution comments supplement
the functional patch above; they do not change its behavior.
