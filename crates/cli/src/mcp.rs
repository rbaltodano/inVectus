//! `invectus mcp`: a Model Context Protocol server over stdio (target product §5, roadmap A2).
//! One JSON-RPC message per line on stdin, one reply per line on stdout; logs go to stderr. It
//! opens no network port. Read tools (A2) and write tools (A3): imports and other jobs run in
//! the background and are followed with `job_status`. The only network use is `verify_gap`,
//! and only when the project's network switch is on. Writing tools (A5) hand descriptions,
//! calibration paraphrases and region names to the agent and keep the engine's checks.

use anyhow::{Context, Result};
use invectus_core::Role;
use invectus_index::select_table;
use invectus_orchestration::{
    self as orch, CheckForm, Emit, ImportForm, JobInfo, OpResult, SearchForm, Shared, SharedBoard,
    TableForm, encoder_for, with_session_wait,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// Protocol revisions this server speaks; the newest is offered when the client's is unknown.
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// Reminder attached to every verdict and gap answer (AGENTS.md: careful absence claims).
const SCOPE: &str = "Results describe this collection only: not global novelty, prior-art or \
                     legal clearance. Nearest neighbours are not proof of absence.";

pub fn serve() -> Result<()> {
    let mut server = Server::default();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    eprintln!("invectus mcp {}: ready on stdio", invectus_core::VERSION);
    for line in stdin.lock().lines() {
        let line = line.context("reading stdin")?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_line(&line) {
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    server.stop_jobs();
    Ok(())
}

#[derive(Default)]
pub struct Server {
    session: Shared,
    board: SharedBoard,
    /// The client's name from `initialize` (`clientInfo.name`), for provenance.
    client: Option<String>,
}

/// Job changes are logged to stderr; agents follow progress with `job_status`.
fn emitter() -> Emit {
    std::sync::Arc::new(|j: &JobInfo| {
        if !j.running() {
            eprintln!("job {} {}: {}", j.id, j.label, j.state);
        }
    })
}

impl Server {
    /// When the client goes away, cancel a running job and wait for it to stop cleanly (jobs
    /// are resumable; this avoids leaving one half-written).
    pub fn stop_jobs(&self) {
        if self.board.lock().unwrap().running().is_none() {
            return;
        }
        eprintln!("client closed: cancelling the running job");
        self.board.lock().unwrap().cancel();
        let start = std::time::Instant::now();
        while self.board.lock().unwrap().running().is_some()
            && start.elapsed() < std::time::Duration::from_secs(60)
        {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// The reply to one line, or `None` for notifications.
    pub fn handle_line(&mut self, line: &str) -> Option<Value> {
        match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> =
                    batch.into_iter().filter_map(|m| self.handle(m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(message) => self.handle(message),
            Err(e) => Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
        }
    }

    fn handle(&mut self, message: Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        // Notifications (no id) get no reply; replies from the client are ignored too.
        let id = id?;
        let result = match method.as_str() {
            "initialize" => {
                self.client = params["clientInfo"]["name"]
                    .as_str()
                    .map(|n| n.trim().to_string())
                    .filter(|n| !n.is_empty());
                Ok(initialize(&params))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => Ok(self.call(&params)),
            "" => Err((-32600, "invalid request: no method".to_string())),
            other => Err((-32601, format!("method not found: {other}"))),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, msg)) => error(id, code, &msg),
        })
    }

    /// A tool call; tool failures are results with `isError`, so the agent can read and retry.
    fn call(&mut self, params: &Value) -> Value {
        let name = params["name"].as_str().unwrap_or_default();
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        match self.run_tool(name, args) {
            Ok(v) => json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }],
                "structuredContent": v,
                "isError": false,
            }),
            Err(e) => json!({
                "content": [{ "type": "text", "text": e }],
                "isError": true,
            }),
        }
    }

    fn run_tool(&mut self, name: &str, args: Value) -> OpResult<Value> {
        match name {
            "list_projects" => {
                let a: ListArgs = parse(args)?;
                let folder = orch::user_path(&a.folder.unwrap_or_else(orch::default_projects_dir));
                Ok(json!({
                    "folder": folder.display().to_string(),
                    "projects": list_projects(&folder, self.open_root().as_deref()),
                }))
            }
            "open_project" => {
                let a: OpenArgs = parse(args)?;
                let info = orch::open_project(
                    &self.session,
                    &self.board,
                    &a.project,
                    a.create.unwrap_or(false),
                )?;
                value(&info)
            }
            "project_status" => {
                let a: ProjectArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let board = self.board.clone();
                let info = with_session_wait(&self.session, |s| orch::project_info(s, &board))?;
                value(&info)
            }
            "search" => {
                let a: SearchArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let r = orch::search(
                    &self.session,
                    SearchForm {
                        collection: a.collection,
                        query: a.query,
                        k: a.k.unwrap_or(10).clamp(1, 100),
                        role: a.role,
                        exhaustive: a.exhaustive.unwrap_or(false),
                        space: a.space,
                    },
                )?;
                let mut v = value(&r)?;
                v["similarity_note"] = json!(
                    "`similarity` is cosine similarity, not a probability; use check_idea for a \
                     calibrated verdict"
                );
                Ok(v)
            }
            "check_idea" => {
                let a: CheckArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let r = orch::check_idea(
                    &self.session,
                    CheckForm {
                        collection: a.collection,
                        query: a.idea,
                        k: a.k.unwrap_or(10).clamp(1, 100),
                        role: a.role,
                        fast: a.fast.unwrap_or(false),
                        space: a.space,
                    },
                )?;
                let mut v = value(&r)?;
                v["scope"] = json!(SCOPE);
                Ok(v)
            }
            "list_gaps" => {
                let a: TableArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let list = with_session_wait(&self.session, |s| {
                    let table = select_table(&s.project, &a.collection, a.space.as_deref())?;
                    invectus_gaps::list(&s.project, &table)
                })?;
                let mut v = value(&list)?;
                if let Some(limit) = a.limit
                    && let Some(gaps) = v["gaps"].as_array_mut()
                {
                    gaps.truncate(limit.max(1));
                }
                v["scope"] = json!(SCOPE);
                Ok(v)
            }
            "gap_detail" => {
                let a: GapArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let detail = with_session_wait(&self.session, |s| {
                    let table = select_table(&s.project, &a.collection, a.space.as_deref())?;
                    invectus_gaps::detail(&s.project, &table, a.gap_id)
                })?;
                let mut v = value(&detail)?;
                v["scope"] = json!(SCOPE);
                Ok(v)
            }
            "inspect_source" => {
                let a: SourceArgs = parse(args)?;
                value(&orch::inspect_source(&a.source)?)
            }
            "import_collection" => {
                let a: ImportArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let form = a.form();
                let req = orch::request_for_session(&self.session, form)?;
                let out = orch::import_then_index(&emitter(), &self.session, &self.board, req)?;
                let mut v = value(&out)?;
                v["next"] = json!(match (&out.job, &out.index_error) {
                    (Some(_), _) =>
                        "Indexing, mapping, gap scan and calibration run in the \
                                     background: poll job_status until it is no longer running.",
                    (None, Some(_)) => "Imported, but indexing could not start: see index_error.",
                    (None, None) =>
                        "Imported, but no embedding model is installed: see \
                                     plan.missing (the person installs models in the app or with \
                                     `invectus models download`).",
                });
                Ok(v)
            }
            "job_status" => value(&self.board.lock().unwrap().view()),
            "cancel_job" => {
                let board = self.board.lock().unwrap();
                let running = board.running().cloned();
                board.cancel();
                Ok(json!({ "cancelling": running }))
            }
            "scan_gaps" => {
                let a: TableArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let report = orch::scan_gaps(&self.session, &self.board, a.table(), None)?;
                let mut v = value(&report)?;
                v["scope"] = json!(SCOPE);
                Ok(v)
            }
            "verify_gap" => {
                let a: GapArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let r = with_session_wait(&self.session, |s| {
                    let table = select_table(&s.project, &a.collection, a.space.as_deref())?;
                    encoder_for(s, &table)?;
                    let encoder = &mut s.encoder.as_mut().unwrap().1;
                    invectus_verify::verify_gap(
                        &s.project,
                        &table,
                        encoder,
                        a.gap_id,
                        // Never overrides the project's network switch.
                        &invectus_verify::VerifyOptions::default(),
                    )
                })?;
                value(&r)
            }
            "suggest_decision" => {
                let a: SuggestArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let author = self.author(a.model.as_deref());
                let view = with_session_wait(&self.session, |s| {
                    let table = select_table(&s.project, &a.collection, a.space.as_deref())?;
                    invectus_gaps::suggest(
                        &s.project,
                        &table,
                        a.gap_id,
                        &a.verdict,
                        &a.reasoning,
                        &author,
                    )
                })?;
                let mut v = value(&view)?;
                v["next"] = json!(
                    "Recorded as a suggestion. The person accepts, edits or dismisses it in the \
                     app; the gap's decision is unchanged until then."
                );
                Ok(v)
            }
            "gap_context" => {
                let a: DescribeArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let ctx = with_session_wait(&self.session, |s| {
                    let table = select_table(&s.project, &a.collection, a.space.as_deref())?;
                    let target = gap_target(s, &table, a.gap_id, a.t)?;
                    invectus_midpoint::context(
                        &s.project,
                        &s.vectors,
                        &table,
                        &target,
                        &a.options(),
                    )
                })?;
                let mut v = value(&ctx)?;
                v["next"] = json!(
                    "Write one title and a 3-5 sentence description following `prompt`, then call \
                     submit_description with the same gap_id (and t). It is stored only if it \
                     passes the checks."
                );
                Ok(v)
            }
            "submit_description" => {
                let a: DescribeArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let author = self.author(a.model.as_deref());
                let (title, description) = (
                    a.title.clone().ok_or("`title` is required")?,
                    a.description.clone().ok_or("`description` is required")?,
                );
                let r = with_session_wait(&self.session, |s| {
                    let table = select_table(&s.project, &a.collection, a.space.as_deref())?;
                    let target = gap_target(s, &table, a.gap_id, a.t)?;
                    encoder_for(s, &table)?;
                    let encoder = &mut s.encoder.as_mut().unwrap().1;
                    invectus_midpoint::submit(
                        &s.project,
                        &s.vectors,
                        &table,
                        encoder,
                        &target,
                        &title,
                        &description,
                        &author,
                        &a.options(),
                    )
                })?;
                let mut v = value(&r)?;
                v["stored"] = json!(r.accepted.is_some());
                v["scope"] = json!(SCOPE);
                Ok(v)
            }
            "paraphrase_sample" => {
                let a: SampleArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let sample = with_session_wait(&self.session, |s| {
                    let table = select_table(&s.project, &a.collection, a.space.as_deref())?;
                    invectus_unsearch::paraphrase_sample(
                        &s.project,
                        &s.vectors,
                        &table,
                        &invectus_unsearch::CalibrateOptions::default(),
                    )
                })?;
                let (offset, limit) = (a.offset.unwrap_or(0), a.limit.unwrap_or(50).clamp(1, 300));
                let items: Vec<Value> = sample
                    .iter()
                    .skip(offset)
                    .take(limit)
                    .map(|i| {
                        // Item ids exceed JavaScript's exact integer range: strings.
                        json!({ "item_id": i.item_id.to_string(), "title": i.title,
                                "text_start": i.text_start })
                    })
                    .collect();
                Ok(json!({
                    "total": sample.len(),
                    "offset": offset,
                    "items": items,
                    "system": invectus_unsearch::calibrate::PARAPHRASE_SYSTEM,
                    "rule": invectus_unsearch::calibrate::PARAPHRASE_RULE,
                    "next": "Paraphrase every item (page with offset/limit), then call \
                             submit_paraphrases once with all of them. At least 30 must pass \
                             screening: not empty, not the title again, at most 80 words.",
                }))
            }
            "submit_paraphrases" => {
                let a: ParaphraseArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let mut given = std::collections::HashMap::new();
                for p in a.paraphrases {
                    let id: i64 = p
                        .item_id
                        .trim()
                        .parse()
                        .map_err(|_| format!("item_id {:?} is not an item id", p.item_id))?;
                    given.insert(id, p.paraphrase);
                }
                let out = orch::start_calibration_from_paraphrases(
                    &emitter(),
                    &self.session,
                    &self.board,
                    a.collection,
                    a.space,
                    self.author(a.model.as_deref()),
                    given,
                )?;
                Ok(json!({
                    "accepted": out.accepted,
                    "rejected": out.rejected.iter()
                        .map(|(id, r)| json!({ "item_id": id.to_string(), "reason": r }))
                        .collect::<Vec<_>>(),
                    "job": out.job,
                    "next": "The calibration runs in the background: poll job_status. When it \
                             completes, check_idea uses the new curve.",
                }))
            }
            "region_context" => {
                let a: RegionArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let (run_id, regions) = with_session_wait(&self.session, |s| {
                    let table = select_table(&s.project, &a.collection, a.space.as_deref())?;
                    invectus_regions::llm_labels::region_context(
                        &s.project,
                        &table,
                        a.unnamed_only.unwrap_or(true),
                    )
                })?;
                let (offset, limit) = (a.offset.unwrap_or(0), a.limit.unwrap_or(20).clamp(1, 100));
                Ok(json!({
                    "run_id": run_id,
                    "total": regions.len(),
                    "offset": offset,
                    "regions": regions.iter().skip(offset).take(limit).collect::<Vec<_>>(),
                    "system": invectus_regions::llm_labels::SYSTEM,
                    "next": "Name each region from its terms and titles (3 to 8 words), then call \
                             submit_region_names.",
                }))
            }
            "submit_region_names" => {
                let a: NamesArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let author = self.author(a.model.as_deref());
                let names: Vec<(i32, String)> =
                    a.names.into_iter().map(|n| (n.region, n.name)).collect();
                let report = with_session_wait(&self.session, |s| {
                    let table = select_table(&s.project, &a.collection, a.space.as_deref())?;
                    invectus_regions::llm_labels::set_names(&s.project, &table, &names, &author)
                })?;
                value(&report)
            }
            "export_gaps" => {
                let a: ExportArgs = parse(args)?;
                self.ensure_open(a.project.as_deref())?;
                let form = TableForm {
                    collection: a.collection,
                    space: a.space,
                };
                let path = orch::export_gaps(&self.session, form, &a.format)?;
                Ok(json!({ "path": path }))
            }
            other => Err(format!("unknown tool {other:?}")),
        }
    }

    /// `agent:<client>/<model>`, from the handshake and the model the agent names.
    fn author(&self, model: Option<&str>) -> String {
        let clean = |s: &str| {
            s.chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
                .trim()
                .replace('/', "-")
        };
        let client = clean(self.client.as_deref().unwrap_or("unknown-client"));
        match model.map(clean).filter(|m| !m.is_empty()) {
            Some(m) => format!("agent:{client}/{m}"),
            None => format!("agent:{client}"),
        }
    }

    fn open_root(&self) -> Option<PathBuf> {
        let guard = self.session.lock().ok()?;
        guard.as_ref().map(|s| s.project.root.clone())
    }

    /// Open `path` unless it is already the open project; with no path, a project must be open.
    fn ensure_open(&mut self, path: Option<&str>) -> OpResult<()> {
        let open = self.open_root();
        match path {
            None if open.is_some() => Ok(()),
            None => Err("no project is open: pass `project` or call open_project first".into()),
            Some(p) => {
                let wanted = orch::user_path(p);
                let same = |root: &Path| {
                    let canon = |x: &Path| std::fs::canonicalize(x).ok();
                    canon(root).is_some()
                        && (canon(root) == canon(&wanted)
                            || canon(root) == canon(&wanted.with_extension("invectus")))
                };
                if open.as_deref().is_some_and(same) {
                    return Ok(());
                }
                orch::open_project(&self.session, &self.board, p, false).map(|_| ())
            }
        }
    }
}

