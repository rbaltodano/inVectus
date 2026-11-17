# Encryption at rest: plan and implementation

Owner request: everything a project stores on the device is encrypted. This plan says what that
covers, what it protects against, how to build it, and what is left for the owner to decide.

## What it protects against

| Threat | Protected after this work? |
|---|---|
| A project folder copied off the machine: cloud sync (iCloud, Dropbox), backups, a shared drive, a USB stick | Yes: the files are unreadable without the key |
| A stolen or lost laptop | Yes, even without FileVault or BitLocker |
| Another user account on the same computer | Yes |
| Malware running as the same user while inVectus is unlocked | No. It can read what the app can read (true of every desktop app); the keychain prompt only slows it |
| Someone with the user's login password | No. The key lives in their keychain |

The honest claim is "encrypted at rest", not "bulletproof". Website and docs must say exactly
that.

## Project contents and current coverage (2026-11-17)

New projects encrypt the database, its WAL/spill files, pulled CSV/JSON snapshots and
verification cache. Vectors, exports and `project.json` remain plaintext. Existing projects
retain the encryption coverage they were created with until explicit migration; merely opening
one does not rewrite it. The threat table above describes the completed plan, not today's
partial coverage.

| Path | Contents | Sensitive |
|---|---|---|
| `project.duckdb` | Items (full text, metadata), mappings, regions, gaps, descriptions, decisions, verifications, jobs | Yes |
| `vectors/*.lance` | Item ids, input hashes, embedding vectors | Yes: embeddings can be partly inverted back to text |
| `sources/<kind>/*.csv` and `.json` | Pulled snapshots from integrations | Yes, may include names and emails |
| `cache/` | Verification connector responses (titles, abstracts from PubMed and OpenAlex) | Moderate: reveals what was checked |
| `exports/` | Files the person exports to share | Leave readable on purpose; say so when exporting |
| `project.json` | Name, settings | Low (the project name can be telling) |

Outside the folder: integration tokens are already in the system keychain; models are public
files; DuckDB temp files are covered by DuckDB's own encryption (below).

## Design

**Keys.**
- Each project gets a random 256-bit data key, created with the project.
- That data key is stored in the system keychain (`app.invectus.projects`, keyed by project id)
  and loaded when the project opens.
- `project.json` gets a project id and a key check value (a MAC over a fixed string), so a
  wrong or missing key is caught before anything is read.
- **Recovery key:** at creation, the app shows the data key once as a recovery code (words or a
  base32 string) to print or store in a password manager. A project moved to another computer
  opens with it. Lose both the keychain entry and the recovery code, and the data is gone. That
  is the price of real encryption, and the UI must say so plainly.

**DuckDB (`project.duckdb`).** The original upgrade proposal was superseded: the pinned crate
already bundles DuckDB 2.0. See phases 2–3 for the OS-random shim and its limits. Open with
`ATTACH … (ENCRYPTION_KEY …, ENCRYPTION_CIPHER 'GCM')`. That covers the database, its
write-ahead log and temp files with authenticated AES-256-GCM. The upgrade is a risk on its own
(storage format, extension and API changes); it needs the full test suite and the gapmap-full
backtest re-run.

**Vectors (Lance).** Lance has no at-rest encryption. Two routes:
1. **Encrypting storage layer (recommended to prototype first).** Lance reads and writes through
   `object_store`. Register a store for a private URL scheme that encrypts each object in
   64 KiB blocks: AES-256-GCM per block, a nonce derived from the object path and block index,
   and a per-object random salt in a small header. Range reads decrypt only the blocks they
   touch. This keeps Lance and its performance; cost: a new layer we own (we already vendor
   `third_party/lancedb-0.40.0`).
2. **Move vectors into DuckDB.** One encrypted store, simpler to reason about. Search is already
   exhaustive (exact cosine), but the 591k-paper project must be measured before choosing this.

