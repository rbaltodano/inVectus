//! Desktop shell: Tauri commands over the same engine crates as the `invectus` CLI.

mod jobs;

use invectus_core::store::{CollectionSummary, JobRecord, VectorTableRecord};
use invectus_core::{CancelToken, Project, Role};
use invectus_embed::{EmbedOptions, ModelStore, OnnxEmbedder, query_encoder, registry};
use invectus_gaps::{GapDetail, GapList, GapView, ScanOptions, ScanReport};
use invectus_index::{
    EmbedJobOptions, SearchMode, SearchOptions, SearchResult, VectorStore, run_embed,
    search as engine_search, select_table,
};
use invectus_ingest::{
    ChunkPolicy, ImportReport, ImportRequest, Mapping, MetaMapping, Preview, RoleMapping, Source,
};
use invectus_regions::{MapData, RegionOptions, map_data, run_regions};
use invectus_unsearch::{CalibrateOptions, CheckOptions, CheckResult, check, run_calibration};
use jobs::{Board, ExistingSpace, IndexPlan, JobInfo, JobsView};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

struct Session {
    project: Project,
    vectors: VectorStore,
    /// Query encoder cached per vector space id.
    encoder: Option<(String, OnnxEmbedder)>,
    /// The local language model, loaded on first use and kept for the session.
    llm: Option<invectus_llm::LlamaGenerator>,
}

#[derive(Default)]
struct AppState {
    session: Shared,
    /// The background job (one at a time) and this session's finished jobs.
    board: SharedBoard,
}

type SharedBoard = Arc<Mutex<Board>>;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

type Shared = Arc<Mutex<Option<Session>>>;

/// Run engine work on its own OS thread: the engine drives its own Tokio runtime for LanceDB,
/// which must not be nested inside Tauri's async runtime, and long work must not block the UI.
async fn off_thread<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    let (tx, rx) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.await.map_err(err)?
}

fn with_session<T>(
    session: &Shared,
    f: impl FnOnce(&mut Session) -> anyhow::Result<T>,
) -> CmdResult<T> {
    let mut guard = session
        .try_lock()
        .map_err(|_| "busy: a job is running in this project".to_string())?;
    let s = guard.as_mut().ok_or("no project is open")?;
    f(s).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn engine_version() -> &'static str {
    invectus_core::VERSION
}

#[derive(Serialize)]
struct TableInfo {
    collection: String,
    space_id: String,
    model_key: String,
    runtime: String,
    dimensions: usize,
    origin: String,
    vectors: usize,
    ann_partitions: Option<u64>,
    calibration: Option<CalibrationStatus>,
    regions: Option<RegionsStatus>,
    /// `indexing`, `indexed`, `outdated`, `interrupted`, `failed`, `cancelled`, `not_indexed`,
    /// `imported` (query-only vectors) or `unknown` (see `jobs::index_status`).
    index_status: String,
    /// When the last embed job of this space was recorded, and its error if it failed.
    indexed_at: Option<String>,
    index_error: Option<String>,
}

#[derive(Serialize)]
struct CalibrationStatus {
    method: String,
    sample: i64,
    /// Vectors when fitted; differs from `vectors` when the collection changed since.
    vectors: i64,
    fitted_at: Option<String>,
    holdout_auc: Option<f64>,
}

#[derive(Serialize)]
struct RegionsStatus {
    run_id: String,
    created_at: Option<String>,
    stats: serde_json::Value,
    /// The latest regions have a gap scan.
    scanned: bool,
}

#[derive(Serialize)]
struct CollectionInfo {
    #[serde(flatten)]
    summary: CollectionSummary,
    /// `csv`, `parquet`, `folder` or another importer's kind, and the source path when known.
    source_kind: Option<String>,
    source_path: Option<String>,
    /// The stored source can be imported again (`reimport_collection`).
    reimportable: bool,
}

#[derive(Serialize)]
struct ProjectInfo {
    root: String,
    name: String,
    warning: Option<String>,
    network_enabled: bool,
    /// `INVECTUS_OFFLINE=1`: every network use is refused whatever the project switch says.
    offline_env: bool,
    collections: Vec<CollectionInfo>,
    tables: Vec<TableInfo>,
    /// Recorded jobs of this project, newest first (they survive restarts).
    activity: Vec<JobRecord>,
    /// The job running in this project now, if any.
    running: Option<JobInfo>,
}

fn info(s: &Session, board: &SharedBoard) -> anyhow::Result<ProjectInfo> {
    let root = s.project.root.display().to_string();
    let running = board.lock().unwrap().running_in(&root).cloned();
    let store = &s.project.store;
    let summaries = store.collection_summaries()?;
    let updated = |c: &str| {
        summaries
            .iter()
            .find(|x| x.collection_id == c)
            .and_then(|x| x.updated_at.clone())
    };
    let tables = store
        .vector_tables(None)?
        .into_iter()
        .map(|t| {
            let calibration = store
                .latest_calibration(&t.collection_id, &t.space_id)?
                .map(|c| CalibrationStatus {
                    holdout_auc: c.metrics["holdout"]["auc"].as_f64(),
                    method: c.method,
                    sample: c.sample,
                    vectors: c.vectors,
                    fitted_at: c.created_at,
                });
            let regions = match store.latest_region_run(&t.collection_id, &t.space_id)? {
                Some(run) => Some(RegionsStatus {
                    scanned: store.gaps(&run.run_id)?.is_some(),
                    run_id: run.run_id,
                    created_at: run.created_at,
                    stats: run.stats,
                }),
                None => None,
            };
            let vectors = s.vectors.count(&t)?;
            let job = store.job(&invectus_index::embed_job::job_id(
                &t.collection_id,
                &t.space_id,
            ))?;
            let indexing_now = running.as_ref().is_some_and(|j| {
                j.kind == "index"
                    && j.collection.as_deref() == Some(t.collection_id.as_str())
                    && j.models.contains(&t.identity.model_key)
            });
            let status = jobs::index_status(
                &t.origin,
                job.as_ref()
                    .map(|j| (j.state.as_str(), j.updated_at.as_deref())),
                updated(&t.collection_id).as_deref(),
                vectors,
                indexing_now,
            );
            Ok(TableInfo {
                calibration,
                regions,
                vectors,
                index_status: status.into(),
                indexed_at: job.as_ref().and_then(|j| j.updated_at.clone()),
                index_error: job
                    .as_ref()
                    .and_then(|j| j.checkpoint["error"].as_str().map(String::from)),
                collection: t.collection_id,
                space_id: t.space_id,
                model_key: t.identity.model_key,
                runtime: t.identity.runtime,
                dimensions: t.identity.dimensions,
                origin: t.origin,
                ann_partitions: t.ann.and_then(|a| a["partitions"].as_u64()),
            })
        })
        .collect::<anyhow::Result<_>>()?;
    let collections = summaries
        .into_iter()
        .map(|summary| CollectionInfo {
            source_kind: summary.source["kind"].as_str().map(String::from),
            source_path: summary.source["path"].as_str().map(String::from),
            reimportable: serde_json::from_value::<Source>(summary.source.clone()).is_ok(),
            summary,
        })
        .collect();
    let mut activity = store.jobs()?;
    activity.truncate(12);
    Ok(ProjectInfo {
        root,
        name: s.project.config.name.clone(),
        warning: s.project.storage_warning(),
        network_enabled: s.project.config.network_enabled,
        offline_env: invectus_core::net::offline(),
        collections,
        tables,
        activity,
        running,
    })
}

