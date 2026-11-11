//! Desktop shell: Tauri commands over the shared orchestration crate and the engine crates the
//! `invectus` CLI uses. Sessions, jobs, the import pipeline and readiness live in
//! `invectus-orchestration`; this file only adapts them to commands and the `job-event`.

use invectus_embed::ModelStore;
use invectus_gaps::{GapDetail, GapList, GapView, ScanOptions, ScanReport};
use invectus_index::{SearchResult, select_table};
use invectus_orchestration::{
    self as orch, CheckForm, Emit, ImportForm, ImportOutcome, ImportPreview, IndexPlan, JobInfo,
    JobsView, LlmInfo, ModelInfo, OpResult as CmdResult, ProjectInfo, SearchForm, Shared,
    SharedBoard, SourceInfo, TableForm, encoder_for, err, with_session, with_session_wait,
};
use invectus_regions::{MapData, map_data};
use invectus_unsearch::CheckResult;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

#[derive(Default)]
struct AppState {
    session: Shared,
    /// The background job (one at a time) and this session's finished jobs.
    board: SharedBoard,
}

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

/// Job changes go to the window as `job-event`, carrying the whole job.
fn emitter(app: &AppHandle) -> Emit {
    let app = app.clone();
    Arc::new(move |j: &JobInfo| {
        let _ = app.emit("job-event", j);
    })
}

#[tauri::command]
fn engine_version() -> &'static str {
    invectus_core::VERSION
}

#[tauri::command]
fn default_projects_dir() -> String {
    orch::default_projects_dir()
}

#[tauri::command]
async fn open_project(
    state: State<'_, AppState>,
    path: String,
    create: bool,
) -> CmdResult<ProjectInfo> {
    let (session, board) = (state.session.clone(), state.board.clone());
    off_thread(move || orch::open_project(&session, &board, &path, create)).await
}

#[tauri::command]
async fn project_info(state: State<'_, AppState>) -> CmdResult<ProjectInfo> {
    let (session, board) = (state.session.clone(), state.board.clone());
    off_thread(move || with_session(&session, |s| orch::project_info(s, &board))).await
}

#[tauri::command]
async fn rename_project(
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> CmdResult<String> {
    let session = state.session.clone();
    off_thread(move || orch::rename_project(&session, &path, &name)).await
}

#[tauri::command]
async fn preview_import(
    state: State<'_, AppState>,
    form: ImportForm,
    rows: Option<usize>,
) -> CmdResult<ImportPreview> {
    let session = state.session.clone();
    off_thread(move || orch::preview_import(&session, form, rows.unwrap_or(20))).await
}

/// Import a collection, then start indexing it with the plan's installed models.
#[tauri::command]
async fn import_collection(
    app: AppHandle,
    state: State<'_, AppState>,
    form: ImportForm,
) -> CmdResult<ImportOutcome> {
    let (session, board, emit) = (state.session.clone(), state.board.clone(), emitter(&app));
    off_thread(move || {
        let req = orch::request_for_session(&session, form)?;
        orch::import_then_index(&emit, &session, &board, req)
    })
    .await
}

/// Import a collection again from its stored source, mapping and chunking, then index it.
#[tauri::command]
async fn reimport_collection(
    app: AppHandle,
    state: State<'_, AppState>,
    collection: String,
) -> CmdResult<ImportOutcome> {
    let (session, board, emit) = (state.session.clone(), state.board.clone(), emitter(&app));
    off_thread(move || {
        let req = orch::reimport_request(&session, collection)?;
        orch::import_then_index(&emit, &session, &board, req)
    })
    .await
}

/// Which installed models indexing a collection would use, and which are missing.
#[tauri::command]
async fn index_plan(state: State<'_, AppState>, collection: String) -> CmdResult<IndexPlan> {
    let session = state.session.clone();
    off_thread(move || with_session(&session, |s| orch::plan_for(s, &collection))).await
}

/// Columns and sample rows of a source; a pulled source (Google Sheet, Notion) is looked up in
/// the open project.
#[tauri::command]
async fn inspect_source(
    state: State<'_, AppState>,
    path: String,
    sheet: Option<String>,
) -> CmdResult<SourceInfo> {
    let session = state.session.clone();
    off_thread(move || {
        let root = session
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|s| s.project.root.clone()));
        orch::inspect_source(&path, sheet, root.as_deref())
    })
    .await
}

