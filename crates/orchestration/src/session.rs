//! The open project, its readiness and its quick reads (search, verdicts).

use crate::{OpResult, SharedBoard, err, jobs};
use invectus_core::store::{CollectionSummary, JobRecord, VectorTableRecord};
use invectus_core::{Project, Role};
use invectus_embed::{EmbedOptions, ModelStore, OnnxEmbedder, query_encoder};
use invectus_index::{SearchMode, SearchOptions, SearchResult, VectorStore, select_table};
use invectus_ingest::Source;
use invectus_unsearch::{CheckOptions, CheckResult, check};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct Session {
    pub project: Project,
    pub vectors: VectorStore,
    /// Query encoder cached per vector space id.
    pub encoder: Option<(String, OnnxEmbedder)>,
    /// The local language model, loaded on first use and kept for the session.
    pub llm: Option<invectus_llm::LlamaGenerator>,
}

impl Session {
    pub fn open(project: Project) -> anyhow::Result<Self> {
        let vectors = VectorStore::open(&project)?;
        Ok(Self {
            project,
            vectors,
            encoder: None,
            llm: None,
        })
    }
}

/// The session slot: `None` until a project is opened.
#[derive(Clone, Default)]
pub struct Shared {
    slot: Arc<Mutex<Option<Session>>>,
    /// Held across app tool calls and project transitions, including gaps between session locks.
    pub(crate) transition: Arc<Mutex<()>>,
}

impl std::ops::Deref for Shared {
    type Target = Mutex<Option<Session>>;
    fn deref(&self) -> &Self::Target {
        &self.slot
    }
}

pub fn with_session<T>(
    session: &Shared,
    f: impl FnOnce(&mut Session) -> anyhow::Result<T>,
) -> OpResult<T> {
    let mut guard = session
        .try_lock()
        .map_err(|_| "busy: a job is running in this project".to_string())?;
    let s = guard.as_mut().ok_or("no project is open")?;
    f(s).map_err(err)
}

/// Background-only reads/writes wait for other short session operations instead of
/// misreporting ordinary read contention as a running job. Never call on the UI thread.
pub fn with_session_wait<T>(
    session: &Shared,
    f: impl FnOnce(&mut Session) -> anyhow::Result<T>,
) -> OpResult<T> {
    let mut guard = session.lock().map_err(err)?;
    let s = guard.as_mut().ok_or("no project is open")?;
    f(s).map_err(err)
}

/// A second handle on the open project for a job, taken without waiting.
pub fn job_project(session: &Shared) -> OpResult<Project> {
    let guard = session
        .try_lock()
        .map_err(|_| "busy: the open project is answering a request; try again".to_string())?;
    let s = guard.as_ref().ok_or("no project is open")?;
    s.project.try_clone().map_err(err)
}

/// Refuse project writes while a job works in the same project.
pub fn ensure_idle(board: &SharedBoard, project: &Project) -> anyhow::Result<()> {
    let root = project.root.display().to_string();
    if let Some(j) = board.lock().unwrap().running_in(&root) {
        anyhow::bail!("{} is running; wait for it or cancel it first", j.label);
    }
    Ok(())
}

/// A typed path with a leading `~` expanded to the home folder.
pub fn user_path(path: &str) -> PathBuf {
    let path = path.trim();
    match (path.strip_prefix("~/"), dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if path == "~" => dirs::home_dir().unwrap_or_default(),
        _ => PathBuf::from(path),
    }
}

pub fn default_projects_dir() -> String {
    dirs::home_dir()
        .map(|h| h.join("inVectus Projects").display().to_string())
        .unwrap_or_default()
}

#[derive(Serialize)]
pub struct TableInfo {
    pub collection: String,
    pub space_id: String,
    pub model_key: String,
    pub runtime: String,
    pub dimensions: usize,
    pub origin: String,
    pub vectors: usize,
    pub ann_partitions: Option<u64>,
    pub calibration: Option<CalibrationStatus>,
    pub regions: Option<RegionsStatus>,
    /// `indexing`, `indexed`, `outdated`, `interrupted`, `failed`, `cancelled`, `not_indexed`,
    /// `imported` (query-only vectors) or `unknown` (see `jobs::index_status`).
    pub index_status: String,
    /// When the last embed job of this space was recorded, and its error if it failed.
    pub indexed_at: Option<String>,
    pub index_error: Option<String>,
}

#[derive(Serialize)]
pub struct CalibrationStatus {
    pub method: String,
    pub sample: i64,
    /// Vectors when fitted; differs from `vectors` when the collection changed since.
    pub vectors: i64,
    pub fitted_at: Option<String>,
    pub holdout_auc: Option<f64>,
}

#[derive(Serialize)]
pub struct RegionsStatus {
    pub run_id: String,
    pub created_at: Option<String>,
    pub stats: serde_json::Value,
    /// The latest regions have a gap scan.
    pub scanned: bool,
}

#[derive(Serialize)]
pub struct CollectionInfo {
    #[serde(flatten)]
    pub summary: CollectionSummary,
    /// `csv`, `parquet`, `folder` or another importer's kind, and the source path when known.
    pub source_kind: Option<String>,
    pub source_path: Option<String>,
    /// The stored source can be imported again (`reimport_collection`).
    pub reimportable: bool,
}

