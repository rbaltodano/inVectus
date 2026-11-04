//! Import, then index → map → gap scan → calibration as background jobs; model lists and
//! downloads. One job at a time, tied to its project (SPEC §7.6).

use crate::jobs::{self, ExistingSpace, IndexPlan, JobInfo};
use crate::{
    Emit, OpResult, Session, Shared, SharedBoard, ensure_idle, err, job_project, user_path,
    with_session,
};
use invectus_core::{CancelToken, Role};
use invectus_embed::{
    EmbedOptions, ModelStore, ModelTokenizer, OnnxEmbedder, query_encoder, registry,
};
use invectus_gaps::ScanOptions;
use invectus_index::{EmbedJobOptions, VectorStore, run_embed, select_table};
use invectus_ingest::{
    ChunkPolicy, ImportReport, ImportRequest, Mapping, MetaMapping, Preview, RoleMapping, Source,
    TokenChunking,
};
use invectus_regions::{RegionOptions, run_regions};
use invectus_unsearch::{CalibrateOptions, run_calibration};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// A source and its column mapping, as the import form (or an agent) gives them.
#[derive(Deserialize)]
pub struct ImportForm {
    pub collection: String,
    pub source_path: String,
    pub text: Vec<String>,
    pub title: Option<String>,
    pub id: Option<String>,
    pub date: Option<String>,
    pub url: Option<String>,
    pub role: Option<Role>,
    pub role_column: Option<String>,
    pub role_rules: Option<String>,
    #[serde(default)]
    pub chunk_tokenizer: Option<String>,
    #[serde(default = "default_chunk_tokens")]
    pub chunk_tokens: usize,
    #[serde(default = "default_chunk_token_overlap")]
    pub chunk_token_overlap: usize,
    pub chunk_chars: usize,
    pub chunk_overlap: usize,
    pub limit: Option<usize>,
}

fn default_chunk_tokens() -> usize {
    TokenChunking::DEFAULT_MAX_TOKENS
}
fn default_chunk_token_overlap() -> usize {
    TokenChunking::DEFAULT_OVERLAP_TOKENS
}

pub fn load_chunk_tokenizer(policy: &ChunkPolicy) -> anyhow::Result<Option<ModelTokenizer>> {
    policy
        .tokens
        .as_ref()
        .map(|t| ModelTokenizer::load(&ModelStore::new(ModelStore::default_root()?), &t.model_key))
        .transpose()
}

/// The import request for `f`; a collection that exists keeps its saved chunking.
pub fn request_for_session(session: &Shared, f: ImportForm) -> OpResult<ImportRequest> {
    let saved = with_session(session, |s| {
        s.project
            .store
            .collection(&f.collection)?
            .map(|c| serde_json::from_value::<ChunkPolicy>(c.chunking))
            .transpose()
            .map_err(Into::into)
    })?;
    request_with_policy(f, saved).map_err(err)
}

pub fn request_with_policy(
    f: ImportForm,
    saved: Option<ChunkPolicy>,
) -> anyhow::Result<ImportRequest> {
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
        chunking: if let Some(policy) = saved {
            policy
        } else {
            match f.chunk_tokenizer {
                Some(key) => {
                    let tokenizer =
                        ModelTokenizer::load(&ModelStore::new(ModelStore::default_root()?), &key)?;
                    ChunkPolicy::tokens(&tokenizer, f.chunk_tokens, f.chunk_token_overlap)
                }
                None => ChunkPolicy {
                    max_chars: f.chunk_chars,
                    overlap_chars: f.chunk_overlap,
                    tokens: None,
                },
            }
        },
        limit: f.limit,
    })
}

#[derive(Serialize)]
pub struct ImportPreview {
    #[serde(flatten)]
    pub preview: Preview,
    pub chunk_counts: Vec<usize>,
}