/// Pull a Google Sheet or Notion database into the open project (network; needs the project's
/// network switch). Runs as a background job.
#[tauri::command]
async fn pull_source(
    app: AppHandle,
    state: State<'_, AppState>,
    spec: String,
) -> CmdResult<JobInfo> {
    let (session, board, emit) = (state.session.clone(), state.board.clone(), emitter(&app));
    off_thread(move || orch::start_pull(&emit, &session, &board, &spec, false)).await
}

/// Where the Notion token comes from (`keychain`, `environment`), or `None`.
#[tauri::command]
async fn notion_status() -> CmdResult<Option<String>> {
    off_thread(|| orch::notion_status().map(|s| s.map(String::from))).await
}

/// Save the Notion token in the system keychain; an empty token removes it.
#[tauri::command]
async fn set_notion_token(token: String) -> CmdResult<Option<String>> {
    off_thread(move || orch::set_notion_token(&token).map(|s| s.map(String::from))).await
}

#[tauri::command]
async fn set_network(state: State<'_, AppState>, enabled: bool) -> CmdResult<ProjectInfo> {
    let (session, board) = (state.session.clone(), state.board.clone());
    off_thread(move || orch::set_network(&session, &board, enabled)).await
}

#[tauri::command]
async fn set_agent_model(
    state: State<'_, AppState>,
    project: String,
    model: Option<String>,
) -> CmdResult<ProjectInfo> {
    let (session, board) = (state.session.clone(), state.board.clone());
    off_thread(move || orch::set_agent_model(&session, &board, &project, model.as_deref())).await
}

#[tauri::command]
fn list_llms() -> CmdResult<Vec<LlmInfo>> {
    orch::list_llms()
}

#[tauri::command]
fn install_model(app: AppHandle, state: State<AppState>, model: String) -> CmdResult<JobInfo> {
    orch::install_model(&emitter(&app), &state.session, &state.board, &model)
}

#[tauri::command]
fn list_models() -> CmdResult<Vec<ModelInfo>> {
    orch::list_models()
}

#[tauri::command]
fn start_index(
    app: AppHandle,
    state: State<AppState>,
    collection: String,
    models: Option<Vec<String>>,
    // Also map and calibrate afterwards (the import pipeline; used by Retry).
    map: Option<bool>,
) -> CmdResult<JobInfo> {
    orch::start_index(
        &emitter(&app),
        &state.session,
        &state.board,
        collection,
        models,
        map.unwrap_or(false),
    )
}

#[tauri::command]
fn cancel_job(state: State<AppState>, id: u64, project: Option<String>) -> CmdResult<()> {
    state
        .board
        .lock()
        .unwrap()
        .cancel_matching(id, project.as_deref())
}

/// The running job (if any) and this session's finished ones.
#[tauri::command]
fn jobs(state: State<AppState>) -> JobsView {
    state.board.lock().unwrap().view()
}

#[tauri::command]
async fn search(state: State<'_, AppState>, form: SearchForm) -> CmdResult<SearchResult> {
    let session = state.session.clone();
    off_thread(move || orch::search(&session, form)).await
}

#[tauri::command]
fn start_calibration(
    app: AppHandle,
    state: State<AppState>,
    collection: String,
    space: Option<String>,
    // Paraphrase queries with the local language model (SPEC §6.2) instead of titles.
    llm: Option<bool>,
) -> CmdResult<JobInfo> {
    orch::start_calibration(
        &emitter(&app),
        &state.session,
        &state.board,
        collection,
        space,
        llm.unwrap_or(false),
    )
}

#[tauri::command]
async fn check_idea(state: State<'_, AppState>, form: CheckForm) -> CmdResult<CheckResult> {
    let session = state.session.clone();
    off_thread(move || orch::check_idea(&session, form)).await
}

#[tauri::command]
fn start_regions(
    app: AppHandle,
    state: State<AppState>,
    collection: String,
    space: Option<String>,
) -> CmdResult<JobInfo> {
    orch::start_regions(
        &emitter(&app),
        &state.session,
        &state.board,
        collection,
        space,
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
        with_session_wait(&session, |s| {
            let table = select_table(&s.project, &collection, space.as_deref())?;
            map_data(&s.project, &table)
        })
    })
    .await
}

/// Score gaps over the latest regions (verdicts are kept).
#[tauri::command]
async fn scan_gaps(
    state: State<'_, AppState>,
    form: TableForm,
    weights: Option<invectus_gaps::scan::Weights>,
) -> CmdResult<ScanReport> {
    let (session, board) = (state.session.clone(), state.board.clone());
    off_thread(move || orch::scan_gaps(&session, &board, form, weights)).await
}