#[derive(Serialize)]
pub struct ProjectInfo {
    pub root: String,
    pub name: String,
    pub warning: Option<String>,
    pub network_enabled: bool,
    pub preferred_agent_model: Option<String>,
    /// `INVECTUS_OFFLINE=1`: every network use is refused whatever the project switch says.
    pub offline_env: bool,
    pub collections: Vec<CollectionInfo>,
    pub tables: Vec<TableInfo>,
    /// Recorded jobs of this project, newest first (they survive restarts).
    pub activity: Vec<JobRecord>,
    /// The job running in this project now, if any.
    pub running: Option<jobs::JobInfo>,
}

/// The open project's collections, vector tables and how ready each is, from recorded facts.
pub fn project_info(s: &Session, board: &SharedBoard) -> anyhow::Result<ProjectInfo> {
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
        preferred_agent_model: s.project.preferred_agent_model()?,
        offline_env: invectus_core::net::offline(),
        collections,
        tables,
        activity,
        running,
    })
}

/// Open (or create) a project into the session. Refused while a job runs in a project: switching
/// would hide it.
pub fn open_project(
    session: &Shared,
    board: &SharedBoard,
    path: &str,
    create: bool,
) -> OpResult<ProjectInfo> {
    let _transition = session.transition.try_lock().map_err(|_| {
        "busy: an agent is answering a request; try switching projects again when it finishes"
            .to_string()
    })?;
    let path = user_path(path);
    let mut guard = session
        .try_lock()
        .map_err(|_| "busy: the open project is answering a request; try again".to_string())?;
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
    *guard = Some(Session::open(project).map_err(err)?);
    project_info(guard.as_ref().unwrap(), board).map_err(err)
}

/// Rename a project: the open one in place, any other on disk.
pub fn rename_project(session: &Shared, path: &str, name: &str) -> OpResult<String> {
    let root = std::fs::canonicalize(user_path(path)).map_err(err)?;
    let mut guard = session
        .try_lock()
        .map_err(|_| "busy: the open project is answering a request; try again".to_string())?;
    if let Some(s) = guard
        .as_mut()
        .filter(|s| std::fs::canonicalize(&s.project.root).ok().as_ref() == Some(&root))
    {
        s.project.rename(name).map_err(err)?;
        Ok(s.project.config.name.clone())
    } else {
        Project::rename_at(&root, name).map_err(err)
    }
}

/// Turn the project's network switch on or off (verification connectors use it).
pub fn set_network(session: &Shared, board: &SharedBoard, enabled: bool) -> OpResult<ProjectInfo> {
    with_session(session, |s| {
        s.project.set_network_enabled(enabled)?;
        project_info(s, board)
    })
}

pub fn set_agent_model(
    session: &Shared,
    board: &SharedBoard,
    project: &str,
    model: Option<&str>,
) -> OpResult<ProjectInfo> {
    with_session(session, |s| {
        anyhow::ensure!(
            s.project.root == std::path::Path::new(project),
            "the open project changed; choose the model again"
        );
        s.project.set_preferred_agent_model(model)?;
        project_info(s, board)
    })
}

/// The session's query encoder for `table`'s space, loaded on first use.
pub fn encoder_for<'a>(
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

/// A collection and, optionally, one of its vector spaces (else its only or default one).
#[derive(Deserialize)]
pub struct TableForm {
    #[serde(default)]
    pub project: Option<String>,
    pub collection: String,
    pub space: Option<String>,
}

impl TableForm {
    pub fn ensure_project(&self, project: &Project) -> anyhow::Result<()> {
        if let Some(root) = &self.project {
            anyhow::ensure!(
                project.root == std::path::Path::new(root),
                "the open project changed; reopen this gap before making changes"
            );
        }
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct SearchForm {
    pub collection: String,
    pub query: String,
    pub k: usize,
    pub role: Option<Role>,
    pub exhaustive: bool,
    pub space: Option<String>,
}

/// Nearest items with cosine similarity (not a probability).
pub fn search(session: &Shared, form: SearchForm) -> OpResult<SearchResult> {
    with_session(session, |s| {
        let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
        encoder_for(s, &table)?;
        let encoder = &mut s.encoder.as_mut().unwrap().1;
        invectus_index::search(
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

#[derive(Deserialize)]
pub struct CheckForm {
    pub collection: String,
    pub query: String,
    pub k: usize,
    pub role: Option<Role>,
    /// Use the ANN index when there is one; exhaustive otherwise.
    pub fast: bool,
    pub space: Option<String>,
}

/// inVectus search verdict: exists / near / open with P(exists) and the evidence.
pub fn check_idea(session: &Shared, form: CheckForm) -> OpResult<CheckResult> {
    with_session(session, |s| {
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
}

/// Score gaps over the latest regions (verdicts are kept). Refused while a job works in the
/// project.
pub fn scan_gaps(
    session: &Shared,
    board: &SharedBoard,
    form: TableForm,
    weights: Option<invectus_gaps::scan::Weights>,
) -> OpResult<invectus_gaps::ScanReport> {
    with_session(session, |s| {
        form.ensure_project(&s.project)?;
        ensure_idle(board, &s.project)?;
        let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
        let mut opts = invectus_gaps::ScanOptions::default();
        if let Some(w) = weights {
            opts.weights = w;
        }
        invectus_gaps::scan(&mut s.project, &s.vectors, &table, &opts)
    })
}

/// Write the latest gaps as `csv` or `markdown` into the project's `exports/` folder; returns
/// the file's path. Titles and links only.
pub fn export_gaps(session: &Shared, form: TableForm, format: &str) -> OpResult<String> {
    with_session(session, |s| {
        form.ensure_project(&s.project)?;
        let table = select_table(&s.project, &form.collection, form.space.as_deref())?;
        let list = invectus_gaps::list(&s.project, &table)?;
        let (text, ext) = match format {
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
}