/// The first `rows` items an import would produce, and how many chunks each becomes.
pub fn preview_import(session: &Shared, form: ImportForm, rows: usize) -> OpResult<ImportPreview> {
    let req = request_for_session(session, form)?;
    let preview = invectus_ingest::preview(&req, rows).map_err(err)?;
    let tokenizer = load_chunk_tokenizer(&req.chunking).map_err(err)?;
    let chunk_counts = preview
        .rows
        .iter()
        .map(|r| {
            req.chunking
                .split(
                    &r.text,
                    tokenizer
                        .as_ref()
                        .map(|t| t as &dyn invectus_core::TokenOffsets),
                )
                .map(|chunks| chunks.len())
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .map_err(err)?;
    Ok(ImportPreview {
        preview,
        chunk_counts,
    })
}

#[derive(Serialize)]
pub struct ImportOutcome {
    pub report: ImportReport,
    /// Which models indexing uses, or what is missing.
    pub plan: IndexPlan,
    /// The indexing job started after the import, if the plan had models.
    pub job: Option<JobInfo>,
    /// Why indexing could not start (the import itself succeeded).
    pub index_error: Option<String>,
}

/// The request that imports a collection again from its stored source, mapping and chunking.
/// Source files are not watched: this is how changes in them reach the collection.
pub fn reimport_request(session: &Shared, collection: String) -> OpResult<ImportRequest> {
    with_session(session, |s| {
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
}

/// Import a collection, then start indexing it (with mapping) using the plan's installed models.
pub fn import_then_index(
    emit: &Emit,
    session: &Shared,
    board: &SharedBoard,
    req: ImportRequest,
) -> OpResult<ImportOutcome> {
    let collection = req.collection.clone();
    let (report, plan) = with_session(session, |s| {
        ensure_idle(board, &s.project)?;
        let tokenizer = load_chunk_tokenizer(&req.chunking)?;
        let report = invectus_ingest::import_with(
            &mut s.project,
            &req,
            tokenizer
                .as_ref()
                .map(|t| t as &dyn invectus_core::TokenOffsets),
        )?;
        let plan = plan_for(s, &req.collection)?;
        Ok((report, plan))
    })?;
    let (job, index_error) = if plan.models.is_empty() {
        (None, None)
    } else {
        match start_index_job(emit, session, board, collection, plan.models.clone(), true) {
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

/// Which installed models indexing a collection would use, and which are missing.
pub fn plan_for(s: &Session, collection: &str) -> anyhow::Result<IndexPlan> {
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

#[derive(Serialize)]
pub struct SourceInfo {
    /// `csv`, `parquet` or `folder`.
    pub kind: String,
    pub columns: Vec<String>,
    /// The first rows, for guessing and showing the column mapping.
    pub sample: Vec<Vec<Option<String>>>,
    /// Rows read for this look (at most `SOURCE_LOOK_ROWS`).
    pub rows_read: usize,
    pub unreadable: usize,
}

const SOURCE_LOOK_ROWS: usize = 1000;

/// Columns and sample rows of an import source, before any mapping is chosen.
pub fn inspect_source(path: &str) -> OpResult<SourceInfo> {
    let source = Source::detect(&user_path(path)).map_err(err)?;
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
}

#[derive(Serialize)]
pub struct LlmInfo {
    pub key: String,
    pub repo: String,
    pub megabytes: u64,
    pub installed: bool,
    pub default: bool,
    pub note: String,
}

pub fn list_llms() -> OpResult<Vec<LlmInfo>> {
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

#[derive(Serialize)]
pub struct ModelInfo {
    pub key: String,
    pub repo: String,
    pub dimensions: usize,
    pub megabytes: u64,
    pub installed: bool,
    pub default: bool,
    pub note: String,
}

pub fn list_models() -> OpResult<Vec<ModelInfo>> {
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

/// Download an embedding model at its pinned revision: explicit network use, refused while the
/// open project is offline; checksums are verified. Runs as a job.
pub fn install_model(
    emit: &Emit,
    session: &Shared,
    board: &SharedBoard,
    model: &str,
) -> OpResult<JobInfo> {
    let spec = registry::spec(model).map_err(err)?;
    invectus_core::net::ensure_online(&format!("downloading model {}", spec.key)).map_err(err)?;
    if let Ok(guard) = session.try_lock()
        && guard
            .as_ref()
            .is_some_and(|s| !s.project.config.network_enabled)
    {
        return Err("offline mode is on for this project: turn it off to download a model".into());
    }
    let mut info = JobInfo::new("download", None, format!("Downloading {}", spec.key));
    info.models = vec![spec.key.into()];
    spawn_job(
        emit,
        board,
        info,
        Box::new(move |_cancel, tick| {
            let store = ModelStore::new(ModelStore::default_root()?);
            store.download(spec, &mut |line| tick(line, 0, 0))?;
            Ok((serde_json::json!({"model": spec.key}), false))
        }),
    )
}

/// A job's work: reports progress through the tick and returns its report and whether it was
/// cancelled.
pub type Work = Box<
    dyn FnOnce(
            &CancelToken,
            &mut dyn FnMut(&str, usize, usize),
        ) -> anyhow::Result<(serde_json::Value, bool)>
        + Send,
>;

/// Register a job on the board and run it on its own thread. Every change is emitted carrying
/// the whole job, including the project it belongs to.
pub fn spawn_job(emit: &Emit, board: &SharedBoard, info: JobInfo, work: Work) -> OpResult<JobInfo> {
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

/// Index a collection with `models`; with `map`, then lay it out (regions, for the map) and
/// fit a title-based calibration on the first model's space, so a fresh import ends ready to
/// explore and search. Mapping is skipped when nothing changed and regions already exist; a
/// calibration failure (e.g. too few titled items) is reported, not fatal.
pub fn start_index_job(
    emit: &Emit,
    session: &Shared,
    board: &SharedBoard,
    collection: String,
    models: Vec<String>,
    map: bool,
) -> OpResult<JobInfo> {
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
/// models, else the default. Resumable: items already embedded are skipped. With `map`, also
/// map and calibrate afterwards (the import pipeline).
pub fn start_index(
    emit: &Emit,
    session: &Shared,
    board: &SharedBoard,
    collection: String,
    models: Option<Vec<String>>,
    map: bool,
) -> OpResult<JobInfo> {
    let models = match models {
        Some(m) if !m.is_empty() => m,
        _ => {
            let guard = session.try_lock().map_err(|_| "busy; try again")?;
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
    start_index_job(emit, session, board, collection, models, map)
}

/// Calibrate a collection's vector space as a job; cancelling saves nothing. With `use_llm`,
/// paraphrase queries with the local language model (SPEC §6.2) instead of titles.
pub fn start_calibration(
    emit: &Emit,
    session: &Shared,
    board: &SharedBoard,
    collection: String,
    space: Option<String>,
    use_llm: bool,
) -> OpResult<JobInfo> {
    let project = job_project(session)?;
    let table = select_table(&project, &collection, space.as_deref()).map_err(err)?;
    // Borrow the session's loaded language model rather than loading a second copy.
    let borrowed = if use_llm {
        session
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
    let session = session.clone();
    spawn_job(
        emit,
        board,
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

/// Find regions and lay out the map as a job; cancelling keeps the previous regions.
pub fn start_regions(
    emit: &Emit,
    session: &Shared,
    board: &SharedBoard,
    collection: String,
    space: Option<String>,
) -> OpResult<JobInfo> {
    let mut project = job_project(session)?;
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
        emit,
        board,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{open_project, project_info};
    use invectus_core::Project;
    use std::sync::{Arc, Mutex};

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
        Arc::new(Mutex::new(Some(Session::open(project).unwrap())))
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
                "role_rules": null, "chunk_tokenizer": registry::DEFAULT_MODEL,
                "chunk_chars": 2000, "chunk_overlap": 200, "limit": null
            }))
            .unwrap()
        };
        let import = || {
            let mut g = session.lock().unwrap();
            let s = g.as_mut().unwrap();
            ensure_idle(&board, &s.project).unwrap();
            let req = request_with_policy(form(), None).unwrap();
            assert_eq!(req.chunking.tokens.as_ref().unwrap().max_tokens, 448);
            let tokenizer = load_chunk_tokenizer(&req.chunking).unwrap();
            invectus_ingest::import_with(
                &mut s.project,
                &req,
                tokenizer
                    .as_ref()
                    .map(|t| t as &dyn invectus_core::TokenOffsets),
            )
            .unwrap();
            plan_for(s, "notes").unwrap()
        };
        let status = || {
            let g = session.lock().unwrap();
            let i = project_info(g.as_ref().unwrap(), &board).unwrap();
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
            let i = project_info(g.as_ref().unwrap(), &board).unwrap();
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
        let other = other.display().to_string();
        let e = open_project(&session, &board, &other, false).err().unwrap();
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
        assert!(open_project(&session, &board, &other, false).is_ok());
        // A model download (no project) never blocks switching.
        board
            .lock()
            .unwrap()
            .begin(JobInfo::new("download", None, "d".into()))
            .unwrap();
        let first = dir.path().join("a").display().to_string();
        assert!(open_project(&session, &board, &first, false).is_ok());
    }

    #[test]
    fn import_form_maps_role_rules() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("x.csv");
        std::fs::write(&csv, "id,t\n1,a\n").unwrap();
        let mut form: ImportForm = serde_json::from_value(serde_json::json!({
            "collection": "c", "source_path": csv, "text": ["t", ""], "title": "",
            "id": "id", "date": null, "url": null, "role": "neutral",
            "role_column": "t", "role_rules": "a=coverage",
            "chunk_chars": 2000, "chunk_overlap": 200, "limit": null
        }))
        .unwrap();
        form.chunk_tokenizer = Some("not-installed".into());
        let req = request_with_policy(form, Some(ChunkPolicy::default())).unwrap();
        assert!(req.chunking.tokens.is_none());
        assert_eq!(req.mapping.text, vec!["t"]);
        assert_eq!(req.mapping.title, None);
        let pv = invectus_ingest::preview(&req, 5).unwrap();
        assert_eq!(pv.rows[0].role, invectus_core::Role::Coverage);
    }
}
