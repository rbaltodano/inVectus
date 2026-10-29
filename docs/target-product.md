# inVectus: target product

Owner-approved direction, 2026-10-29. This is the final form the project is heading for. SPEC.md
remains the source of truth for engine behaviour; this document adds how the product is
packaged and used. **Most of what it describes under "agent interface" is not built yet**:
§10 separates what exists from what is planned.

## 1. In one paragraph

inVectus finds what is missing in a collection. In its final form an **AI agent does the
legwork and a person makes the calls**. The engine runs locally. Agents reach it through an
MCP server: they import data, run searches, scan for gaps, verify against outside sources and
draft descriptions. People review the agent's findings in the desktop app (map, evidence,
decisions) or the CLI. The same engine ships in two editions: a headless **agent edition**
and the **full app**.

## 2. Who does what

| Work | Who | Why |
|---|---|---|
| Import, index, map, scan, repeated searches, rephrasing ideas, batch verification, first-draft descriptions | **Agent** | Mechanical, long-running, repetitive; the engine is deterministic and offline |
| Choosing which column is the text and which items count as coverage | **Agent proposes, person confirms** | A wrong guess quietly skews every gap score |
| Deciding a gap is real, already exists, not viable or selected | **Person** (an agent may suggest) | A judgement call; the engine only says "not found in this collection" |
| Exploring the map, noticing things nobody asked about | **Person** | That is what the map is for |

The rule that matters most: **absence claims stay careful through the agent.** Every tool
answer keeps "in this collection" and the calibration quality, so an agent summarising results
has the caveat in front of it.

## 3. Editions

One engine, one code base, two installers.

| | **Agent edition** | **Full app** |
|---|---|---|
| What it is | The `invectus` binary: CLI plus `invectus mcp`, no window | Desktop app (Tauri) with the same binary and MCP server inside |
| For | People who drive inVectus through Claude, ChatGPT, Cursor or another agent | People who also want to look: the data circle, evidence, decisions |
| Embedding model | Required; ~134 MB, bundled or fetched once with consent | Same |
| Local language model | None; the agent does that work (§6) | Optional download (not bundled) |
| Size | Small binary + embedding model | App + embedding model; LLM optional |

Keeping one engine means search, verdicts and gaps are identical whichever way someone uses
inVectus.

## 4. Models

inVectus uses two kinds of model, with very different roles.

| | Embedding model (default `bge-small-en-v1.5`) | Language model (today Gemma 4 12B, ~7 GB) |
|---|---|---|
| Does | Turns items and queries into vectors: search, map, gaps, calibration | Gap descriptions, region names, paraphrases for calibration |
| Required | Yes, in every edition | No |
| Replaceable by the agent | **No.** Agents cannot produce these vectors, and a cloud embedding service would break local-first and the rule that a collection's vectors all come from one identical model | **Yes.** The agent is a language model; tools give it the evidence and check its output (§6) |

**Small-model option for the full app.** A 2–4 B local model could cover region names and
short descriptions. Adopt one only after measuring it against Gemma 12B on the existing tests:
paraphrase calibration (Gemma 12B paraphrases gave held-out AUC 0.952 vs 0.890 title-only on
the 5,000-paper demo) and the description round-trip pass rate (on-target and novelty checks).

## 5. Agent interface (MCP)

### 5.1 How an agent reaches inVectus

1. Installing either edition puts the `invectus` binary on disk (in the app bundle, e.g.
   `/Applications/inVectus.app/Contents/MacOS/invectus`).
2. The agent's client starts `invectus mcp` as a child process and talks over stdin/stdout
   (the MCP stdio transport). It runs only while the agent uses it and **opens no network
   port**.
3. Registering it is one step. For Claude Code:
   `claude mcp add invectus -- /Applications/inVectus.app/Contents/MacOS/invectus mcp`.
   Other clients take a JSON entry with the same command and argument.
4. The full app offers **Settings → Connect an AI agent**: it writes that entry for supported
   clients after the user agrees, or shows it to copy. Claude Desktop can also take a one-file
   extension bundle (`.mcpb`).
5. Models live in the shared app-data folder, so the server and the app never download twice.

### 5.2 Tools

All tools take a project path (or use the one opened with `open_project`). Outputs are JSON
built from the engine's existing result types.

| Tool | Does | Writes | Network |
|---|---|---|---|
| `list_projects`, `open_project`, `project_status` | Recent projects; readiness of each collection and index | No | No |
| `inspect_source` | Columns and sample rows of a CSV/Parquet/folder | No | No |
| `import_collection` | Import with a mapping; starts index → map → gap scan → calibration | Yes | No |
| `job_status`, `cancel_job` | Progress of the running job | — | No |
| `search` | Nearest items with cosine similarity (not a probability) | No | No |
| `check_idea` | Calibrated verdict (exists / near / open, P(exists) in this collection, quality) | No | No |
| `list_gaps`, `gap_detail` | Ranked gaps with score terms, timeline, central items, decisions | No | No |
| `scan_gaps` | Score gaps on the latest regions | Yes | No |
| `gap_context` | Evidence for writing a description (nearest items on each side) | No | No |
| `submit_description` | Runs the on-target and novelty checks on an agent-written description; stores it if it passes | Yes | No |
| `submit_paraphrases` | Agent-written paraphrases for calibration | Yes | No |
| `verify_gap` | PubMed and OpenAlex check | Cache | **Yes**, refused while the project is offline |
| `suggest_decision` | Records the agent's suggested decision and reasoning (§7) | Yes | No |
| `export_gaps` | Markdown or CSV report, titles and links only | Yes (file) | No |

