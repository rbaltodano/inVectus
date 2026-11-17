# Security model

What inVectus protects, where its trust boundaries are, and what it does not claim. Updated
2026-11-12 with the audit of the 2026-11-10…12 commits (baseline 66fa1ec). See PROGRESS.md for
what was tested on which platform.

## Network

- Nothing goes online by itself. Every call starts from the person: a model download, a pull
  from their own integration, the listing of what a token can read, or a gap check.
- **Offline mode** (`--offline`, `INVECTUS_OFFLINE=1`) refuses all of it.
  - The CLI flag is held in memory (no environment change).
  - A request an offline agent forwards to the app runs offline there too, whatever the app's own
    state, including any job it starts. A forwarded request that does not say is treated as
    offline.
- **External checks** (a per-project switch, off by default) gates calls that send text derived
  from the collection to another service: gap checks against PubMed, OpenAlex, Semantic Scholar
  and arXiv. The choice is recorded per machine (`<OS data dir>/inVectus/consent.json`)
  as well as in the project, so a copied or shared project arrives with checks off.
- **Credentials** (integration tokens) are stored only in the system keychain.
  - Each client is bound to its service's exact origin (`https://api.notion.com`, …). Every URL,
    including next-page links a service returns, is checked before sending: https only, that
    host exactly, port 443, no user name or password.
  - Clients never follow redirects, so no header, key or body is replayed elsewhere.
  - Cursors and ids are percent-encoded, and paged loops are bounded (50,000 pages) and stop on
    a repeated cursor.
  - Google Sheets sends no credentials and follows redirects by hand, only within
    `docs.google.com` and `*.googleusercontent.com`.
- **Deadlines.**
  - Pulls: 15 s to connect, 60 s for an answer, 120 s per request. Rate-limit and server-error
    retries are bounded (5 attempts) and stop at once when cancelled.
  - Gap checks: 10/20/30 s, 3 attempts, 90 s for the whole check; they are cancellable (Stop) and
    hold no project lock while waiting.
  - Model downloads: bounded connect, answer and per-read waits; https only; checksums verified.
- **Network activity** (Settings → Network) lists every call since the app opened: time, host,
  purpose, and status. Paths, queries, headers and bodies are never recorded.

## Agents (MCP) — trust boundary

An agent client (Claude Code, Codex, and the cloud model behind it) calls inVectus tools over
stdio or the app's local socket (owner-only, peer user checked). **What a tool returns can reach
the agent's provider.**

- **Files.** Agent tools read files only inside:
  - the projects folder (`~/inVectus Projects`, or the folder chosen in Settings → Storage; kept
    per machine in `<OS data dir>/inVectus/settings.json`);
  - the open project;
  - files and folders the person granted in Settings → Storage → Agent folders or with
    `invectus agents allow <path>`. Grants are kept per machine
    (`<OS data dir>/inVectus/agent-access.json`), never in a project.
  - The whole disk, the home folder, and any folder that contains the home folder are refused,
    both as a grant and as the projects folder.
- **Path checks.** Paths are resolved (`..`, `~`, and symlinks, including the final file) before
  they are compared, so links that escape are refused. Agents open or create projects only in the
  same places. Imports the person makes in the app (file dialog) are not limited.
- **Collection text.** Search results, gap context and samples go to the agent by design, only
  for projects whose Agent access switch is on (off by default; new projects start with the last
  choice made on this machine).
- **Who grants.** Grants and the projects folder change only through the app's own window or the
  CLI, never through an agent tool. A script injected into the app's window could change them;
  the window's content security policy runs only the app's own scripts, and no code renders
  collection text as HTML.

## Files on disk

- On macOS and Linux, project folders are `0700` and files `0600` whatever the umask. That covers
  the project root, `vectors/`, `cache/`, `exports/`, `sources/`, `project.json`,
  `project.duckdb`, pulled copies and their records, gap-check caches, the grants file and the
  consent file.
- Writes go through an owner-only temporary file that is renamed into place, and are removed on
  failure.
- Projects made by older versions are tightened when opened (the root and its standard folders).
- **Windows:** project files/folders and shared settings get a protected ACL granting access
  to the current user, including inheritance for new children. Platform validation is recorded
  per CI run in PROGRESS.md; macOS checks alone do not verify Windows behavior.
- **Pulled copies** stay in the project until removed: nothing expires them.
  - `invectus sources list` shows each copy, its origin, size, pull time, and the collections
    using it.
  - `invectus sources forget <name>` removes a copy and its record, and refuses while a collection
    still uses it.
  - Settings → Network → Pulled copies offers the same list and guarded removal.
- **Encryption coverage (new projects):** the database, WAL/spills, pulled CSV/JSON copies and
  verification cache are encrypted. Source/cache encryption uses authenticated 64 KiB blocks,
  bound to project id, relative path, header and block index, with a fresh derived key per write.
  Decrypted remote CSV is parsed in memory; temporary writes contain ciphertext only.
  Authentication/key errors fail rather than falling back to plaintext or a fresh network call.
- New-project setup shows a recovery code before the first import. Save it separately: losing
  both the stored key and recovery code loses access to encrypted data. Settings can show it again.
- Existing projects keep their previous coverage until migration. Filenames, sizes and
  `project.json` remain visible. Encryption does not prevent replay of complete older files;
  DuckDB also has the format limitations documented in `docs/encryption-plan.md`.

## Not yet (planned or open)

- Lance vectors remain plaintext, as do intentional exports and `project.json`. Migration of
  existing projects is not implemented. Do not describe entire projects as encrypted yet;
  see `docs/encryption-plan.md` for phases 5–7 and measurement work.
- Linux keeps integration tokens in the kernel keyring, which does not survive a reboot.
- Spreadsheet imports have no decompression-size cap.
- Model files are re-hashed only when their size or modification time changes.

No claim here means the app is "secure" in general. These are specific controls, and their
tests are listed in PROGRESS.md.
