//! The desktop's background jobs: one at a time, each tied to the project it runs in, with
//! progress, cancellation and a short history. Jobs run on their own database connection, so
//! the open project keeps answering reads while one runs (SPEC §7.6).

use invectus_core::CancelToken;
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

/// Jobs kept in the session's history (newest first).
const HISTORY: usize = 30;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct JobInfo {
    pub id: u64,
    /// `index`, `calibrate`, `regions` or `download`.
    pub kind: String,
    /// The project root the job works in; `None` for model downloads (shared by projects).
    pub project: Option<String>,
    pub collection: Option<String>,
    pub space: Option<String>,
    /// Model keys the job uses (embedding models for `index`, the model for `download`).
    pub models: Vec<String>,
    pub label: String,
    pub phase: Option<String>,
    pub done: usize,
    pub total: usize,
    /// `running`, `completed`, `cancelled` or `failed`.
    pub state: String,
    pub error: Option<String>,
    pub report: Option<serde_json::Value>,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
}

impl JobInfo {
    pub fn new(kind: &str, project: Option<String>, label: String) -> Self {
        Self {
            id: 0,
            kind: kind.into(),
            project,
            collection: None,
            space: None,
            models: Vec::new(),
            label,
            phase: None,
            done: 0,
            total: 0,
            state: "running".into(),
            error: None,
            report: None,
            started_ms: now_ms(),
            finished_ms: None,
        }
    }

    pub fn running(&self) -> bool {
        self.state == "running"
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Default)]
pub struct Board {
    current: Option<JobInfo>,
    cancel: Option<CancelToken>,
    history: Vec<JobInfo>,
    next: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobsView {
    pub current: Option<JobInfo>,
    pub history: Vec<JobInfo>,
}

impl Board {
    /// Register a job, or refuse while another is running.
    pub fn begin(&mut self, mut info: JobInfo) -> Result<(JobInfo, CancelToken), String> {
        if let Some(c) = self.current.as_ref().filter(|c| c.running()) {
            return Err(format!(
                "{} is still running; wait for it or cancel it first",
                c.label
            ));
        }
        if let Some(done) = self.current.take() {
            self.push_history(done);
        }
        self.next += 1;
        info.id = self.next;
        let cancel = CancelToken::default();
        self.cancel = Some(cancel.clone());
        self.current = Some(info.clone());
        Ok((info, cancel))
    }

    pub fn progress(&mut self, id: u64, phase: &str, done: usize, total: usize) -> Option<JobInfo> {
        let c = self.current.as_mut().filter(|c| c.id == id)?;
        c.phase = Some(phase.into());
        c.done = done;
        c.total = total;
        Some(c.clone())
    }

    pub fn finish(
        &mut self,
        id: u64,
        result: Result<(serde_json::Value, bool), String>,
    ) -> Option<JobInfo> {
        let c = self.current.as_mut().filter(|c| c.id == id)?;
        c.finished_ms = Some(now_ms());
        match result {
            Ok((report, cancelled)) => {
                c.state = if cancelled { "cancelled" } else { "completed" }.into();
                c.report = Some(report);
            }
            Err(e) => {
                c.state = "failed".into();
                c.error = Some(e);
            }
        }
        self.cancel = None;
        Some(c.clone())
    }

    pub fn cancel(&self) {
        if let Some(c) = &self.cancel {
            c.cancel();
        }
    }

    pub fn running(&self) -> Option<&JobInfo> {
        self.current.as_ref().filter(|c| c.running())
    }

    /// A running job that works inside `root` (downloads never do).
    pub fn running_in(&self, root: &str) -> Option<&JobInfo> {
        self.running()
            .filter(|c| c.project.as_deref() == Some(root))
    }

    pub fn view(&self) -> JobsView {
        JobsView {
            current: self.current.clone(),
            history: self.history.clone(),
        }
    }

    fn push_history(&mut self, j: JobInfo) {
        self.history.insert(0, j);
        self.history.truncate(HISTORY);
    }
}

/// An embedding model the plan needs but cannot use because it is not installed.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct MissingModel {
    pub key: String,
    pub repo: String,
    pub megabytes: u64,
}

/// Which installed models indexing a collection will use, and why.
#[derive(Clone, Debug, Serialize, PartialEq, Default)]
pub struct IndexPlan {
    pub models: Vec<String>,
    pub missing: Vec<MissingModel>,
    pub notes: Vec<String>,
}

/// One vector table of a collection, as the planner sees it.
pub struct ExistingSpace<'a> {
    pub model_key: &'a str,
    /// `embedded` tables are re-embedded; `imported` ones (from gapmap) are query-only.
    pub origin: &'a str,
}

/// Choose models for indexing a collection. A collection keeps the spaces it has: every
/// embedded space is updated with its own model (never another), and a missing model is
/// reported rather than downloaded. A collection without spaces gets the default model, or
/// the first installed one; with none installed the plan is empty and says what is needed.
pub fn plan_index(
    existing: &[ExistingSpace],
    installed: &dyn Fn(&str) -> bool,
    default_model: &str,
    catalogue: &[(String, String, u64)],
) -> IndexPlan {
    let missing = |key: &str| {
        let (repo, megabytes) = catalogue
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, r, mb)| (r.clone(), *mb))
            .unwrap_or_default();
        MissingModel {
            key: key.into(),
            repo,
            megabytes,
        }
    };
    let mut plan = IndexPlan::default();
    let embedded: Vec<&str> = existing
        .iter()
        .filter(|e| e.origin == "embedded")
        .map(|e| e.model_key)
        .collect();
    if existing.iter().any(|e| e.origin != "embedded") {
        plan.notes.push(
            "imported vectors are query-only and are not updated; changed items keep their old \
             vectors there"
                .into(),
        );
    }
    if embedded.is_empty() {
        if installed(default_model) {
            plan.models.push(default_model.into());
        } else if let Some((k, _, _)) = catalogue.iter().find(|(k, _, _)| installed(k)) {
            plan.models.push(k.clone());
            plan.notes.push(format!(
                "the default model {default_model} is not installed; using {k}"
            ));
        } else {
            plan.missing.push(missing(default_model));
        }
        return plan;
    }
    for key in embedded {
        if installed(key) {
            plan.models.push(key.into());
        } else {
            plan.missing.push(missing(key));
        }
    }
    plan
}

