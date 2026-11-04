//! Orchestration shared by the desktop app and agent tools: the open project (a session), the
//! background job board, the import → index → map → scan → calibrate pipeline and readiness
//! (target product §11, A1). Nothing here knows about a window or a transport; callers pass an
//! [`Emit`] for job changes and run the blocking calls off their UI or async threads.

pub mod jobs;
mod pipeline;
mod session;

pub use jobs::{Board, ExistingSpace, IndexPlan, JobInfo, JobsView, MissingModel};
pub use pipeline::*;
pub use session::*;

use std::sync::{Arc, Mutex};

/// Results carry a message for the person (or agent) who asked, with the error's full chain.
pub type OpResult<T> = Result<T, String>;

/// The background job (one at a time) and this session's finished jobs.
pub type SharedBoard = Arc<Mutex<Board>>;

/// Where job changes go: the window's `job-event` in the app, a recorder in tests.
pub type Emit = Arc<dyn Fn(&JobInfo) + Send + Sync>;

pub fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}
