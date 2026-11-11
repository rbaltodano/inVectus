# Integrations: data sources, outside checks and exports

Status (2026-11-10): Excel, Google Sheets, Notion, Zotero and Airtable are implemented in the
engine, CLI, desktop app and agent tools. Excel is tested against a fixture; the four online
pulls are **not yet tested against the live services** (unit tests cover link parsing, value
flattening and the snapshot round trip only).

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
  `invectus integrations token notion` (stdin; the same command takes `zotero` and `airtable`). Stored in the OS keychain (service
  `app.invectus.integrations`); `INVECTUS_NOTION_TOKEN` overrides it. Never written to project
  files. Linux uses the kernel keyring, which does not survive a reboot.
- Source: a database (`notion:<id>` or its notion.so link). The user must share the database with
  the integration (••• → Connections).
- Columns: `page_id`, `url`, `created_time`, `last_edited_time`, every property (title first,
  then by name; lists joined with ", "), then `body` (page text, nested blocks up to 3 levels,
  at most 1,000 blocks per page; child pages and databases are not entered).
- API version `2022-06-28`; requests are spaced 350 ms apart (Notion allows ~3/s) and retried on
  429/5xx. About 1,000 pages take ~6 minutes because each page body is a separate call.

## Zotero

- Auth: an API key from zotero.org/settings/keys (read-only library access; group read access for
  groups). Keychain name `zotero`; `INVECTUS_ZOTERO_TOKEN` overrides it.
- Source: `zotero:library` (the key's own library), `zotero:collection:<KEY>`,
  `zotero:group:<id>[/collection:<KEY>]`, or a zotero.org library or collection link. The library
  must be synced to zotero.org.
- Columns: `key`, `item_type`, `title`, `abstract`, `creators` ("Last, First; …"), `date`,
  `publication` (journal, book, proceedings, site, university or publisher by item type), `doi`,
  `url`, `tags`, `collections`, `date_added`, `date_modified`, `extra`. Top-level items only;
  notes and attachments are skipped. Requests are spaced 150 ms and honour `Backoff` /
  `Retry-After`.

## Airtable

- Auth: a personal access token with `data.records:read` and access to the base. Keychain name
  `airtable`; `INVECTUS_AIRTABLE_TOKEN` overrides it.
- Source: a table or view link (`airtable.com/app…/tbl…[/viw…]`) or `airtable:app…/tbl…[/viw…]`.
- Columns: `record_id`, `created_time`, then each field in the order first seen. Lists are joined
  with ", "; collaborators by name, attachments by file name. Requests are spaced 220 ms (five a
  second per base is Airtable's limit).

## Interfaces

| Where | Excel | Google Sheets, Notion, Zotero, Airtable |
|---|---|---|
| CLI | `invectus import file.xlsx --sheet Tab …` | `invectus pull -p P <link> [--network]`, then `invectus import -p P <link> …` |
| App | Add to Dataset (file picker or path) | Add to Dataset: paste link → Pull → map columns; Collections: Pull and update |
| Agents (MCP) | `inspect_source` / `import_collection` with `sheet` | Same tools read an existing pull (`needs_pull` otherwise); agents cannot pull |

## Roadmap

Every integration feeds one side of a gap: **demand** (what people ask for) or **coverage**
(what already exists). All follow the same rules: pull on request into a project snapshot,
read-only access to the other service, tokens in the OS keychain, `--offline` refuses them,
nothing outbound without a separate approval. All are beyond SPEC v1 (owner decision).

| Integration | Feeds | Auth | Status |
|---|---|---|---|
| Excel / OpenDocument | either | none (local) | done |
| Google Sheets | either | shared link | done (sign-in later) |
| Notion databases | either | internal token | done |
| Zotero libraries | coverage (research) | API key | done |
| Airtable tables | either | personal access token | done |
| Google Forms, Typeform, Tally responses | demand | OAuth / token | planned |
| Canny, Productboard requests | demand vs roadmap | token | planned |
| Mendeley | coverage (research) | OAuth | planned |
| Confluence, Google Drive, SharePoint folders | coverage (internal docs) | OAuth | planned |
| App Store / Play reviews, Reddit | demand (public) | public APIs | planned |
| Gong, Fireflies call transcripts | demand | OAuth / token | planned |
| Slack / Teams channels | demand | OAuth, per-channel opt-in | planned (privacy review first) |
| CRMs: HubSpot, Salesforce (deal notes, lost reasons) | demand | OAuth | planned |
| Support desks: Zendesk, Intercom | demand vs help articles | OAuth / token | planned |
| Issue trackers: Jira, Linear, GitHub Issues | demand | token | planned |
| **Outside checks** (beside PubMed, OpenAlex): Semantic Scholar, arXiv, Google Patents / Lens.org, Product Hunt | verification | none / key | planned |
| **Exports**: gap reports to Notion, Sheets, Slack | output | reuses the connections | planned (outbound: needs explicit approval per send) |

Order: Zotero and Airtable (token, no OAuth, researchers and product teams) → forms and Canny →
the OAuth-based sources once an OAuth client exists → exports.

## Not done / next

- Live-service tests for Sheets, Notion, Zotero and Airtable (needs test accounts).
- Google sign-in for private sheets; Notion pages under a parent page (not databases).
- Notion API 2025-09-03 "data sources" (multi-source databases) are not handled.