/// Ranked gaps of the latest scan with their verdicts.
#[tauri::command]
async fn list_gaps(state: State<'_, AppState>, form: TableForm) -> CmdResult<GapList> {
    let session = state.session.clone();
    off_thread(move || {
        with_session_wait(&session, |s| {
            form.ensure_project(&s.project)?;
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            invectus_gaps::list(&s.project, &table)
        })
    })
    .await
}

/// Project-local bookmarks and decisions for this collection and embedding space.
#[tauri::command]
async fn saved_overview(
    state: State<'_, AppState>,
    form: TableForm,
) -> CmdResult<invectus_gaps::saved::SavedOverview> {
    let session = state.session.clone();
    off_thread(move || {
        with_session_wait(&session, |s| {
            form.ensure_project(&s.project)?;
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            invectus_gaps::saved::overview(&s.project, &table)
        })
    })
    .await
}

#[tauri::command]
async fn bookmark_gap(state: State<'_, AppState>, form: TableForm, gap_id: i32) -> CmdResult<()> {
    let session = state.session.clone();
    off_thread(move || {
        with_session_wait(&session, |s| {
            form.ensure_project(&s.project)?;
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            invectus_gaps::saved::bookmark(&s.project, &table, gap_id)
        })
    })
    .await
}

#[tauri::command]
async fn remove_gap_bookmark(
    state: State<'_, AppState>,
    form: TableForm,
    kind: String,
    core_key: String,
) -> CmdResult<()> {
    let session = state.session.clone();
    off_thread(move || {
        with_session_wait(&session, |s| {
            form.ensure_project(&s.project)?;
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            invectus_gaps::saved::remove_bookmark(
                &s.project,
                &table,
                &kind,
                &serde_json::from_str::<Vec<i64>>(&core_key)?,
            )
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
    t: Option<f32>,
) -> CmdResult<GapDetail> {
    let session = state.session.clone();
    off_thread(move || {
        with_session_wait(&session, |s| {
            form.ensure_project(&s.project)?;
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            invectus_gaps::detail_at(&s.project, &table, gap_id, t.unwrap_or(0.5))
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
    expected_suggestion: Option<String>,
) -> CmdResult<GapView> {
    let session = state.session.clone();
    off_thread(move || {
        with_session_wait(&session, |s| {
            form.ensure_project(&s.project)?;
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            if let Some(expected) = expected_suggestion {
                invectus_gaps::respond_to_suggestion(
                    &s.project,
                    &table,
                    gap_id,
                    &expected,
                    Some(&verdict),
                    &note,
                )
            } else {
                invectus_gaps::set_verdict(&s.project, &table, gap_id, &verdict, &note)
            }
        })
    })
    .await
}

/// Set a gap's pending agent suggestion aside without deciding (the decision is unchanged).
#[tauri::command]
async fn dismiss_gap_suggestion(
    state: State<'_, AppState>,
    form: TableForm,
    gap_id: i32,
    expected_suggestion: String,
) -> CmdResult<GapView> {
    let session = state.session.clone();
    off_thread(move || {
        with_session_wait(&session, |s| {
            form.ensure_project(&s.project)?;
            let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
            invectus_gaps::respond_to_suggestion(
                &s.project,
                &table,
                gap_id,
                &expected_suggestion,
                None,
                "",
            )
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
            form.ensure_project(&s.project)?;
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
            form.ensure_project(&s.project)?;
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
    off_thread(move || orch::export_gaps(&session, form, &format)).await
}

/// The clients Settings lists.
#[tauri::command]
fn connect_clients() -> CmdResult<Vec<orch::connect::ClientInfo>> {
    use orch::connect;
    Ok(connect::catalog(&connect::Env::system().map_err(err)?))
}

/// What connecting an agent client would change (Settings → Connect an AI agent). Reads only.
/// `custom` takes the client's JSON configuration file and the key holding its servers.
#[tauri::command]
fn connect_plan(
    client: String,
    file: Option<String>,
    key: Option<String>,
) -> CmdResult<orch::connect::ConnectPlan> {
    use orch::connect;
    let binary = connect::server_binary().map_err(err)?;
    if client == connect::CUSTOM {
        let file = file.ok_or_else(|| "give the client's configuration file".to_string())?;
        let key = key.unwrap_or_else(|| "mcpServers".into());
        return connect::plan_custom(std::path::Path::new(&file), &key, &binary).map_err(err);
    }
    let env = connect::Env::system().map_err(err)?;
    connect::plan(connect::Client::parse(&client).map_err(err)?, &binary, &env).map_err(err)
}

/// Connect a client after the person agreed: the plan is recomputed here, never taken from
/// the window.
#[tauri::command]
fn connect_apply(client: String, file: Option<String>, key: Option<String>) -> CmdResult<String> {
    let plan = connect_plan(client, file, key)?;
    orch::connect::apply(&plan).map_err(err)
}

#[tauri::command]
fn connect_remove(client: String, file: Option<String>, key: Option<String>) -> CmdResult<String> {
    use orch::connect;
    if client == connect::CUSTOM {
        let file = file.ok_or_else(|| "give the client's configuration file".to_string())?;
        let key = key.unwrap_or_else(|| "mcpServers".into());
        return connect::disconnect_custom(std::path::Path::new(&file), &key).map_err(err);
    }
    let env = connect::Env::system().map_err(err)?;
    connect::disconnect(connect::Client::parse(&client).map_err(err)?, &env).map_err(err)
}

#[tauri::command]
fn connect_status() -> CmdResult<Vec<orch::connect::ConnectStatus>> {
    use orch::connect;
    let env = connect::Env::system().map_err(err)?;
    Ok(connect::status(
        connect::server_binary().ok().as_deref(),
        &env,
    ))
}

/// Who a client's CLI is signed in as (`claude auth status`, `codex login status`). Runs the
/// CLI, so it is asked for separately from `connect_status`.
#[tauri::command]
async fn connect_account(client: String) -> CmdResult<Option<orch::connect::Account>> {
    use orch::connect;
    let client = connect::Client::parse(&client).map_err(err)?;
    off_thread(move || {
        Ok(connect::account(
            client,
            &connect::Env::system().map_err(err)?,
        ))
    })
    .await
}

/// Star inVectus on GitHub through the person's `gh` login, after they pressed Star.
#[tauri::command]
async fn star_repository() -> CmdResult<orch::github::StarOutcome> {
    off_thread(|| orch::github::star().map_err(err)).await
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

/// Listen for agent calls from `invectus mcp` (target product §8 stage 2). They run in this
/// app's session and job board; each call is also sent to the window as `agent-activity`.
fn start_agent_handoff(app: &AppHandle, session: Shared, board: SharedBoard) {
    let path = match orch::handoff::socket_path() {
        Ok(p) => p,
        Err(e) => return eprintln!("agent hand-off disabled: {e:#}"),
    };
    let window = app.clone();
    let activity: Arc<dyn Fn(&serde_json::Value) + Send + Sync> =
        Arc::new(move |event: &serde_json::Value| {
            let _ = window.emit("agent-activity", event);
        });
    let handler = orch::handoff::app_handler(session, board, emitter(app), activity);
    if let Err(e) = orch::handoff::serve(&path, handler) {
        eprintln!("agent hand-off disabled: {e:#}");
    }
}

pub fn run() {
    let state = AppState::default();
    let (session, board) = (state.session.clone(), state.board.clone());
    tauri::Builder::default()
        .setup(move |app| {
            start_agent_handoff(app.handle(), session.clone(), board.clone());
            // A release executable launched outside its .app bundle has no Dock icon by
            // default. Set the same icon explicitly, keeping its existing WebKit profile.
            #[cfg(target_os = "macos")]
            {
                use objc2::{AllocAnyThread, MainThreadMarker};
                use objc2_app_kit::{NSApplication, NSImage};
                use objc2_foundation::NSData;
                let main_thread =
                    MainThreadMarker::new().ok_or("app setup requires the main thread")?;
                let data = NSData::with_bytes(include_bytes!("../icons/icon.icns"));
                let icon =
                    NSImage::initWithData(NSImage::alloc(), &data).ok_or("invalid app icon")?;
                let app = NSApplication::sharedApplication(main_thread);
                // SAFETY: the main-thread marker above verifies AppKit's thread requirement;
                // `icon` is a valid retained NSImage for the duration of the setter call.
                unsafe {
                    app.setApplicationIconImage(Some(&icon));
                }
            }
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            engine_version,
            default_projects_dir,
            open_project,
            project_info,
            rename_project,
            inspect_source,
            pull_source,
            notion_status,
            set_notion_token,
            set_network,
            set_agent_model,
            list_llms,
            install_model,
            default_gap_weights,
            export_gaps,
            open_url,
            connect_clients,
            connect_plan,
            connect_apply,
            connect_remove,
            connect_status,
            connect_account,
            star_repository,
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
            saved_overview,
            bookmark_gap,
            remove_gap_bookmark,
            set_gap_verdict,
            dismiss_gap_suggestion,
            describe_gap,
            verify_gap
        ])
        .run(tauri::generate_context!())
        .expect("failed to run inVectus");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_uses_core() {
        assert_eq!(engine_version(), "0.2.0");
    }
}
