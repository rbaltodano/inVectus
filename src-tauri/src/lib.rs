//! Desktop shell: Tauri commands over the same engine crates as the `invectus` CLI.

use invectus_core::store::{CollectionSummary, VectorTableRecord};
use invectus_core::{CancelToken, Project, Role};
use invectus_embed::{EmbedOptions, ModelStore, OnnxEmbedder, query_encoder, registry};
use invectus_gaps::{GapDetail, GapList, GapView, ScanOptions, ScanReport};
use invectus_index::{
    EmbedJobOptions, EmbedReport, SearchMode, SearchOptions, SearchResult, VectorStore, run_embed,
    search as engine_search, select_table,
};
use invectus_ingest::{
    ChunkPolicy, ImportReport, ImportRequest, Mapping, MetaMapping, Preview, RoleMapping, Source,
};
use invectus_regions::{MapData, RegionOptions, RegionReport, map_data, run_regions};
use invectus_unsearch::{
    CalibrateOptions, CalibrationReport, CheckOptions, CheckResult, check, run_calibration,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};

struct Session {
    project: Project,
    vectors: VectorStore,
    /// Query encoder cached per vector space id.
    encoder: Option<(String, OnnxEmbedder)>,
}

#[derive(Default)]
struct AppState {
    session: Shared,
    cancel: Mutex<Option<CancelToken>>,
}

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
}

#[derive(Serialize)]
struct ProjectInfo {
    root: String,
    name: String,
    warning: Option<String>,
    collections: Vec<CollectionSummary>,
    tables: Vec<TableInfo>,
}

fn info(s: &Session) -> anyhow::Result<ProjectInfo> {
    let tables = s
        .project
        .store
        .vector_tables(None)?
        .into_iter()
        .map(|t| {
            Ok(TableInfo {
                vectors: s.vectors.count(&t)?,
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
    Ok(ProjectInfo {
        root: s.project.root.display().to_string(),
        name: s.project.config.name.clone(),
        warning: s.project.storage_warning(),
        collections: s.project.store.collection_summaries()?,
        tables,
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
    off_thread(move || open_project_blocking(&session, path, create)).await
}

fn open_project_blocking(session: &Shared, path: String, create: bool) -> CmdResult<ProjectInfo> {
    let path = PathBuf::from(path);
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
    let mut guard = session
        .try_lock()
        .map_err(|_| "busy: a job is running".to_string())?;
    *guard = Some(Session {
        project,
        vectors,
        encoder: None,
    });
    info(guard.as_ref().unwrap()).map_err(err)
}

#[tauri::command]
async fn project_info(state: State<'_, AppState>) -> CmdResult<ProjectInfo> {
    let session = state.session.clone();
    off_thread(move || with_session(&session, |s| info(s))).await
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
    let source = Source::detect(&PathBuf::from(&f.source_path))?;
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
async fn preview_import(form: ImportForm) -> CmdResult<Preview> {
    off_thread(move || {
        let req = request(form).map_err(err)?;
        invectus_ingest::preview(&req, 20).map_err(err)
    })
    .await
}

#[tauri::command]
async fn run_import(state: State<'_, AppState>, form: ImportForm) -> CmdResult<ImportReport> {
    let session = state.session.clone();
    off_thread(move || {
        let req = request(form).map_err(err)?;
        with_session(&session, |s| invectus_ingest::import(&mut s.project, &req))
    })
    .await
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

#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum EmbedEvent {
    Progress { done: usize, total: usize },
    Finished { report: EmbedReport },
    Failed { error: String },
}

/// Start an embed job on a worker thread; progress arrives as `embed-event` events.
#[tauri::command]
fn start_embed(
    app: tauri::AppHandle,
    state: State<AppState>,
    collection: String,
    model: String,
) -> CmdResult<()> {
    let session = state.session.clone();
    if session
        .try_lock()
        .map_err(|_| "busy: a job is running")?
        .is_none()
    {
        return Err("no project is open".into());
    }
    let cancel = CancelToken::default();
    *state.cancel.lock().unwrap() = Some(cancel.clone());
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<EmbedReport> {
            let mut guard = session.lock().unwrap();
            let s = guard
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("no project is open"))?;
            let store = ModelStore::new(ModelStore::default_root()?);
            let mut embedder = OnnxEmbedder::load(&store, &model, &EmbedOptions::default())?;
            let app2 = app.clone();
            run_embed(
                &s.project,
                &s.vectors,
                &collection,
                &mut embedder,
                &EmbedJobOptions::default(),
                &cancel,
                &mut |p| {
                    let _ = app2.emit(
                        "embed-event",
                        EmbedEvent::Progress {
                            done: p.done,
                            total: p.total,
                        },
                    );
                },
            )
        })();
        let event = match result {
            Ok(report) => EmbedEvent::Finished { report },
            Err(e) => EmbedEvent::Failed {
                error: format!("{e:#}"),
            },
        };
        let _ = app.emit("embed-event", event);
    });
    Ok(())
}

#[tauri::command]
fn cancel_job(state: State<AppState>) {
    if let Some(c) = state.cancel.lock().unwrap().as_ref() {
        c.cancel();
    }
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

#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum CalibrateEvent {
    Progress { done: usize, total: usize },
    Finished { report: Box<CalibrationReport> },
    Failed { error: String },
}

/// Calibrate a collection's vector space on a worker thread; progress arrives as
/// `calibrate-event` events and `cancel_job` stops it (nothing is saved when cancelled).
#[tauri::command]
fn start_calibration(
    app: tauri::AppHandle,
    state: State<AppState>,
    collection: String,
    space: Option<String>,
) -> CmdResult<()> {
    let session = state.session.clone();
    if session
        .try_lock()
        .map_err(|_| "busy: a job is running")?
        .is_none()
    {
        return Err("no project is open".into());
    }
    let cancel = CancelToken::default();
    *state.cancel.lock().unwrap() = Some(cancel.clone());
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<CalibrationReport> {
            let mut guard = session.lock().unwrap();
            let s = guard
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("no project is open"))?;
            let table = select_table(&s.project, &collection, space.as_deref())?;
            encoder_for(s, &table)?;
            let encoder = &mut s.encoder.as_mut().unwrap().1;
            let app2 = app.clone();
            run_calibration(
                &s.project,
                &s.vectors,
                &table,
                encoder,
                &CalibrateOptions::default(),
                &cancel,
                &mut |p| {
                    let _ = app2.emit(
                        "calibrate-event",
                        CalibrateEvent::Progress {
                            done: p.done,
                            total: p.total,
                        },
                    );
                },
            )
        })();
        let event = match result {
            Ok(report) => CalibrateEvent::Finished {
                report: Box::new(report),
            },
            Err(e) => CalibrateEvent::Failed {
                error: format!("{e:#}"),
            },
        };
        let _ = app.emit("calibrate-event", event);
    });
    Ok(())
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