fn value<T: serde::Serialize>(v: &T) -> OpResult<Value> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

fn parse<T: for<'de> Deserialize<'de>>(args: Value) -> OpResult<T> {
    serde_json::from_value(args).map_err(|e| format!("invalid arguments: {e}"))
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn initialize(params: &Value) -> Value {
    let asked = params["protocolVersion"].as_str().unwrap_or_default();
    let version = PROTOCOLS
        .iter()
        .find(|v| **v == asked)
        .unwrap_or(&PROTOCOLS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "invectus", "version": invectus_core::VERSION },
        "instructions": "inVectus answers whether an idea already exists in a local collection \
            and where its gaps are. Open a project (list_projects, open_project), read \
            project_status for collections and readiness, then use check_idea for a calibrated \
            verdict (exists / near / open) or search for nearest items. Verdicts hold for this \
            collection only. You can also write for the collection: gap descriptions \
            (gap_context, submit_description), calibration paraphrases (paraphrase_sample, \
            submit_paraphrases) and region names (region_context, submit_region_names); the \
            engine checks your writing and records you as its author. Suggest decisions with \
            suggest_decision; the person makes them. Everything runs offline on this computer.",
    })
}

/// Project folders (`*.invectus` with a `project.json`) directly inside `folder`.
fn list_projects(folder: &Path, open: Option<&Path>) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let open = open.and_then(|p| std::fs::canonicalize(p).ok());
    let mut out: Vec<Value> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter_map(|root| {
            let raw = std::fs::read(root.join("project.json")).ok()?;
            let config: Value = serde_json::from_slice(&raw).ok()?;
            Some(json!({
                "root": root.display().to_string(),
                "name": config["name"].as_str().unwrap_or_default(),
                "open": open.is_some() && std::fs::canonicalize(&root).ok() == open,
            }))
        })
        .collect();
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    out
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    folder: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectArgs {
    project: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenArgs {
    project: String,
    create: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceArgs {
    source: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportArgs {
    project: Option<String>,
    collection: String,
    source: String,
    #[serde(default)]
    text: Vec<String>,
    title: Option<String>,
    id: Option<String>,
    date: Option<String>,
    url: Option<String>,
    role: Option<Role>,
    role_column: Option<String>,
    role_rules: Option<String>,
    limit: Option<usize>,
}

impl ImportArgs {
    /// The app's defaults: token chunks for the default embedding model when it is installed,
    /// else 2,000-character chunks. A collection that exists keeps its saved chunking.
    fn form(self) -> ImportForm {
        use invectus_embed::{ModelStore, registry};
        use invectus_ingest::TokenChunking;
        let tokenizer = ModelStore::default_root()
            .ok()
            .filter(|r| {
                registry::spec(registry::DEFAULT_MODEL)
                    .is_ok_and(|m| ModelStore::new(r.clone()).is_installed(m))
            })
            .map(|_| registry::DEFAULT_MODEL.to_string());
        ImportForm {
            collection: self.collection,
            source_path: self.source,
            text: self.text,
            title: self.title,
            id: self.id,
            date: self.date,
            url: self.url,
            role: self.role,
            role_column: self.role_column,
            role_rules: self.role_rules,
            chunk_tokenizer: tokenizer,
            chunk_tokens: TokenChunking::DEFAULT_MAX_TOKENS,
            chunk_token_overlap: TokenChunking::DEFAULT_OVERLAP_TOKENS,
            chunk_chars: 2000,
            chunk_overlap: 200,
            limit: self.limit,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestArgs {
    project: Option<String>,
    collection: String,
    space: Option<String>,
    gap_id: i32,
    verdict: String,
    reasoning: String,
    model: Option<String>,
}

/// The description target of a gap: its region, or the point `t` between a bridge's sides.
fn gap_target(
    s: &orch::Session,
    table: &invectus_core::store::VectorTableRecord,
    gap_id: i32,
    t: Option<f32>,
) -> anyhow::Result<invectus_midpoint::Target> {
    let g = invectus_gaps::detail(&s.project, table, gap_id)?.view.gap;
    Ok(match g.region_b {
        Some(b) => invectus_midpoint::Target::Between {
            a: g.region_a,
            b,
            t: t.unwrap_or(0.5),
        },
        None => invectus_midpoint::Target::Region { region: g.region_a },
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DescribeArgs {
    project: Option<String>,
    collection: String,
    space: Option<String>,
    gap_id: i32,
    t: Option<f32>,
    genre: Option<String>,
    coverage_genre: Option<String>,
    // submit_description only.
    title: Option<String>,
    description: Option<String>,
    model: Option<String>,
}

impl DescribeArgs {
    fn options(&self) -> invectus_midpoint::DescribeOptions {
        let d = invectus_midpoint::DescribeOptions::default();
        invectus_midpoint::DescribeOptions {
            genre: self.genre.clone().unwrap_or(d.genre),
            coverage_genre: self.coverage_genre.clone().unwrap_or(d.coverage_genre),
            ..d
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SampleArgs {
    project: Option<String>,
    collection: String,
    space: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Paraphrase {
    item_id: String,
    paraphrase: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParaphraseArgs {
    project: Option<String>,
    collection: String,
    space: Option<String>,
    paraphrases: Vec<Paraphrase>,
    model: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegionArgs {
    project: Option<String>,
    collection: String,
    space: Option<String>,
    unnamed_only: Option<bool>,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegionName {
    region: i32,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NamesArgs {
    project: Option<String>,
    collection: String,
    space: Option<String>,
    names: Vec<RegionName>,
    model: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportArgs {
    project: Option<String>,
    collection: String,
    space: Option<String>,
    /// `markdown` or `csv`.
    format: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    project: Option<String>,
    collection: String,
    query: String,
    k: Option<usize>,
    role: Option<Role>,
    exhaustive: Option<bool>,
    space: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckArgs {
    project: Option<String>,
    collection: String,
    idea: String,
    k: Option<usize>,
    role: Option<Role>,
    fast: Option<bool>,
    space: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableArgs {
    project: Option<String>,
    collection: String,
    space: Option<String>,
    limit: Option<usize>,
}

impl TableArgs {
    fn table(self) -> TableForm {
        TableForm {
            collection: self.collection,
            space: self.space,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GapArgs {
    project: Option<String>,
    collection: String,
    space: Option<String>,
    gap_id: i32,
}

fn tools() -> Value {
    let project = json!({
        "type": "string",
        "description": "Project folder (`<name>.invectus`). Optional once a project is open."
    });
    let collection =
        json!({ "type": "string", "description": "Collection id (see project_status)." });
    let space = json!({
        "type": "string",
        "description": "Vector space id; needed only when the collection has several."
    });
    let role = json!({
        "type": "string",
        "enum": ["demand", "coverage", "neutral"],
        "description": "Item role: `demand` (asks for or needs something, e.g. studies), `coverage` (answers it, e.g. systematic reviews) or `neutral`."
    });
    let read_only = json!({ "readOnlyHint": true, "openWorldHint": false });
    let writes = json!({ "readOnlyHint": false, "destructiveHint": false, "openWorldHint": false });
    json!([
        {
            "name": "list_projects",
            "description": "inVectus projects in a folder (default: ~/inVectus Projects).",
            "inputSchema": { "type": "object", "properties": {
                "folder": { "type": "string", "description": "Folder to look in." }
            }, "additionalProperties": false },
            "annotations": read_only,
        },
        {
            "name": "open_project",
            "description": "Open a project for the following calls; returns its status. With \
                `create`, make a new empty project there.",
            "inputSchema": { "type": "object", "properties": { "project": project,
                "create": { "type": "boolean", "default": false } },
                "required": ["project"], "additionalProperties": false },
            "annotations": writes,
        },
        {
            "name": "project_status",
            "description": "Collections, vector spaces and readiness: index status, calibration \
                (needed by check_idea) and regions/gap scans (needed by list_gaps).",
            "inputSchema": { "type": "object", "properties": { "project": project },
                "additionalProperties": false },
            "annotations": read_only,
        },
        {
            "name": "search",
            "description": "Nearest items to a query with cosine similarity (not a probability). \
                Approximate unless `exhaustive`; the result says which.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection,
                "query": { "type": "string" },
                "k": { "type": "integer", "minimum": 1, "maximum": 100, "default": 10 },
                "role": role, "space": space,
                "exhaustive": { "type": "boolean", "default": false,
                    "description": "Compare every stored vector (exact)." }
            }, "required": ["collection", "query"], "additionalProperties": false },
            "annotations": read_only,
        },
        {
            "name": "check_idea",
            "description": "Does an idea already exist in this collection? Calibrated verdict \
                (exists / near / open) with P(exists), the calibration quality and the nearest \
                items as evidence. Exhaustive by default. Holds for this collection only.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection,
                "idea": { "type": "string", "description": "The idea, as a title or a few sentences." },
                "k": { "type": "integer", "minimum": 1, "maximum": 100, "default": 10 },
                "role": role, "space": space,
                "fast": { "type": "boolean", "default": false,
                    "description": "Use the approximate index when there is one." }
            }, "required": ["collection", "idea"], "additionalProperties": false },
            "annotations": read_only,
        },
        {
            "name": "list_gaps",
            "description": "Ranked gaps of the latest scan with score terms and decisions.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection, "space": space,
                "limit": { "type": "integer", "minimum": 1, "description": "Return the top N." }
            }, "required": ["collection"], "additionalProperties": false },
            "annotations": read_only,
        },
        {
            "name": "gap_detail",
            "description": "One gap: score terms, timeline, central items on each side, \
                description and decision.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection, "space": space,
                "gap_id": { "type": "integer" }
            }, "required": ["collection", "gap_id"], "additionalProperties": false },
            "annotations": read_only,
        },
        {
            "name": "inspect_source",
            "description": "Columns and sample rows of a CSV/TSV, Parquet file or folder of text \
                files, to choose the mapping for import_collection.",
            "inputSchema": { "type": "object", "properties": {
                "source": { "type": "string", "description": "File or folder path." }
            }, "required": ["source"], "additionalProperties": false },
            "annotations": read_only,
        },
        {
            "name": "import_collection",
            "description": "Import a source into a collection (new or existing), then start \
                indexing, mapping, a gap scan and calibration as one background job (follow it \
                with job_status). Re-importing updates changed items; unchanged ones are kept.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection,
                "source": { "type": "string", "description": "File or folder path." },
                "text": { "type": "array", "items": { "type": "string" },
                    "description": "Text column(s), joined in order. Not used for folders." },
                "title": { "type": "string", "description": "Title column." },
                "id": { "type": "string", "description": "Stable id column (else row hashes)." },
                "date": { "type": "string", "description": "Date or year column." },
                "url": { "type": "string", "description": "Link column." },
                "role": role,
                "role_column": { "type": "string", "description": "Column that sets each \
                    item's role, with role_rules like `review=coverage,study=demand`." },
                "role_rules": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1, "description": "Import the first N rows." }
            }, "required": ["collection", "source"], "additionalProperties": false },
            "annotations": writes,
        },
        {
            "name": "job_status",
            "description": "The running background job (phase, progress) and this session's \
                finished jobs with their reports or errors.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": read_only,
        },
        {
            "name": "cancel_job",
            "description": "Cancel the running job. Work done so far is kept; running it again \
                resumes.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": writes,
        },
        {
            "name": "scan_gaps",
            "description": "Score gaps again on the latest regions (seconds; decisions are kept).",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection, "space": space
            }, "required": ["collection"], "additionalProperties": false },
            "annotations": writes,
        },
        {
            "name": "verify_gap",
            "description": "Check a gap against PubMed and OpenAlex. Uses the network, so it is \
                refused unless the project's network switch is on (the person turns it on in \
                the app); results are cached in the project.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection, "space": space,
                "gap_id": { "type": "integer" }
            }, "required": ["collection", "gap_id"], "additionalProperties": false },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "openWorldHint": true },
        },
        {
            "name": "suggest_decision",
            "description": "Suggest a decision on a gap, with your reasoning. It is kept apart \
                from the person's decision and shown to them in the app as the agent's \
                suggestion; it never becomes a decision without them.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection, "space": space,
                "gap_id": { "type": "integer" },
                "verdict": { "type": "string",
                    "enum": ["open", "confirmed_gap", "already_exists", "not_viable", "selected"] },
                "reasoning": { "type": "string", "description": "What you checked and why: the \
                    person reads this next to the evidence." },
                "model": { "type": "string", "description": "Your model's name, recorded with \
                    the suggestion (e.g. claude-opus-5-5)." }
            }, "required": ["collection", "gap_id", "verdict", "reasoning"],
                "additionalProperties": false },
            "annotations": writes,
        },
        {
            "name": "gap_context",
            "description": "Evidence and instructions for describing what would fill a gap: the \
                same prompt the local language model gets (titles and roles only) and the checks \
                a description must pass. For bridges, `t` (0 to 1) picks the point between the \
                two sides.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection, "space": space,
                "gap_id": { "type": "integer" },
                "t": { "type": "number", "minimum": 0, "maximum": 1, "default": 0.5 },
                "genre": { "type": "string", "description": "What one item is, e.g. \"research study\"." },
                "coverage_genre": { "type": "string", "description": "What a coverage item is, e.g. \"systematic review\"." }
            }, "required": ["collection", "gap_id"], "additionalProperties": false },
            "annotations": read_only,
        },
        {
            "name": "submit_description",
            "description": "Submit your title and description for a gap. The engine checks it \
                lands on the gap's point and is not already in the collection (calibrated), and \
                stores it only if both pass; otherwise the reasons come back so you can revise.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection, "space": space,
                "gap_id": { "type": "integer" },
                "t": { "type": "number", "minimum": 0, "maximum": 1, "default": 0.5 },
                "title": { "type": "string" }, "description": { "type": "string" },
                "genre": { "type": "string" }, "coverage_genre": { "type": "string" },
                "model": { "type": "string", "description": "Your model's name, recorded as the writer." }
            }, "required": ["collection", "gap_id", "title", "description"],
                "additionalProperties": false },
            "annotations": writes,
        },
        {
            "name": "paraphrase_sample",
            "description": "The items calibration samples (title and start of text), with the \
                paraphrase instructions, paged. Paraphrase them for submit_paraphrases.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection, "space": space,
                "offset": { "type": "integer", "minimum": 0, "default": 0 },
                "limit": { "type": "integer", "minimum": 1, "maximum": 300, "default": 50 }
            }, "required": ["collection"], "additionalProperties": false },
            "annotations": read_only,
        },
        {
            "name": "submit_paraphrases",
            "description": "Fit the collection's calibration (what similarity means \"the same \
                thing\") from your paraphrases, exactly as with the local model's. Screened first; \
                runs as a background job (job_status). Replaces the current calibration.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection, "space": space,
                "paraphrases": { "type": "array", "items": { "type": "object", "properties": {
                    "item_id": { "type": "string" }, "paraphrase": { "type": "string" }
                }, "required": ["item_id", "paraphrase"], "additionalProperties": false } },
                "model": { "type": "string", "description": "Your model's name, recorded as the writer." }
            }, "required": ["collection", "paraphrases"], "additionalProperties": false },
            "annotations": writes,
        },
        {
            "name": "region_context",
            "description": "Fine regions of the latest map with their distinctive terms and \
                central titles, to name them (unnamed only by default), paged.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection, "space": space,
                "unnamed_only": { "type": "boolean", "default": true },
                "offset": { "type": "integer", "minimum": 0, "default": 0 },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 }
            }, "required": ["collection"], "additionalProperties": false },
            "annotations": read_only,
        },
        {
            "name": "submit_region_names",
            "description": "Store readable names for regions beside their term labels, recorded \
                as written by you. Replaces an existing name.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection, "space": space,
                "names": { "type": "array", "items": { "type": "object", "properties": {
                    "region": { "type": "integer" }, "name": { "type": "string" }
                }, "required": ["region", "name"], "additionalProperties": false } },
                "model": { "type": "string", "description": "Your model's name, recorded as the writer." }
            }, "required": ["collection", "names"], "additionalProperties": false },
            "annotations": writes,
        },
        {
            "name": "export_gaps",
            "description": "Write the latest gaps as Markdown or CSV (titles and links only) into \
                the project's exports folder; returns the file path.",
            "inputSchema": { "type": "object", "properties": {
                "project": project, "collection": collection, "space": space,
                "format": { "type": "string", "enum": ["markdown", "csv"] }
            }, "required": ["collection", "format"], "additionalProperties": false },
            "annotations": writes,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(s: &mut Server, id: i64, method: &str, params: Value) -> Value {
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        s.handle_line(&line.to_string()).unwrap()
    }

    #[test]
    fn handshake_tools_and_errors() {
        let mut s = Server::default();
        let r = call(
            &mut s,
            1,
            "initialize",
            json!({ "protocolVersion": "2025-03-26" }),
        );
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        let r = call(
            &mut s,
            2,
            "initialize",
            json!({ "protocolVersion": "1999-01-01" }),
        );
        assert_eq!(r["result"]["protocolVersion"], PROTOCOLS[0]);
        assert!(
            s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .is_none()
        );
        let r = call(&mut s, 3, "tools/list", json!({}));
        let names: Vec<&str> = r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "list_projects",
                "open_project",
                "project_status",
                "search",
                "check_idea",
                "list_gaps",
                "gap_detail",
                "inspect_source",
                "import_collection",
                "job_status",
                "cancel_job",
                "scan_gaps",
                "verify_gap",
                "suggest_decision",
                "gap_context",
                "submit_description",
                "paraphrase_sample",
                "submit_paraphrases",
                "region_context",
                "submit_region_names",
                "export_gaps"
            ]
        );
        assert_eq!(call(&mut s, 4, "nope", json!({}))["error"]["code"], -32601);
        assert_eq!(s.handle_line("{not json").unwrap()["error"]["code"], -32700);
        // Tool failures are results the agent can read.
        let r = call(
            &mut s,
            5,
            "tools/call",
            json!({ "name": "project_status", "arguments": {} }),
        );
        assert_eq!(r["result"]["isError"], true);
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("no project is open")
        );
        let r = call(
            &mut s,
            6,
            "tools/call",
            json!({ "name": "search", "arguments": { "collection": "c" , "bogus": 1 } }),
        );
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("invalid arguments")
        );
    }

    #[test]
    fn lists_and_opens_projects() {
        let dir = tempfile::tempdir().unwrap();
        invectus_core::Project::create(&dir.path().join("b")).unwrap();
        invectus_core::Project::create(&dir.path().join("a")).unwrap();
        std::fs::create_dir(dir.path().join("not-a-project")).unwrap();
        let mut s = Server::default();
        let folder = dir.path().display().to_string();
        let r = call(
            &mut s,
            1,
            "tools/call",
            json!({ "name": "list_projects", "arguments": { "folder": folder } }),
        );
        let projects = &r["result"]["structuredContent"]["projects"];
        assert_eq!(projects.as_array().unwrap().len(), 2);
        assert_eq!(projects[0]["name"], "a");
        assert_eq!(projects[0]["open"], false);
        let a = projects[0]["root"].as_str().unwrap().to_string();
        let r = call(
            &mut s,
            2,
            "tools/call",
            json!({ "name": "project_status", "arguments": { "project": a } }),
        );
        assert_eq!(r["result"]["isError"], false, "{r}");
        assert_eq!(r["result"]["structuredContent"]["name"], "a");
        let r = call(
            &mut s,
            3,
            "tools/call",
            json!({ "name": "list_projects", "arguments": { "folder": folder } }),
        );
        assert_eq!(
            r["result"]["structuredContent"]["projects"][0]["open"],
            true
        );
        // Without a path, the open project answers.
        let r = call(
            &mut s,
            4,
            "tools/call",
            json!({ "name": "project_status", "arguments": {} }),
        );
        assert_eq!(r["result"]["structuredContent"]["collections"], json!([]));
    }

    /// The A2 goal: an agent asks whether an idea exists and gets a verdict with its quality
    /// and evidence. Import → index → map → calibrate through the shared pipeline, then
    /// `check_idea` over the server. Needs the dev BGE model (INVECTUS_MODELS_DIR).
    #[test]
    fn check_idea_answers_with_verdict_quality_and_evidence() {
        use invectus_embed::{ModelStore, registry};
        let installed = ModelStore::default_root().ok().is_some_and(|r| {
            ModelStore::new(r).is_installed(registry::spec(registry::DEFAULT_MODEL).unwrap())
        });
        if !installed {
            eprintln!("skipped: {} is not installed", registry::DEFAULT_MODEL);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("notes.csv");
        let topics = [
            "retinal detachment surgery outcomes",
            "glaucoma intraocular pressure medication",
            "myopia progression in children",
            "corneal transplant rejection",
        ];
        let mut rows = String::from("id,title,text\n");
        for i in 0..240 {
            let t = topics[i % 4];
            rows.push_str(&format!(
                "{i},Study {i} on {t},\"Patients in cohort {i} were followed for {t}. We \
                 report baseline characteristics, the intervention schedule, adverse events and \
                 outcomes at one and two years for case series {i}, with {} participants.\"\n",
                40 + i
            ));
        }
        std::fs::write(&csv, rows).unwrap();
        let mut s = Server::default();
        let root = dir.path().join("p");
        orch::open_project(&s.session, &s.board, root.to_str().unwrap(), true).unwrap();
        let form: orch::ImportForm = serde_json::from_value(json!({
            "collection": "notes", "source_path": csv, "text": ["text"], "title": "title",
            "id": "id", "date": null, "url": null, "role": "neutral", "role_column": null,
            "role_rules": null, "chunk_chars": 2000, "chunk_overlap": 200, "limit": null
        }))
        .unwrap();
        let req = orch::request_for_session(&s.session, form).unwrap();
        let emit: orch::Emit = std::sync::Arc::new(|_: &orch::JobInfo| {});
        let out = orch::import_then_index(&emit, &s.session, &s.board, req).unwrap();
        assert!(out.job.is_some(), "{:?}", out.index_error);
        let start = std::time::Instant::now();
        while s.board.lock().unwrap().running().is_some() {
            assert!(start.elapsed().as_secs() < 300, "job did not finish");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let r = call(
            &mut s,
            1,
            "tools/call",
            json!({ "name": "check_idea", "arguments": {
                "collection": "notes", "idea": "glaucoma eye pressure drugs", "k": 5
            } }),
        );
        assert_eq!(r["result"]["isError"], false, "{r}");
        let v = &r["result"]["structuredContent"];
        assert!(
            ["exists", "near", "open"].contains(&v["verdict"].as_str().unwrap()),
            "{v}"
        );
        assert!(v["p_exists"].is_number());
        assert!(!v["calibration"]["quality"].as_str().unwrap().is_empty());
        assert_eq!(v["search"]["exhaustive"], true);
        let top = v["search"]["hits"][0]["item"]["title"].as_str().unwrap();
        assert!(top.contains("glaucoma"), "{top}");
        assert!(
            v["scope"]
                .as_str()
                .unwrap()
                .contains("this collection only")
        );

        let r = call(
            &mut s,
            2,
            "tools/call",
            json!({ "name": "list_gaps", "arguments": { "collection": "notes", "limit": 3 } }),
        );
        assert_eq!(r["result"]["isError"], false, "{r}");
        let gaps = r["result"]["structuredContent"]["gaps"].as_array().unwrap();
        assert!(gaps.len() <= 3);
        let id = gaps
            .first()
            .and_then(|g| g["gap_id"].as_i64())
            .expect("gaps found");
        {
            let r = call(
                &mut s,
                3,
                "tools/call",
                json!({ "name": "gap_detail", "arguments": { "collection": "notes", "gap_id": id } }),
            );
            assert_eq!(r["result"]["isError"], false, "{r}");
        }
    }
}