#[tauri::command]
fn default_projects_dir() -> String {
    dirs::home_dir()
        .map(|h| h.join("inVectus Projects").display().to_string())
        .unwrap_or_default()
}

#[tauri::command]
async fn open_project(
    state: State<'_, AppState>,
    path: String,
    create: bool,
) -> CmdResult<ProjectInfo> {
    let session = state.session.clone();
    let board = state.board.clone();
    off_thread(move || open_project_blocking(&session, &board, path, create)).await
}

/// A typed path with a leading `~` expanded to the home folder.
fn user_path(path: &str) -> PathBuf {
    let path = path.trim();
    match (path.strip_prefix("~/"), dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if path == "~" => dirs::home_dir().unwrap_or_default(),
        _ => PathBuf::from(path),
    }
}

fn open_project_blocking(
    session: &Shared,
    board: &SharedBoard,
    path: String,
    create: bool,
) -> CmdResult<ProjectInfo> {
    let path = user_path(&path);
    let mut guard = session
        .try_lock()
        .map_err(|_| "busy: the open project is answering a request; try again".to_string())?;
    // A job keeps working in its own project; switching would hide it, so refuse instead.
    if let Some(j) = board
        .lock()
        .unwrap()
        .running()
        .filter(|j| j.project.is_some())
    {
        return Err(format!(
            "{} is running in {}; wait for it or cancel it before switching projects",
            j.label,
            j.project.as_deref().unwrap_or_default()
        ));
    }
    let project = if create {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(err)?;
        }
        Project::create(&path)
    } else {
        Project::open(&path)
    }
    .map_err(err)?;
    let vectors = VectorStore::open(&project).map_err(err)?;
    *guard = Some(Session {
        project,
        vectors,
        encoder: None,
        llm: None,
    });
    info(guard.as_ref().unwrap(), board).map_err(err)
}

#[tauri::command]
async fn project_info(state: State<'_, AppState>) -> CmdResult<ProjectInfo> {
    let session = state.session.clone();
    let board = state.board.clone();
    off_thread(move || with_session(&session, |s| info(s, &board))).await
}

#[derive(Deserialize)]
struct ImportForm {
    collection: String,
    source_path: String,
    text: Vec<String>,
    title: Option<String>,
    id: Option<String>,
    date: Option<String>,
    url: Option<String>,
    role: Option<Role>,
    role_column: Option<String>,
    role_rules: Option<String>,
    chunk_chars: usize,
    chunk_overlap: usize,
    limit: Option<usize>,
}

fn request(f: ImportForm) -> anyhow::Result<ImportRequest> {
    let source = Source::detect(&user_path(&f.source_path))?;
    let blank = |o: Option<String>| o.filter(|s| !s.trim().is_empty());
    let role = match blank(f.role_column) {
        Some(column) => RoleMapping::Column {
            column,
            rules: RoleMapping::parse_rules(f.role_rules.as_deref().unwrap_or(""))?,
            default: f.role.unwrap_or(Role::Neutral),
        },
        None => RoleMapping::Constant {
            role: f.role.unwrap_or(Role::Neutral),
        },
    };
    let mapping = match source {
        Source::Folder { .. } => Mapping::folder(role),
        _ => Mapping {
            id: blank(f.id),
            title: blank(f.title),
            text: f
                .text
                .into_iter()
                .filter(|t| !t.trim().is_empty())
                .collect(),
            date: blank(f.date),
            url: blank(f.url),
            role,
            meta: MetaMapping::All,
        },
    };
    Ok(ImportRequest {
        collection: f.collection,
        source,
        mapping,
        chunking: ChunkPolicy {
            max_chars: f.chunk_chars,
            overlap_chars: f.chunk_overlap,
            tokens: None,
        },
        limit: f.limit,
    })
}

#[tauri::command]
async fn preview_import(form: ImportForm, rows: Option<usize>) -> CmdResult<Preview> {
    off_thread(move || {
        let req = request(form).map_err(err)?;
        invectus_ingest::preview(&req, rows.unwrap_or(20)).map_err(err)
    })
    .await
}

/// Refuse project writes while a job works in the same project.
fn ensure_idle(board: &SharedBoard, project: &Project) -> anyhow::Result<()> {
    let root = project.root.display().to_string();
    if let Some(j) = board.lock().unwrap().running_in(&root) {
        anyhow::bail!("{} is running; wait for it or cancel it first", j.label);
    }
    Ok(())
}

#[derive(Serialize)]
struct ImportOutcome {
    report: ImportReport,
    /// Which models indexing uses, or what is missing.
    plan: IndexPlan,
    /// The indexing job started after the import, if the plan had models.
    job: Option<JobInfo>,
    /// Why indexing could not start (the import itself succeeded).
    index_error: Option<String>,
}