#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum RegionsEvent {
    Progress {
        phase: String,
        done: usize,
        total: usize,
    },
    Finished {
        report: Box<RegionReport>,
    },
    Failed {
        error: String,
    },
}

/// Find regions and lay out the map on a worker thread; progress arrives as `regions-event`
/// events and `cancel_job` stops it (the previous regions are kept when cancelled).
#[tauri::command]
fn start_regions(
    app: tauri::AppHandle,
    state: State<AppState>,
    collection: String,
    space: Option<String>,
) -> CmdResult<()> {
    let session = state.session.clone();
    if session
        .try_lock()
        .map_err(|_| "busy: a job is running")?
        .is_none()
    {
        return Err("no project is open".into());
    }
    let cancel = CancelToken::default();
    *state.cancel.lock().unwrap() = Some(cancel.clone());
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<RegionReport> {
            let mut guard = session.lock().unwrap();
            let s = guard
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("no project is open"))?;
            let table = select_table(&s.project, &collection, space.as_deref())?;
            let app2 = app.clone();
            run_regions(
                &mut s.project,
                &s.vectors,
                &table,
                &RegionOptions::default(),
                &cancel,
                &mut |p| {
                    let _ = app2.emit(
                        "regions-event",
                        RegionsEvent::Progress {
                            phase: p.phase.clone(),
                            done: p.done,
                            total: p.total,
                        },
                    );
                },
            )
        })();
        let event = match result {
            Ok(report) => RegionsEvent::Finished {
                report: Box::new(report),
            },
            Err(e) => RegionsEvent::Failed {
                error: format!("{e:#}"),
            },
        };
        let _ = app.emit("regions-event", event);
    });
    Ok(())
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
async fn scan_gaps(state: State<'_, AppState>, form: TableForm) -> CmdResult<ScanReport> {
    let session = state.session.clone();
    off_thread(move || {
        with_session(&session, |s| {
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            invectus_gaps::scan(&mut s.project, &s.vectors, &table, &ScanOptions::default())
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

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            engine_version,
            default_projects_dir,
            open_project,
            project_info,
            preview_import,
            run_import,
            list_models,
            start_embed,
            cancel_job,
            search,
            start_calibration,
            check_idea,
            start_regions,
            region_map,
            scan_gaps,
            list_gaps,
            gap_detail,
            set_gap_verdict
        ])
        .run(tauri::generate_context!())
        .expect("failed to run inVectus");
}

#[cfg(test)]
mod tests {
    #[test]
    fn shell_uses_core() {
        assert_eq!(super::engine_version(), "0.1.0");
    }

    #[test]
    fn import_form_maps_role_rules() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("x.csv");
        std::fs::write(&csv, "id,t\n1,a\n").unwrap();
        let form: super::ImportForm = serde_json::from_value(serde_json::json!({
            "collection": "c", "source_path": csv, "text": ["t", ""], "title": "",
            "id": "id", "date": null, "url": null, "role": "neutral",
            "role_column": "t", "role_rules": "a=coverage",
            "chunk_chars": 2000, "chunk_overlap": 200, "limit": null
        }))
        .unwrap();
        let req = super::request(form).unwrap();
        assert_eq!(req.mapping.text, vec!["t"]);
        assert_eq!(req.mapping.title, None);
        let pv = invectus_ingest::preview(&req, 5).unwrap();
        assert_eq!(pv.rows[0].role, invectus_core::Role::Coverage);
    }
}
