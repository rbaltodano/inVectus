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

## duckdb 1.20000.0 (owned raw database handle)

Upstream `duckdb` (duckdb-rs) 1.20000.0 (MIT, crates.io archive SHA-256
`54411716279c39d685e0f3f10346ff503def53fd9b8aa8fbf4ea1db4499c312a`), patched in the root
`Cargo.toml` (`[patch.crates-io]`). The engine itself (`libduckdb-sys`, the bundled DuckDB
v2.0.0 sources) is **not** vendored or modified.

Why: encrypted projects need a DuckDB instance opened through the C API, so that
`crates/core/cpp/duckdb_os_random.cpp` can install its encryption module before anything is
attached (docs/encryption-plan.md, phase 2). Upstream's `Connection::open_from_raw` never closes
the handle it is given, so the caller has to; a connection cloned from it could then outlive the
database and use freed memory (found in review 2026-11-17, reproduced as a SIGSEGV). The patch,
[`duckdb-1.20000.0-owned-raw.patch`](duckdb-1.20000.0-owned-raw.patch), adds
`Connection::open_from_raw_owned`: the same code path with `close_on_drop = true`, so the
handle joins duckdb-rs's reference-counted ownership and closes with the last clone. Nothing
else changes.

Vendored files: `src/`, `README.md`, `Cargo.toml` (its `[[example]]` and `[[test]]` sections
removed; their files are not vendored) and `LICENSE`, copied verbatim from the upstream
`v1.20000.0` tag (https://raw.githubusercontent.com/duckdb/duckdb-rs/v1.20000.0/LICENSE); the
crates.io archive omits it. `src/lib.rs` and `Cargo.toml` carry a note of the change. On a DuckDB
upgrade: re-copy the new crate, re-apply the patch, and keep the store tests passing
(`a_connection_cloned_out_of_an_encrypted_store_keeps_the_database_alive` covers this patch).
Remove the copy if upstream gains an owning constructor.