/// Import a collection, then start indexing it with the plan's installed models.
#[tauri::command]
async fn import_collection(
    app: AppHandle,
    state: State<'_, AppState>,
    form: ImportForm,
) -> CmdResult<ImportOutcome> {
    let req = request(form).map_err(err)?;
    import_then_index(app, state.session.clone(), state.board.clone(), req).await
}

/// Import a collection again from its stored source, mapping and chunking, then index it.
/// Source files are not watched: this is how changes in them reach the collection.
#[tauri::command]
async fn reimport_collection(
    app: AppHandle,
    state: State<'_, AppState>,
    collection: String,
) -> CmdResult<ImportOutcome> {
    let session = state.session.clone();
    let req = off_thread(move || {
        with_session(&session, |s| {
            let c = s.project.store.require_collection(&collection)?;
            let source: Source = serde_json::from_value(c.source).map_err(|_| {
                anyhow::anyhow!(
                    "{collection:?} was not imported from a file or folder; import it again with \
                     the CLI"
                )
            })?;
            Ok(ImportRequest {
                collection,
                source,
                mapping: serde_json::from_value(c.mapping)?,
                chunking: serde_json::from_value(c.chunking)?,
                limit: None,
            })
        })
    })
    .await?;
    import_then_index(app, state.session.clone(), state.board.clone(), req).await
}

async fn import_then_index(
    app: AppHandle,
    session: Shared,
    board: SharedBoard,
    req: ImportRequest,
) -> CmdResult<ImportOutcome> {
    let collection = req.collection.clone();
    let (s2, b2) = (session.clone(), board.clone());
    let (report, plan) = off_thread(move || {
        with_session(&s2, |s| {
            ensure_idle(&b2, &s.project)?;
            let report = invectus_ingest::import(&mut s.project, &req)?;
            let plan = plan_for(s, &req.collection)?;
            Ok((report, plan))
        })
    })
    .await?;
    let (job, index_error) = if plan.models.is_empty() {
        (None, None)
    } else {
        match start_index_job(
            &emitter(&app),
            &session,
            &board,
            collection,
            plan.models.clone(),
            true,
        ) {
            Ok(j) => (Some(j), None),
            Err(e) => (None, Some(e)),
        }
    };
    Ok(ImportOutcome {
        report,
        plan,
        job,
        index_error,
    })
}

fn plan_for(s: &Session, collection: &str) -> anyhow::Result<IndexPlan> {
    let tables = s.project.store.vector_tables(Some(collection))?;
    let existing: Vec<ExistingSpace> = tables
        .iter()
        .map(|t| ExistingSpace {
            model_key: &t.identity.model_key,
            origin: &t.origin,
        })
        .collect();
    let store = ModelStore::new(ModelStore::default_root()?);
    let catalogue: Vec<(String, String, u64)> = registry::MODELS
        .iter()
        .map(|m| (m.key.into(), m.repo.into(), m.total_bytes() / 1_000_000))
        .collect();
    let installed = |k: &str| registry::spec(k).is_ok_and(|m| store.is_installed(m));
    Ok(jobs::plan_index(
        &existing,
        &installed,
        registry::DEFAULT_MODEL,
        &catalogue,
    ))
}

/// Which installed models indexing a collection would use, and which are missing.
#[tauri::command]
async fn index_plan(state: State<'_, AppState>, collection: String) -> CmdResult<IndexPlan> {
    let session = state.session.clone();
    off_thread(move || with_session(&session, |s| plan_for(s, &collection))).await
}

#[derive(Serialize)]
struct SourceInfo {
    /// `csv`, `parquet` or `folder`.
    kind: String,
    columns: Vec<String>,
    /// The first rows, for guessing and showing the column mapping.
    sample: Vec<Vec<Option<String>>>,
    /// Rows read for this look (at most `SOURCE_LOOK_ROWS`).
    rows_read: usize,
    unreadable: usize,
}

const SOURCE_LOOK_ROWS: usize = 1000;

/// Columns and sample rows of an import source, before any mapping is chosen.
#[tauri::command]
async fn inspect_source(path: String) -> CmdResult<SourceInfo> {
    off_thread(move || {
        let source = Source::detect(&user_path(&path)).map_err(err)?;
        let kind = match source {
            Source::Csv { .. } => "csv",
            Source::Parquet { .. } => "parquet",
            Source::Folder { .. } => "folder",
        };
        let table = invectus_ingest::source::read(&source, Some(SOURCE_LOOK_ROWS)).map_err(err)?;
        Ok(SourceInfo {
            kind: kind.into(),
            sample: table.rows.iter().take(5).cloned().collect(),
            rows_read: table.rows.len(),
            unreadable: table.unreadable.len(),
            columns: table.columns,
        })
    })
    .await
}

/// Turn the project's network switch on or off (verification connectors use it).
#[tauri::command]
async fn set_network(state: State<'_, AppState>, enabled: bool) -> CmdResult<ProjectInfo> {
    let session = state.session.clone();
    let board = state.board.clone();
    off_thread(move || {
        with_session(&session, |s| {
            s.project.set_network_enabled(enabled)?;
            info(s, &board)
        })
    })
    .await
}

#[derive(Serialize)]
struct LlmInfo {
    key: String,
    repo: String,
    megabytes: u64,
    installed: bool,
    default: bool,
    note: String,
}

#[tauri::command]
fn list_llms() -> CmdResult<Vec<LlmInfo>> {
    let root = ModelStore::default_root().map_err(err)?;
    Ok(invectus_llm::registry::MODELS
        .iter()
        .map(|m| LlmInfo {
            key: m.key.into(),
            repo: m.repo.into(),
            megabytes: m.bytes / 1_000_000,
            installed: invectus_llm::store::is_installed(&root, m),
            default: m.key == invectus_llm::DEFAULT_LLM,
            note: m.note.into(),
        })
        .collect())
}

