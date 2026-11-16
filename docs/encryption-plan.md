# Encryption at rest: plan (proposal, 2026-11-12)

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

## What a project holds today (all plaintext)

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

**DuckDB (`project.duckdb`).** Upgrade from DuckDB 1.2 to the newest stable 1.4.x (1.4.2 or
later; earlier 1.4 releases had encryption fixes). Open with
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
   (`.cargo/config.toml`), never the keychain. The app does not yet show the code right after
   creating a project (Settings only).
2. DuckDB 1.4 upgrade on its own, with every test and benchmark re-run (medium; the main risk).
3. DuckDB encryption on, for new projects first (small once 2 lands).
4. Encrypted snapshot and cache files (small to medium).
5. Vectors: prototype route 1, measure on `gapmap-full`, decide, finish (large).
6. Migration of existing projects, export warning, log audit (medium).
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
