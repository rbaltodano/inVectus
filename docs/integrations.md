# Data-source integrations: Excel, Google Sheets, Notion

Status (2026-11-10): implemented in the engine, CLI, desktop app and agent tools. Excel is
tested against a fixture; the Google Sheets and Notion pulls are **not yet tested against the
live services** (unit tests cover parsing, flattening and the snapshot round trip only).

## Model: pull, then work offline

- **Excel** (`.xlsx`, `.xlsm`, `.xlsb`, `.xls`, `.ods`) is a local source like CSV. The first
  non-empty row is the header; a sheet can be chosen (`--sheet`, the import screen's Sheet menu,
  `sheet` in agent tools). Dates become ISO 8601 text; whole numbers lose `.0`.
- **Google Sheets** and **Notion** are *pulled sources*. A pull is the only network step: it
  writes `<project>/sources/<kind>/<id>.csv` and a `.json` record (origin, time, rows,
  SHA-256). Import, re-import, search and gaps read that copy offline.
- A pull needs the project's network switch (or the CLI's `--network`) and is refused by
  `--offline` / `INVECTUS_OFFLINE=1`. It runs as a cancellable `pull` job in the app.
- "Pull and update" on a collection pulls again, then re-imports with the saved mapping.

## Google Sheets

- Paste the sheet's link (`docs.google.com/spreadsheets/d/<id>/…#gid=<tab>`). The tab comes from
  `gid`; without it Google exports the first tab.
- Works only for sheets shared as "Anyone with the link" (CSV export, no sign-in). Private sheets
  get a clear error. Google sign-in (OAuth) is deferred until the owner creates a Google Cloud
  OAuth client; until Google verifies the app, a read-only Sheets scope shows an "unverified
  app" warning and is capped at 100 users.

## Notion

- Auth: an internal integration token (`ntn_…`), pasted in Integrations or
  `invectus integrations notion-token` (stdin). Stored in the OS keychain (service
  `app.invectus.integrations`); `INVECTUS_NOTION_TOKEN` overrides it. Never written to project
  files. Linux uses the kernel keyring, which does not survive a reboot.
- Source: a database (`notion:<id>` or its notion.so link). The user must share the database with
  the integration (••• → Connections).
- Columns: `page_id`, `url`, `created_time`, `last_edited_time`, every property (title first,
  then by name; lists joined with ", "), then `body` (page text, nested blocks up to 3 levels,
  at most 1,000 blocks per page; child pages and databases are not entered).
- API version `2022-06-28`; requests are spaced 350 ms apart (Notion allows ~3/s) and retried on
  429/5xx. About 1,000 pages take ~6 minutes because each page body is a separate call.

## Interfaces

| Where | Excel | Google Sheets / Notion |
|---|---|---|
| CLI | `invectus import file.xlsx --sheet Tab …` | `invectus pull -p P <link> [--network]`, then `invectus import -p P <link> …` |
| App | Add to Dataset (file picker or path) | Add to Dataset: paste link → Pull → map columns; Collections: Pull and update |
| Agents (MCP) | `inspect_source` / `import_collection` with `sheet` | Same tools read an existing pull (`needs_pull` otherwise); agents cannot pull |

## Not done / next

- Live-service tests for Sheets and Notion (needs a shared test sheet and a Notion workspace).
- Google sign-in for private sheets; Notion pages under a parent page (not databases).
- Notion API 2025-09-03 "data sources" (multi-source databases) are not handled.