/// How far a vector table is from its collection's latest import, from recorded facts only.
/// Timestamps are DuckDB `now()` values from the same database, so they compare as strings.
pub fn index_status(
    origin: &str,
    job: Option<(&str, Option<&str>)>,
    collection_updated: Option<&str>,
    vectors: usize,
    indexing_now: bool,
) -> &'static str {
    if indexing_now {
        return "indexing";
    }
    if origin != "embedded" {
        return "imported";
    }
    match job {
        None if vectors == 0 => "not_indexed",
        // Vectors without a job record (an older project): nothing says they are current.
        None => "unknown",
        Some(("running", _)) => "interrupted",
        Some(("failed", _)) => "failed",
        Some(("cancelled", _)) => "cancelled",
        Some(("completed", at)) => match (at, collection_updated) {
            (Some(at), Some(up)) if at >= up => "indexed",
            (Some(_), Some(_)) => "outdated",
            _ => "unknown",
        },
        Some(_) => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Vec<(String, String, u64)> {
        vec![
            ("bge".into(), "BAAI/bge".into(), 134),
            ("nomic".into(), "nomic-ai/nomic".into(), 548),
        ]
    }

    #[test]
    fn new_collection_uses_the_default_or_says_what_is_missing() {
        let all = |_: &str| true;
        let none = |_: &str| false;
        let only_nomic = |k: &str| k == "nomic";
        assert_eq!(plan_index(&[], &all, "bge", &cat()).models, vec!["bge"]);
        let p = plan_index(&[], &only_nomic, "bge", &cat());
        assert_eq!(p.models, vec!["nomic"]);
        assert_eq!(p.notes.len(), 1);
        let p = plan_index(&[], &none, "bge", &cat());
        assert!(p.models.is_empty());
        assert_eq!(p.missing[0].key, "bge");
        assert_eq!(p.missing[0].megabytes, 134);
    }

    #[test]
    fn existing_spaces_keep_their_own_models() {
        let only_bge = |k: &str| k == "bge";
        let spaces = [
            ExistingSpace {
                model_key: "bge",
                origin: "embedded",
            },
            ExistingSpace {
                model_key: "nomic",
                origin: "embedded",
            },
            ExistingSpace {
                model_key: "nomic",
                origin: "imported",
            },
        ];
        let p = plan_index(&spaces, &only_bge, "bge", &cat());
        assert_eq!(p.models, vec!["bge"]);
        assert_eq!(p.missing[0].key, "nomic");
        assert_eq!(p.notes.len(), 1);
        // Only imported vectors: nothing embedded to update, so a new space with the default.
        let p = plan_index(&spaces[2..], &only_bge, "bge", &cat());
        assert_eq!(p.models, vec!["bge"]);
    }

    #[test]
    fn status_comes_from_recorded_jobs_and_import_times() {
        let up = Some("2026-10-29 10:00:00");
        assert_eq!(index_status("embedded", None, up, 0, false), "not_indexed");
        assert_eq!(index_status("embedded", None, up, 5, false), "unknown");
        assert_eq!(index_status("imported", None, up, 5, false), "imported");
        assert_eq!(
            index_status("embedded", Some(("running", None)), up, 5, false),
            "interrupted"
        );
        assert_eq!(
            index_status("embedded", Some(("running", None)), up, 5, true),
            "indexing"
        );
        let done = |at| Some(("completed", Some(at)));
        assert_eq!(
            index_status("embedded", done("2026-10-29 10:00:05"), up, 5, false),
            "indexed"
        );
        assert_eq!(
            index_status("embedded", done("2026-10-29 09:00:00"), up, 5, false),
            "outdated"
        );
        assert_eq!(
            index_status("embedded", Some(("failed", None)), up, 5, false),
            "failed"
        );
    }

    #[test]
    fn one_job_at_a_time_and_stale_ids_are_ignored() {
        let mut b = Board::default();
        let (a, _) = b
            .begin(JobInfo::new("index", Some("/p".into()), "A".into()))
            .unwrap();
        assert!(
            b.begin(JobInfo::new("regions", Some("/p".into()), "B".into()))
                .is_err()
        );
        assert!(b.running_in("/p").is_some());
        assert!(b.running_in("/q").is_none());
        assert!(b.progress(a.id + 1, "x", 1, 2).is_none());
        assert_eq!(b.progress(a.id, "embed", 1, 2).unwrap().done, 1);
        let f = b.finish(a.id, Err("boom".into())).unwrap();
        assert_eq!(
            (f.state.as_str(), f.error.as_deref()),
            ("failed", Some("boom"))
        );
        let (c, _) = b.begin(JobInfo::new("download", None, "C".into())).unwrap();
        assert!(b.running_in("/p").is_none());
        assert_eq!(b.view().history[0].label, "A");
        b.finish(c.id, Ok((serde_json::json!({}), true)));
        assert_eq!(b.view().current.unwrap().state, "cancelled");
    }
}