/// Download an embedding model at its pinned revision: explicit network use, refused while the
/// open project is offline; checksums are verified. Runs as a job (`job-event`).
#[tauri::command]
fn install_model(app: AppHandle, state: State<AppState>, model: String) -> CmdResult<JobInfo> {
    let spec = registry::spec(&model).map_err(err)?;
    invectus_core::net::ensure_online(&format!("downloading model {}", spec.key)).map_err(err)?;
    if let Ok(guard) = state.session.try_lock()
        && guard
            .as_ref()
            .is_some_and(|s| !s.project.config.network_enabled)
    {
        return Err("offline mode is on for this project: turn it off to download a model".into());
    }
    let mut info = JobInfo::new("download", None, format!("Downloading {}", spec.key));
    info.models = vec![spec.key.into()];
    spawn_job(
        &emitter(&app),
        &state.board,
        info,
        Box::new(move |_cancel, tick| {
            let store = ModelStore::new(ModelStore::default_root()?);
            store.download(spec, &mut |line| tick(line, 0, 0))?;
            Ok((serde_json::json!({"model": spec.key}), false))
        }),
    )
}

#[derive(Serialize)]
struct ModelInfo {
    key: String,
    repo: String,
    dimensions: usize,
    megabytes: u64,
    installed: bool,
    default: bool,
    note: String,
}

#[tauri::command]
fn list_models() -> CmdResult<Vec<ModelInfo>> {
    let store = ModelStore::new(ModelStore::default_root().map_err(err)?);
    Ok(registry::MODELS
        .iter()
        .map(|m| ModelInfo {
            key: m.key.into(),
            repo: m.repo.into(),
            dimensions: m.dimensions,
            megabytes: m.total_bytes() / 1_000_000,
            installed: store.is_installed(m),
            default: m.key == registry::DEFAULT_MODEL,
            note: m.note.into(),
        })
        .collect())
}

type Work = Box<
    dyn FnOnce(
            &CancelToken,
            &mut dyn FnMut(&str, usize, usize),
        ) -> anyhow::Result<(serde_json::Value, bool)>
        + Send,
>;

/// Where job changes go: the window's `job-event` in the app, a recorder in tests.
type Emit = Arc<dyn Fn(&JobInfo) + Send + Sync>;

fn emitter(app: &AppHandle) -> Emit {
    let app = app.clone();
    Arc::new(move |j: &JobInfo| {
        let _ = app.emit("job-event", j);
    })
}

/// Register a job on the board and run it on its own thread. Every change is emitted as a
/// `job-event` carrying the whole job, including the project it belongs to.
fn spawn_job(emit: &Emit, board: &SharedBoard, info: JobInfo, work: Work) -> CmdResult<JobInfo> {
    let (info, cancel) = board.lock().unwrap().begin(info)?;
    emit(&info);
    let (emit, board, id) = (emit.clone(), board.clone(), info.id);
    std::thread::spawn(move || {
        let mut last: Option<Instant> = None;
        let mut tick = |phase: &str, done: usize, total: usize| {
            // At most ten updates a second, but always the last one of a phase.
            if done < total && last.is_some_and(|t| t.elapsed() < Duration::from_millis(100)) {
                return;
            }
            last = Some(Instant::now());
            if let Some(j) = board.lock().unwrap().progress(id, phase, done, total) {
                emit(&j);
            }
        };
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&cancel, &mut tick)))
                .map_err(|_| "the job stopped unexpectedly (internal error)".to_string())
                .and_then(|r| r.map_err(|e| format!("{e:#}")));
        if let Some(j) = board.lock().unwrap().finish(id, result) {
            emit(&j);
        }
    });
    Ok(info)
}

/// A second handle on the open project for a job, taken without waiting.
fn job_project(session: &Shared) -> CmdResult<Project> {
    let guard = session
        .try_lock()
        .map_err(|_| "busy: the open project is answering a request; try again".to_string())?;
    let s = guard.as_ref().ok_or("no project is open")?;
    s.project.try_clone().map_err(err)
}

/// Index a collection with `models`; with `map`, then lay it out (regions, for the map) and
/// fit a title-based calibration on the first model's space, so a fresh import ends ready to
/// explore and search. Mapping is skipped when nothing changed and regions already exist; a
/// calibration failure (e.g. too few titled items) is reported, not fatal.
fn start_index_job(
    emit: &Emit,
    session: &Shared,
    board: &SharedBoard,
    collection: String,
    models: Vec<String>,
    map: bool,
) -> CmdResult<JobInfo> {
    let project = job_project(session)?;
    project.store.require_collection(&collection).map_err(err)?;
    let mut info = JobInfo::new(
        "index",
        Some(project.root.display().to_string()),
        format!("Indexing {collection}"),
    );
    info.collection = Some(collection.clone());
    info.models = models.clone();
    spawn_job(
        emit,
        board,
        info,
        Box::new(move |cancel, tick| {
            let mut project = project;
            let vectors = VectorStore::open(&project)?;
            let store = ModelStore::new(ModelStore::default_root()?);
            let mut reports = Vec::new();
            for model in &models {
                let mut embedder = OnnxEmbedder::load(&store, model, &EmbedOptions::default())?;
                let phase = format!("Embedding with {model}");
                let report = run_embed(
                    &project,
                    &vectors,
                    &collection,
                    &mut embedder,
                    &EmbedJobOptions::default(),
                    cancel,
                    &mut |p| tick(&phase, p.done, p.total),
                )?;
                let cancelled = report.cancelled;
                reports.push(report);
                if cancelled {
                    return Ok((serde_json::json!({ "embed": reports }), true));
                }
            }
            let mut out = serde_json::json!({ "embed": reports });
            let Some(first) = reports.first().filter(|_| map) else {
                return Ok((out, false));
            };
            let table = select_table(&project, &collection, Some(&first.space_id))?;
            let changed = reports
                .iter()
                .any(|r| r.embedded > 0 || r.removed_stale > 0);
            let has_regions = project
                .store
                .latest_region_run(&table.collection_id, &table.space_id)?
                .is_some();
            if changed || !has_regions {
                // Small or uniform collections may not form regions; the index still stands.
                match run_regions(
                    &mut project,
                    &vectors,
                    &table,
                    &RegionOptions::default(),
                    cancel,
                    &mut |p| tick(&format!("Mapping: {}", p.phase), p.done, p.total),
                ) {
                    Ok(r) if r.cancelled => return Ok((out, true)),
                    Ok(r) => {
                        out["regions"] =
                            serde_json::json!({ "fine": r.fine, "seconds": r.seconds_total });
                        // Score gaps on the new regions (seconds, offline) so the map shows them.
                        out["gaps"] = match invectus_gaps::scan(
                            &mut project,
                            &vectors,
                            &table,
                            &ScanOptions::default(),
                        ) {
                            Ok(g) => serde_json::json!({ "found": g.gaps.len() }),
                            Err(e) => serde_json::json!({ "skipped": format!("{e:#}") }),
                        };
                    }
                    Err(e) => out["regions"] = serde_json::json!({ "skipped": format!("{e:#}") }),
                }
            }
            let calibrated = project
                .store
                .latest_calibration(&table.collection_id, &table.space_id)?
                .is_some();
            if changed || !calibrated {
                let cal = query_encoder(
                    &store,
                    &table.identity,
                    &EmbedOptions {
                        threads: 4,
                        batch_size: 1,
                    },
                )
                .and_then(|mut enc| {
                    run_calibration(
                        &project,
                        &vectors,
                        &table,
                        &mut enc,
                        None,
                        &CalibrateOptions::default(),
                        cancel,
                        &mut |p| tick("Calibrating", p.done, p.total),
                    )
                });
                out["calibration"] = match cal {
                    Ok(c) if c.cancelled => return Ok((out, true)),
                    Ok(c) => serde_json::json!({ "method": c.method }),
                    Err(e) => serde_json::json!({ "skipped": format!("{e:#}") }),
                };
            }
            Ok((out, false))
        }),
    )
}

