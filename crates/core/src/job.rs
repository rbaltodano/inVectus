//! Cancellation and progress for long-running jobs (SPEC §7.6).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Default, Debug)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Progress update: `done` of `total` units in the named phase.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Progress {
    pub job_id: String,
    pub phase: String,
    pub done: usize,
    pub total: usize,
}

pub type ProgressFn<'a> = &'a mut dyn FnMut(&Progress);