**Pulled snapshots and cache.** Write them as encrypted files (the same block format, one file
each); imports decrypt into memory and read through DuckDB, without writing plaintext to disk.
`.json` records are encrypted too.

**Never plaintext on disk.** No temp CSV and no plaintext spill. Logs never contain item text
(audit existing log lines). Exports stay readable; the export dialog says the file is not
encrypted.

**Folder permissions.** Create project folders as `0700` on macOS and Linux (owner only).

**Existing projects.** On open, an unencrypted project offers "Encrypt this project". The
migration writes an encrypted copy beside the original, verifies it item by item (counts, input
hashes, vector checksums), then swaps it in and securely removes the plaintext. It is resumable
like every job.

**CLI and agents.** The CLI and the MCP server read the same keychain entry. On a headless
machine without a keychain, `INVECTUS_PROJECT_KEY` supplies it (documented as less safe).

## Phases

1. Key management, recovery code, project id, and folder permissions (small). **Done
   2026-11-16** (`crates/core/src/keys.rs`): new projects get a key in the keychain and a
   `project_id`/`key_check` in `project.json`; `invectus key status|show|restore`; Settings →
   Network → Recovery code. Tests and `cargo run` keep keys in `.spike-data/dev-keys`
   (`.cargo/config.toml`), never the keychain. The app shows the recovery code after project
   creation and before the first import, with an explicit saved-code acknowledgement. It stays
   available in Settings. Navigating away skips acknowledgement without starting the import.
2. DuckDB 1.4 upgrade on its own, with every test and benchmark re-run (medium; the main risk).
   **Finding 2026-11-16: no upgrade needed, but writing is blocked.** `duckdb =1.20000.0`
   already bundles DuckDB **v2.0.0** (the crate version encodes the engine version; 1.20000.x =
   2.0.x). It reads encrypted databases, but refuses
   to write one ("read-only crypto module"): without `httpfs` its mbedtls module would draw
   GCM nonces from `RandomEngine`, a non-cryptographic generator, and DuckDB only allows that
   behind `force_mbedtls_unsafe` ("NOT securely encrypted"). The crate's `httpfs` feature
   (OpenSSL + curl via CMake) is "supported only on Linux and macOS", and it compiles an HTTP
   client into the app. Recorded by `store::tests::the_bundled_duckdb_will_not_write_encrypted_databases_yet`.
   **Resolved 2026-11-17 (owner chose option a): OS random generator, no vendoring.** Instead
   of vendoring `libduckdb-sys`, `crates/core/cpp/duckdb_os_random.cpp` subclasses DuckDB's
   mbedtls AES state and overrides only `GenerateRandomData` with `BCryptGenRandom` (Windows) or
   `getentropy` (macOS, Linux). It is installed as the instance's `encryption_util`, the hook
   httpfs uses for OpenSSL, on an in-memory instance opened through the C API
   (`crates/core/src/duck_crypto.rs`) and handed to duckdb-rs with
   `Connection::open_from_raw_owned`, our one-function patch to the vendored duckdb-rs
   (`third_party/duckdb-1.20000.0`; the engine is not vendored). Project databases are attached
   to it. The cipher is
   DuckDB's own; block, WAL and canary nonces and the temp-file key all come from the OS
   generator. `crates/core/build.rs` compiles it against the bundled DuckDB headers with the
   same defines. A DuckDB upgrade that changes these classes fails to compile;
   `store::tests::a_stock_duckdb_instance_still_refuses_encrypted_writes` fails if DuckDB's own
   module changes. Not affected: the per-file salt and key ids still use `RandomEngine`; they
   need to be unique, not secret, and the key is a uniformly random 256-bit value.