/// Index (embed) a collection. With no `models`, the collection's plan decides: its own spaces'
/// models, else the default. Resumable: items already embedded are skipped.
#[tauri::command]
fn start_index(
    app: AppHandle,
    state: State<AppState>,
    collection: String,
    models: Option<Vec<String>>,
    // Also map and calibrate afterwards (the import pipeline; used by Retry).
    map: Option<bool>,
) -> CmdResult<JobInfo> {
    let models = match models {
        Some(m) if !m.is_empty() => m,
        _ => {
            let guard = state.session.try_lock().map_err(|_| "busy; try again")?;
            let s = guard.as_ref().ok_or("no project is open")?;
            let plan = plan_for(s, &collection).map_err(err)?;
            if plan.models.is_empty() {
                return Err(match plan.missing.first() {
                    Some(m) => format!("no embedding model is installed; {} is needed", m.key),
                    None => "no embedding model to use".into(),
                });
            }
            plan.models
        }
    };
    for m in &models {
        let spec = registry::spec(m).map_err(err)?;
        if !ModelStore::new(ModelStore::default_root().map_err(err)?).is_installed(spec) {
            return Err(format!("{m} is not installed"));
        }
    }
    start_index_job(
        &emitter(&app),
        &state.session,
        &state.board,
        collection,
        models,
        map.unwrap_or(false),
    )
}

#[tauri::command]
fn cancel_job(state: State<AppState>) {
    state.board.lock().unwrap().cancel();
}

/// The running job (if any) and this session's finished ones.
#[tauri::command]
fn jobs(state: State<AppState>) -> JobsView {
    state.board.lock().unwrap().view()
}

#[derive(Deserialize)]
struct SearchForm {
    collection: String,
    query: String,
    k: usize,
    role: Option<Role>,
    exhaustive: bool,
    space: Option<String>,
}

#[tauri::command]
async fn search(state: State<'_, AppState>, form: SearchForm) -> CmdResult<SearchResult> {
    let session = state.session.clone();
    off_thread(move || search_blocking(&session, form)).await
}

/// The session's query encoder for `table`'s space, loaded on first use.
fn encoder_for<'a>(
    s: &'a mut Session,
    table: &VectorTableRecord,
) -> anyhow::Result<&'a mut OnnxEmbedder> {
    if s.encoder
        .as_ref()
        .is_none_or(|(id, _)| *id != table.space_id)
    {
        let store = ModelStore::new(ModelStore::default_root()?);
        let enc = query_encoder(
            &store,
            &table.identity,
            &EmbedOptions {
                threads: 4,
                batch_size: 1,
            },
        )?;
        s.encoder = Some((table.space_id.clone(), enc));
    }
    Ok(&mut s.encoder.as_mut().unwrap().1)
}

fn search_blocking(session: &Shared, form: SearchForm) -> CmdResult<SearchResult> {
    with_session(session, |s| {
        let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
        encoder_for(s, &table)?;
        let encoder = &mut s.encoder.as_mut().unwrap().1;
        engine_search(
            &s.project,
            &s.vectors,
            &table,
            encoder,
            &form.query,
            &SearchOptions {
                k: form.k,
                role: form.role,
                mode: if form.exhaustive {
                    SearchMode::Exhaustive
                } else {
                    SearchMode::Auto
                },
            },
        )
    })
}