### 5.3 Guardrails (unchanged from the app)

- Network use stays explicit: `verify_gap` refuses while the project is offline;
  `INVECTUS_OFFLINE=1` refuses everything; model downloads still need the user's consent and
  are never triggered by a tool.
- No shell or file access beyond the project, the chosen import source and exports.
- One background job at a time, tied to its project; a second is refused with the running
  job's name.
- Embedding spaces never mix; a tool never presents one model's index as another's.
- Every verdict and gap answer carries "in this collection", the calibration quality and,
  for approximate search, that it was approximate.

## 6. Agent-written work keeps the rigor

Where the full app uses its local language model, the agent edition hands the writing to the
agent and keeps the checks in the engine.

- **Gap descriptions.** `gap_context` gives the agent the evidence; it writes a title and
  description; `submit_description` runs the same on-target and novelty checks the local model's
  output goes through today (SPEC §6.3), and stores it only if it passes. Rejected attempts come
  back with the reason, so the agent can try again.
- **Calibration.** The agent writes paraphrases of sampled item titles; `submit_paraphrases`
  fits the curve exactly as `calibrate --llm` does now, recording the writer as the agent and
  its model.
- **Region names.** The agent proposes names; they are stored as agent-written beside the
  c-TF-IDF terms.

## 7. Decision provenance

Today a gap decision records the verdict and a note but not who made it. The target adds:

- an `author` on every decision: `person`, or `agent:<client>/<model>`;
- **suggestions** kept apart from decisions: the app shows "Agent suggests: confirmed gap"
  with its reasoning, and the person accepts, edits or overrules it;
- the same provenance on descriptions, region names and calibration paraphrases.

Data model change: add `author` and `kind` (`decision` | `suggestion`) to `gap_verdicts`, and
`author` to midpoint and label records.

## 8. The app and the agent on the same project

DuckDB allows one writer per database file. Two stages:

1. **First release (simple).** If the app holds a project, the MCP server opens it read-only;
   tools that write answer "this project is open in the inVectus app: close it there or let the
   app run it".
2. **Later (better).** When the app is running, the server forwards requests to it over a
   private local socket (a Unix domain socket or named pipe; never a network port). The agent's
   work then shows live in the app: the circle marks what it searched, its jobs appear in
   Activity, its suggestions appear on the gaps. Without the app, the server opens the project
   directly.

## 9. Human surfaces

- **Desktop app**: the review surface. The data circle, the search and gap panels with their
  evidence, decisions and notes, collections and their readiness, settings.
- **CLI** (`invectus …`): scripting, benchmarks, and people who prefer a terminal.
- **TUI** (optional, later): a terminal version of the panel with a braille-dot circle
  (ratatui canvas), mouse and key pan/zoom. Useful, but lower priority than the agent interface.

## 10. Status

| Piece | State |
|---|---|
| Engine: import, embedding, exact/approximate search, calibration, verdicts, regions, map, gaps, descriptions, verification, backtest | **Built** (see PROGRESS.md for measurements and limits) |
| CLI with JSON output | **Built** |
| Desktop app: data circle, search/gaps/collections panels, import → index → map → scan → calibrate job | **Built**, macOS only verified |
| Job board with project tagging and write guards | **Built**, inside `src-tauri` |
| Shared orchestration crate (job board, pipeline, readiness) usable by app and MCP | Planned |
| `invectus mcp` server and tools (§5) | Planned |
| Agent-written descriptions, paraphrases, region names with checks (§6) | Planned |
| Decision provenance (§7) | Planned |
| Connect-an-agent button, `.mcpb` bundle | Planned |
| App hand-off socket (§8 stage 2) | Planned |
| Headless agent-edition installer | Planned |
| Small local model evaluation | Planned |
| TUI | Optional, later |

## 11. Roadmap

| # | Step | Done when |
|---|---|---|
| A1 | Move the job board, import pipeline and readiness rules from `src-tauri` into a shared crate | App unchanged; its end-to-end test passes against the crate |
| A2 | `invectus mcp` with read tools (projects, status, search, check, gaps, gap detail) | An agent answers "does X exist in this collection?" with verdict, quality and evidence |
| A3 | Write tools (import, scan, jobs, verify, export) and the single-writer handling (§8 stage 1) | An agent imports a CSV and reports gaps end to end |
| A4 | Provenance and suggestions (§7) in the store, MCP and app | Agent suggestions show in the app, separate from the person's decisions |
| A5 | Agent-written descriptions, paraphrases and region names (§6) | Pass rates measured against the local-model baseline |
| A6 | Connect-an-agent button and `.mcpb` bundle | One-click setup for Claude Desktop, Claude Code and Cursor |
| A7 | App hand-off socket (§8 stage 2) | Agent activity appears live in the app |
| A8 | Headless installer; small local model evaluated for the full app | Both editions install cleanly on macOS; Windows/Linux after CI |

## 12. Open questions

1. Bundle the embedding model in the installers, or fetch it on first run with consent?
2. Which agent clients to support for one-click setup first?
3. Should agent suggestions ever become decisions automatically (for example, "already exists"
   when verification finds a close match), or always need a person?
4. Pricing and licensing between the two editions (SPEC §14.5).