3. DuckDB encryption on, for new projects first (small once 2 lands). **Done 2026-11-17.**
   New projects record `store_encrypted: true` and open `project.duckdb` with
   `ATTACH … (ENCRYPTION_KEY, ENCRYPTION_CIPHER 'GCM')`: blocks, write-ahead log and spill files
   (`temp_file_encryption`, spill folder `project.duckdb.tmp` beside the database) are AES-256-GCM.
   A wrong key, a missing key, or a plaintext file in place of the database is refused. A
   project whose key is not on this computer is *locked*: the app asks for its recovery code
   (`UnlockProject`), the CLI uses `invectus key restore`. Older projects, including phase-1
   projects that have a key, stay plaintext until phase 6. Opening never creates a missing
   database. **Downgrade guard:** this machine records which projects it created or opened
   encrypted (`<data dir>/encrypted-projects.json`, by id and folder); such a project never
   opens unencrypted, even if `project.json` is edited to say so and a plaintext database is put
   in its place. **Limits (document, not fixed):**
   - Random 96-bit GCM nonces: collision chance about q²/2^97 over all q block writes under one
     key, negligible below about 2^32 writes.
   - Each block's tag detects modification, but blocks are not bound to their position (no AAD),
     the main file header is not authenticated, and a person able to write the folder can roll
     the whole database back to an older copy or swap in older valid blocks. Encryption gives
     confidentiality against copies; it is not tamper-proofing against someone who can write
     the folder.
   - The key passes through SQL text; only our final copy of that string is wiped.
   - A project moved to a new folder *and* stripped of its id is not recognised by the guard.
4. Encrypted snapshot and cache files. **Done for new projects, 2026-11-17.**
   - `core::encrypted_file` uses ring 0.17.14 AES-256-GCM in 64 KiB blocks. Each write creates a
     random 256-bit salt; HKDF-SHA256 derives a fresh key bound to the project id and normalized
     relative path. Block-index nonces are unique under that derived key, including rewrites.
     The versioned header, total length and block index are authenticated; empty files have a
     tag. Wrong keys, changed paths, block swaps, truncation and plaintext substitutions fail.
     The reader bounds plaintext at 512 MiB and validates the exact stored length before
     allocating a plaintext buffer. See the format contract in `encrypted_file.rs` and the
     [ring AEAD API](https://docs.rs/ring/0.17.14/ring/aead/struct.LessSafeKey.html).
   - `core::project_files` carries the policy and key without opening DuckDB. It only handles
     `sources/` and `cache/`, rejects symlink paths and writes ciphertext through private atomic
     temporaries. CSV is decrypted and parsed entirely in memory, never via a plaintext temp
     CSV or scratch-DuckDB disk scan. JSON source records and verification answers use the same
     envelope; authentication/key errors do not trigger a network fallback.
   - `files_encrypted` is separate from `store_encrypted`: existing phase-3 projects may already
     contain plaintext sources/cache. Older projects remain unchanged. The per-machine
     downgrade record covers both flags and is updated under a cross-process file lock.
     The record remains best-effort if settings are unwritable; moving and stripping a
     project's identity can still evade the local guard.
   - Complete old versions of a file can still be replayed; filenames and sizes remain visible.
     Plaintext exists in process memory, and comprehensive wiping of all runtime/library
     copies is not guaranteed. This phase does not encrypt Lance vectors or existing projects.
5. Vectors: prototype route 1, measure on `gapmap-full`, decide, finish (large).
6. Migration of existing projects, export warning, log audit (medium). Deleting an old plaintext
   copy cannot guarantee erasure on SSDs, snapshots or backups; migration must explain that.
7. Docs, website Privacy copy, PROGRESS measurements (search and import speed before and after).

Until phase 7 lands, the website must not claim on-device encryption.

## Decisions for the owner

1. **Key storage.** Keychain plus a recovery code (recommended: no extra password day to day),
   or also an optional per-project passphrase (protects the project even from someone with the
   user's login, at the cost of typing it and losing everything if it is forgotten).
2. **Vectors.** Approve prototyping the encrypting storage layer first, with a fallback to moving
   vectors into DuckDB if it is too slow or too complex.
3. **Encryption for every project or a choice.** Recommended: always on for new projects, no
   off switch.