/// Calibrate a collection's vector space as a job; cancelling saves nothing.
#[tauri::command]
fn start_calibration(
    app: AppHandle,
    state: State<AppState>,
    collection: String,
    space: Option<String>,
    // Paraphrase queries with the local language model (SPEC §6.2) instead of titles.
    llm: Option<bool>,
) -> CmdResult<JobInfo> {
    let project = job_project(&state.session)?;
    let table = select_table(&project, &collection, space.as_deref()).map_err(err)?;
    let use_llm = llm.unwrap_or(false);
    // Borrow the session's loaded language model rather than loading a second copy.
    let borrowed = if use_llm {
        state
            .session
            .try_lock()
            .ok()
            .and_then(|mut g| g.as_mut().and_then(|s| s.llm.take()))
    } else {
        None
    };
    let mut info = JobInfo::new(
        "calibrate",
        Some(project.root.display().to_string()),
        format!("Calibrating {collection}"),
    );
    info.collection = Some(collection);
    info.space = Some(table.space_id.clone());
    info.models = vec![table.identity.model_key.clone()];
    let session = state.session.clone();
    spawn_job(
        &emitter(&app),
        &state.board,
        info,
        Box::new(move |cancel, tick| {
            let vectors = VectorStore::open(&project)?;
            let store = ModelStore::new(ModelStore::default_root()?);
            let mut encoder = query_encoder(
                &store,
                &table.identity,
                &EmbedOptions {
                    threads: 4,
                    batch_size: 1,
                },
            )?;
            let mut generator = match (use_llm, borrowed) {
                (_, Some(g)) => Some(g),
                (true, None) => {
                    let spec = invectus_llm::registry::spec(invectus_llm::DEFAULT_LLM)?;
                    Some(invectus_llm::LlamaGenerator::load(&store.root, spec, 8)?)
                }
                (false, None) => None,
            };
            let result = run_calibration(
                &project,
                &vectors,
                &table,
                &mut encoder,
                generator
                    .as_mut()
                    .map(|g| g as &mut dyn invectus_llm::TextGenerator),
                &CalibrateOptions::default(),
                cancel,
                &mut |p| tick("Calibrating", p.done, p.total),
            );
            // Hand the model back to the session if it still has the same project open.
            if let (Some(g), Ok(mut guard)) = (generator, session.lock())
                && let Some(s) = guard.as_mut()
                && s.project.root == project.root
                && s.llm.is_none()
            {
                s.llm = Some(g);
            }
            let report = result?;
            let cancelled = report.cancelled;
            Ok((serde_json::to_value(&report)?, cancelled))
        }),
    )
}

#[derive(Deserialize)]
struct CheckForm {
    collection: String,
    query: String,
    k: usize,
    role: Option<Role>,
    /// Use the ANN index when there is one; exhaustive otherwise.
    fast: bool,
    space: Option<String>,
}

/// inVectus search verdict: exists / near / open with P(exists) and the evidence.
#[tauri::command]
async fn check_idea(state: State<'_, AppState>, form: CheckForm) -> CmdResult<CheckResult> {
    let session = state.session.clone();
    off_thread(move || {
        with_session(&session, |s| {
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            invectus_unsearch::verdict::calibration_for(&s.project, &table)?;
            encoder_for(s, &table)?;
            let encoder = &mut s.encoder.as_mut().unwrap().1;
            check(
                &s.project,
                &s.vectors,
                &table,
                encoder,
                &form.query,
                &CheckOptions {
                    k: form.k,
                    role: form.role,
                    mode: if form.fast {
                        SearchMode::Auto
                    } else {
                        SearchMode::Exhaustive
                    },
                },
            )
        })
    })
    .await
}

/// Find regions and lay out the map as a job; cancelling keeps the previous regions.
#[tauri::command]
fn start_regions(
    app: AppHandle,
    state: State<AppState>,
    collection: String,
    space: Option<String>,
) -> CmdResult<JobInfo> {
    let mut project = job_project(&state.session)?;
    let table = select_table(&project, &collection, space.as_deref()).map_err(err)?;
    let mut info = JobInfo::new(
        "regions",
        Some(project.root.display().to_string()),
        format!("Finding regions in {collection}"),
    );
    info.collection = Some(collection);
    info.space = Some(table.space_id.clone());
    info.models = vec![table.identity.model_key.clone()];
    spawn_job(
        &emitter(&app),
        &state.board,
        info,
        Box::new(move |cancel, tick| {
            let vectors = VectorStore::open(&project)?;
            let report = run_regions(
                &mut project,
                &vectors,
                &table,
                &RegionOptions::default(),
                cancel,
                &mut |p| tick(&p.phase, p.done, p.total),
            )?;
            let cancelled = report.cancelled;
            Ok((serde_json::to_value(&report)?, cancelled))
        }),
    )
}

/// Regions and one point per item for the map view.
#[tauri::command]
async fn region_map(
    state: State<'_, AppState>,
    collection: String,
    space: Option<String>,
) -> CmdResult<MapData> {
    let session = state.session.clone();
    off_thread(move || {
        with_session(&session, |s| {
            let table = select_table(&s.project, &collection, space.as_deref())?;
            map_data(&s.project, &table)
        })
    })
    .await
}

#[derive(Deserialize)]
struct TableForm {
    collection: String,
    space: Option<String>,
}

/// Score gaps over the latest regions (verdicts are kept).
#[tauri::command]
async fn scan_gaps(
    state: State<'_, AppState>,
    form: TableForm,
    weights: Option<invectus_gaps::scan::Weights>,
) -> CmdResult<ScanReport> {
    let session = state.session.clone();
    let board = state.board.clone();
    off_thread(move || {
        with_session(&session, |s| {
            ensure_idle(&board, &s.project)?;
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            let mut opts = ScanOptions::default();
            if let Some(w) = weights {
                opts.weights = w;
            }
            invectus_gaps::scan(&mut s.project, &s.vectors, &table, &opts)
        })
    })
    .await
}

/// Ranked gaps of the latest scan with their verdicts.
#[tauri::command]
async fn list_gaps(state: State<'_, AppState>, form: TableForm) -> CmdResult<GapList> {
    let session = state.session.clone();
    off_thread(move || {
        with_session(&session, |s| {
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            invectus_gaps::list(&s.project, &table)
        })
    })
    .await
}

/// One gap with its score terms, timeline, central items and verdict.
#[tauri::command]
async fn gap_detail(
    state: State<'_, AppState>,
    form: TableForm,
    gap_id: i32,
) -> CmdResult<GapDetail> {
    let session = state.session.clone();
    off_thread(move || {
        with_session(&session, |s| {
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            invectus_gaps::detail(&s.project, &table, gap_id)
        })
    })
    .await
}

/// Record a verdict (open, confirmed_gap, already_exists, not_viable, selected) and a note.
#[tauri::command]
async fn set_gap_verdict(
    state: State<'_, AppState>,
    form: TableForm,
    gap_id: i32,
    verdict: String,
    note: String,
) -> CmdResult<GapView> {
    let session = state.session.clone();
    off_thread(move || {
        with_session(&session, |s| {
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            invectus_gaps::set_verdict(&s.project, &table, gap_id, &verdict, &note)
        })
    })
    .await
}

