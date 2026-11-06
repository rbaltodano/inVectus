//! The agent tools (target product §5.2), shared by `invectus mcp` and the desktop app's
//! hand-off socket (§8 stage 2): every tool takes JSON arguments and returns JSON, over the
//! same session, job board and engine calls the app uses.

use crate::{
    self as orch, CheckForm, Emit, ImportForm, JobInfo, OpResult, SearchForm, Shared, SharedBoard,
    TableForm, encoder_for, with_session_wait,
};
use invectus_core::Role;
use invectus_index::select_table;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Reminder attached to every verdict and gap answer (AGENTS.md: careful absence claims).
/// Reminder attached to every verdict and gap answer (AGENTS.md: careful absence claims).
const SCOPE: &str = "Results describe this collection only: not global novelty, prior-art or \
                     legal clearance. Nearest neighbours are not proof of absence.";

/// One agent's view of inVectus: a session and job board (its own in `invectus mcp`, the
/// app's when calls come through the app), where job changes go, and who is calling.
pub struct Agent {
    pub session: Shared,
    pub board: SharedBoard,
    pub emit: Emit,
    /// The client's name from the MCP handshake (`clientInfo.name`), for provenance.
    pub client: Option<String>,
    /// Calls through the app work on the app's open project only: opening another is refused.
    pub pinned: Option<PathBuf>,
}

impl Default for Agent {
    fn default() -> Self {
        Self {
            session: Shared::default(),
            board: SharedBoard::default(),
            emit: log_emitter(),
            client: None,
            pinned: None,
        }
    }
}

/// The handshake's client name as a stable id for provenance. Codex introduces itself as
/// `codex-mcp-client`; it is recorded as `codex`, like the connect client id.
pub fn client_id(name: &str) -> String {
    match name.trim() {
        "codex-mcp-client" => "codex".into(),
        other => other.to_string(),
    }
}

/// Job changes are logged to stderr; agents follow progress with `job_status`.
pub fn log_emitter() -> Emit {
    std::sync::Arc::new(|j: &JobInfo| {
        if !j.running() {
            eprintln!("job {} {}: {}", j.id, j.label, j.state);
        }
    })
}

impl Agent {
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

    pub fn run_tool(&mut self, name: &str, args: Value) -> OpResult<Value> {
        let mut result = self.run_tool_inner(name, args)?;
        if let Some(root) = self.open_root() {
            // Read metadata afresh so an already-running client sees the app's next choice.
            let model = with_session_wait(&self.session, |s| s.project.preferred_agent_model())?;
            if let Some(model) = model {
                result["agent_model_preference"] = json!({
                    "requested_model": model,
                    "project": root,
                    "instruction": "For future work, the user requests this model and effort. Model switching is controlled by your client, not inVectus. If you cannot use it, tell the user; do not claim to have switched. Always report your actual model in submissions."
                });
            }
        }
        Ok(result)
    }

    fn run_tool_inner(&mut self, name: &str, args: Value) -> OpResult<Value> {
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
                if self.pinned.is_some() {
                    self.ensure_open(Some(&a.project))?;
                    let board = self.board.clone();
                    let info = with_session_wait(&self.session, |s| orch::project_info(s, &board))?;
                    return value(&info);
                }
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
                let out = orch::import_then_index(&self.emit, &self.session, &self.board, req)?;
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
                let a: CancelArgs = parse(args)?;
                let board = self.board.lock().unwrap();
                let running = board
                    .running()
                    .cloned()
                    .ok_or("that job is no longer running")?;
                board.cancel_matching(a.job_id, running.project.as_deref())?;
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
                    &self.emit,
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
                    project: None,
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
    pub fn author(&self, model: Option<&str>) -> String {
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

    pub fn open_root(&self) -> Option<PathBuf> {
        let guard = self.session.lock().ok()?;
        guard.as_ref().map(|s| s.project.root.clone())
    }

    /// Open `path` unless it is already the open project; with no path, a project must be open.
    pub fn ensure_open(&mut self, path: Option<&str>) -> OpResult<()> {
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
                if let Some(pinned) = &self.pinned {
                    return Err(format!(
                        "the inVectus app has {} open; agent calls through the app work on that \
                         project only",
                        pinned.display()
                    ));
                }
                orch::open_project(&self.session, &self.board, p, false).map(|_| ())
            }
        }
    }
}

fn value<T: serde::Serialize>(v: &T) -> OpResult<Value> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelArgs {
    job_id: u64,
}

fn parse<T: for<'de> Deserialize<'de>>(args: Value) -> OpResult<T> {
    serde_json::from_value(args).map_err(|e| format!("invalid arguments: {e}"))
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
            // MCP paths were resolved by ensure_open; app forms carry their exact root.
            project: None,
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

pub fn tools() -> Value {
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
            "description": "Cancel the job_id returned by job_status or import_collection. \
                Refuses if that job has finished or been replaced. Work done so far is kept.",
            "inputSchema": { "type": "object", "properties": {
                "job_id": { "type": "integer", "minimum": 1 }
            }, "required": ["job_id"], "additionalProperties": false },
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
    #[test]
    fn later_agent_calls_receive_saved_model_preference_without_changing_author() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("preference.invectus");
        let mut agent = super::Agent {
            client: Some("claude-code".into()),
            ..Default::default()
        };
        agent
            .run_tool(
                "open_project",
                serde_json::json!({"project": root, "create": true}),
            )
            .unwrap();
        assert!(
            super::orch::set_agent_model(
                &agent.session,
                &agent.board,
                "/another-project.invectus",
                Some("GPT-6.1 High")
            )
            .is_err()
        );
        super::orch::set_agent_model(
            &agent.session,
            &agent.board,
            root.to_str().unwrap(),
            Some("GPT-6.1 High"),
        )
        .unwrap();
        let response = agent
            .run_tool("project_status", serde_json::json!({}))
            .unwrap();
        assert_eq!(response["preferred_agent_model"], "GPT-6.1 High");
        assert_eq!(
            response["agent_model_preference"]["requested_model"],
            "GPT-6.1 High"
        );
        assert_eq!(
            agent.author(Some("claude-opus-5-5")),
            "agent:claude-code/claude-opus-5-5"
        );
        super::orch::set_agent_model(&agent.session, &agent.board, root.to_str().unwrap(), None)
            .unwrap();
        let response = agent
            .run_tool("project_status", serde_json::json!({}))
            .unwrap();
        assert!(response.get("agent_model_preference").is_none());
    }

    #[test]
    fn codex_is_recorded_under_its_connect_id() {
        assert_eq!(super::client_id(" codex-mcp-client "), "codex");
        assert_eq!(super::client_id("claude-code"), "claude-code");
    }
}