/// Describe what would fill a gap with the local language model (round-trip checked; stored).
/// Loads the model for each call (~1 s once its checksum is cached).
#[tauri::command]
async fn describe_gap(
    state: State<'_, AppState>,
    form: TableForm,
    gap_id: i32,
    t: Option<f32>,
    genre: Option<String>,
    coverage_genre: Option<String>,
) -> CmdResult<invectus_midpoint::DescribeResult> {
    let session = state.session.clone();
    off_thread(move || {
        with_session(&session, |s| {
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            let g = invectus_gaps::detail(&s.project, &table, gap_id)?.view.gap;
            let target = match g.region_b {
                Some(b) => invectus_midpoint::Target::Between {
                    a: g.region_a,
                    b,
                    t: t.unwrap_or(0.5),
                },
                None => invectus_midpoint::Target::Region { region: g.region_a },
            };
            if s.llm.is_none() {
                let store = ModelStore::new(ModelStore::default_root()?);
                let spec = invectus_llm::registry::spec(invectus_llm::DEFAULT_LLM)?;
                s.llm = Some(invectus_llm::LlamaGenerator::load(&store.root, spec, 8)?);
            }
            encoder_for(s, &table)?;
            let encoder = &mut s.encoder.as_mut().unwrap().1;
            let generator = s.llm.as_mut().unwrap();
            let defaults = invectus_midpoint::DescribeOptions::default();
            invectus_midpoint::describe(
                &s.project,
                &s.vectors,
                &table,
                encoder,
                generator,
                &target,
                &invectus_midpoint::DescribeOptions {
                    genre: genre.unwrap_or(defaults.genre),
                    coverage_genre: coverage_genre.unwrap_or(defaults.coverage_genre),
                    ..defaults
                },
            )
        })
    })
    .await
}

/// Check a gap against PubMed and OpenAlex (network; cached in the project). `network` allows
/// this call to use the network when the project's switch is off.
#[tauri::command]
async fn verify_gap(
    state: State<'_, AppState>,
    form: TableForm,
    gap_id: i32,
    network: bool,
) -> CmdResult<invectus_verify::Verification> {
    let session = state.session.clone();
    off_thread(move || {
        with_session(&session, |s| {
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            encoder_for(s, &table)?;
            let encoder = &mut s.encoder.as_mut().unwrap().1;
            invectus_verify::verify_gap(
                &s.project,
                &table,
                encoder,
                gap_id,
                &invectus_verify::VerifyOptions {
                    allow_network: network,
                    ..Default::default()
                },
            )
        })
    })
    .await
}

/// The scan's default weights, for the settings screen's reset.
#[tauri::command]
fn default_gap_weights() -> invectus_gaps::scan::Weights {
    ScanOptions::default().weights
}

/// Write the latest gaps as CSV or Markdown into the project's `exports/` folder.
#[tauri::command]
async fn export_gaps(
    state: State<'_, AppState>,
    form: TableForm,
    format: String,
) -> CmdResult<String> {
    let session = state.session.clone();
    off_thread(move || {
        with_session(&session, |s| {
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            let list = invectus_gaps::list(&s.project, &table)?;
            let (text, ext) = match format.as_str() {
                "csv" => (invectus_gaps::export::csv(&list), "csv"),
                "markdown" => (
                    invectus_gaps::export::markdown(&s.project, &table.collection_id, &list, 50)?,
                    "md",
                ),
                other => anyhow::bail!("unknown export format {other:?} (csv, markdown)"),
            };
            let dir = s.project.root.join("exports");
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(format!(
                "gaps-{}-{}.{ext}",
                table.collection_id,
                &list.run_id[..list.run_id.len().min(8)]
            ));
            std::fs::write(&path, text)?;
            Ok(path.display().to_string())
        })
    })
    .await
}

/// Open an http(s) link in the system browser (the webview does not follow external links).
#[tauri::command]
fn open_url(url: String) -> CmdResult<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("not a web link: {url}"));
    }
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(&url).spawn().map(|_| ()).map_err(err)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            engine_version,
            default_projects_dir,
            open_project,
            project_info,
            inspect_source,
            set_network,
            list_llms,
            install_model,
            default_gap_weights,
            export_gaps,
            open_url,
            preview_import,
            import_collection,
            reimport_collection,
            index_plan,
            list_models,
            start_index,
            cancel_job,
            jobs,
            search,
            start_calibration,
            check_idea,
            start_regions,
            region_map,
            scan_gaps,
            list_gaps,
            gap_detail,
            set_gap_verdict,
            describe_gap,
            verify_gap
        ])
        .run(tauri::generate_context!())
        .expect("failed to run inVectus");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_job(board: &SharedBoard) -> JobInfo {
        let start = Instant::now();
        loop {
            let v = board.lock().unwrap().view();
            if let Some(c) = v.current.filter(|c| !c.running()) {
                return c;
            }
            assert!(
                start.elapsed() < Duration::from_secs(300),
                "job did not finish"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn session_for(path: &std::path::Path) -> Shared {
        let project = Project::create(path).unwrap();
        let vectors = VectorStore::open(&project).unwrap();
        Arc::new(Mutex::new(Some(Session {
            project,
            vectors,
            encoder: None,
            llm: None,
        })))
    }

    /// Import → automatic model choice → indexing on its own connection → readiness, then a
    /// re-import (outdated until re-indexed), a failed job and its retry. Uses the dev model
    /// store (INVECTUS_MODELS_DIR); skipped when the default model is not installed.
    #[test]
    fn import_then_index_end_to_end() {
        let installed = ModelStore::default_root().ok().is_some_and(|r| {
            ModelStore::new(r).is_installed(registry::spec(registry::DEFAULT_MODEL).unwrap())
        });
        if !installed {
            eprintln!("skipped: {} is not installed", registry::DEFAULT_MODEL);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("notes.csv");
        let mut rows = String::from("id,title,text\n");
        let topics = [
            "retinal detachment surgery outcomes",
            "glaucoma intraocular pressure medication",
            "myopia progression in children",
            "corneal transplant rejection",
        ];
        for i in 0..240 {
            let t = topics[i % 4];
            rows.push_str(&format!(
                "{i},Study {i} on {t},Patients in cohort {i} were followed for {t}; case {i}.\n"
            ));
        }
        std::fs::write(&csv, rows).unwrap();
        let session = session_for(&dir.path().join("p"));
        let board = SharedBoard::default();
        let events: Arc<Mutex<Vec<JobInfo>>> = Arc::default();
        let ev = events.clone();
        let emit: Emit = Arc::new(move |j: &JobInfo| ev.lock().unwrap().push(j.clone()));
        let form = || -> ImportForm {
            serde_json::from_value(serde_json::json!({
                "collection": "notes", "source_path": csv, "text": ["text"], "title": "title",
                "id": "id", "date": null, "url": null, "role": "neutral", "role_column": null,
                "role_rules": null, "chunk_chars": 2000, "chunk_overlap": 200, "limit": null
            }))
            .unwrap()
        };
        let import = || {
            let mut g = session.lock().unwrap();
            let s = g.as_mut().unwrap();
            ensure_idle(&board, &s.project).unwrap();
            invectus_ingest::import(&mut s.project, &request(form()).unwrap()).unwrap();
            plan_for(s, "notes").unwrap()
        };
        let status = || {
            let g = session.lock().unwrap();
            let i = info(g.as_ref().unwrap(), &board).unwrap();
            i.tables
                .iter()
                .map(|t| (t.index_status.clone(), t.vectors))
                .collect::<Vec<_>>()
        };

        let plan = import();
        assert_eq!(plan.models, vec![registry::DEFAULT_MODEL.to_string()]);
        assert!(status().is_empty(), "nothing indexed yet");
        start_index_job(
            &emit,
            &session,
            &board,
            "notes".into(),
            plan.models.clone(),
            true,
        )
        .unwrap();
        // The session keeps answering while the job runs on its own connection.
        assert!(session.try_lock().is_ok());
        let done = wait_for_job(&board);
        assert_eq!(done.state, "completed", "{:?}", done.error);
        assert_eq!(status(), vec![("indexed".to_string(), 240)]);
        let report = done.report.clone().unwrap();
        assert!(
            report["regions"]["fine"].as_u64().unwrap_or(0) > 0,
            "{report}"
        );
        assert!(report["calibration"].is_object(), "{report}");
        {
            let g = session.lock().unwrap();
            let i = info(g.as_ref().unwrap(), &board).unwrap();
            assert!(
                i.tables[0].regions.is_some(),
                "regions are recorded for the map"
            );
        }
        let root = session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .project
            .root
            .display()
            .to_string();
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .all(|j| j.project.as_deref() == Some(root.as_str()))
        );

        // Importing again changes the collection's import time: the index is outdated until
        // the next run, which embeds nothing new (resumable by input hash).
        let plan = import();
        assert_eq!(status()[0].0, "outdated");
        start_index_job(
            &emit,
            &session,
            &board,
            "notes".into(),
            plan.models.clone(),
            true,
        )
        .unwrap();
        let done = wait_for_job(&board);
        assert_eq!(done.report.as_ref().unwrap()["embed"][0]["embedded"], 0);
        assert_eq!(status()[0].0, "indexed");

        // A failing job is reported with its error, and a retry works.
        start_index_job(
            &emit,
            &session,
            &board,
            "notes".into(),
            vec!["no-such-model".into()],
            false,
        )
        .unwrap();
        let failed = wait_for_job(&board);
        assert_eq!(failed.state, "failed");
        assert!(failed.error.unwrap().contains("no-such-model"));
        start_index_job(&emit, &session, &board, "notes".into(), plan.models, false).unwrap();
        assert_eq!(wait_for_job(&board).state, "completed");
    }

    /// A running job blocks project writes and switching, in its project only.
    #[test]
    fn running_jobs_block_writes_and_switching() {
        let dir = tempfile::tempdir().unwrap();
        let session = session_for(&dir.path().join("a"));
        let other = dir.path().join("b");
        Project::create(&other).unwrap();
        let board = SharedBoard::default();
        let root = session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .project
            .root
            .display()
            .to_string();
        let (job, _) = board
            .lock()
            .unwrap()
            .begin(JobInfo::new(
                "regions",
                Some(root),
                "Finding regions".into(),
            ))
            .unwrap();
        {
            let g = session.lock().unwrap();
            assert!(ensure_idle(&board, &g.as_ref().unwrap().project).is_err());
        }
        let e = open_project_blocking(&session, &board, other.display().to_string(), false)
            .err()
            .unwrap();
        assert!(e.contains("before switching"), "{e}");
        // Still on the first project.
        assert!(
            session
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .project
                .root
                .ends_with("a.invectus")
        );
        board.lock().unwrap().finish(job.id, Err("x".into()));
        assert!(
            open_project_blocking(&session, &board, other.display().to_string(), false).is_ok()
        );
        // A model download (no project) never blocks switching.
        board
            .lock()
            .unwrap()
            .begin(JobInfo::new("download", None, "d".into()))
            .unwrap();
        let first = dir.path().join("a").display().to_string();
        assert!(open_project_blocking(&session, &board, first, false).is_ok());
    }
    #[test]
    fn shell_uses_core() {
        assert_eq!(engine_version(), "0.1.0");
    }

    #[test]
    fn import_form_maps_role_rules() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("x.csv");
        std::fs::write(&csv, "id,t\n1,a\n").unwrap();
        let form: ImportForm = serde_json::from_value(serde_json::json!({
            "collection": "c", "source_path": csv, "text": ["t", ""], "title": "",
            "id": "id", "date": null, "url": null, "role": "neutral",
            "role_column": "t", "role_rules": "a=coverage",
            "chunk_chars": 2000, "chunk_overlap": 200, "limit": null
        }))
        .unwrap();
        let req = request(form).unwrap();
        assert_eq!(req.mapping.text, vec!["t"]);
        assert_eq!(req.mapping.title, None);
        let pv = invectus_ingest::preview(&req, 5).unwrap();
        assert_eq!(pv.rows[0].role, invectus_core::Role::Coverage);
    }
}
