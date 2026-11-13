//! Network gate. Import, embedding, search, regions and gap scans never call the network.
//! Model downloads, pulls, listings and outside-source checks call [`ensure_online`]. GitHub
//! starring and client sign-in probes check [`offline`]. All of them refuse the work when
//! offline mode is in force (SPEC §7.7).
//!
//! Offline mode comes from three places, and any one of them is enough:
//! - `INVECTUS_OFFLINE=1` in the environment (set by the person, read only);
//! - [`force_offline`], called once by the CLI for `--offline` (no environment change);
//! - [`scoped_offline`], which applies a caller's restriction to the current thread only, so a
//!   request forwarded from an offline agent can never be run online by the app that serves it.
//!
//! Work moved to another thread must carry the policy with it: capture [`offline`] before
//! spawning and wrap the work in [`scoped_offline`] (the job runner does this).

use anyhow::{Result, bail};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

static FORCED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static SCOPED: Cell<bool> = const { Cell::new(false) };
}

/// Whether network use is refused here and now.
pub fn offline() -> bool {
    FORCED.load(Ordering::SeqCst)
        || SCOPED.with(Cell::get)
        || std::env::var("INVECTUS_OFFLINE").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Refuse network use for the rest of this process (the CLI's `--offline`). Cannot be undone.
pub fn force_offline() {
    FORCED.store(true, Ordering::SeqCst);
}

/// Run `f` with offline mode in force on this thread when `offline` is true. A `false` never
/// lifts a restriction that is already in force (from the environment, [`force_offline`] or an
/// outer scope): policies only get stricter.
pub fn scoped_offline<T>(offline: bool, f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCOPED.with(|c| c.set(self.0));
        }
    }
    let before = SCOPED.with(Cell::get);
    let _restore = Restore(before);
    SCOPED.with(|c| c.set(before || offline));
    f()
}

pub fn ensure_online(what: &str) -> Result<()> {
    if offline() {
        bail!("offline mode: refusing network access for {what}");
    }
    Ok(())
}

// ---- Outbound call log -------------------------------------------------------------------

/// One network call, as the UI shows it: when, which host, why, and how it ended. Never the
/// path, query, headers or body, so no token, cursor, id or collection text is recorded.
#[derive(Clone, Debug, serde::Serialize)]
pub struct OutboundCall {
    pub at_ms: u64,
    pub host: String,
    pub purpose: String,
    /// The HTTP status, or `None` when no answer came (refused, timed out, failed).
    pub status: Option<u16>,
}

const LOG_CAP: usize = 500;
static LOG: std::sync::Mutex<std::collections::VecDeque<OutboundCall>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

/// The host of `url` only (`api.notion.com`), or `"?"`.
pub fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "?".into())
}

/// Record a finished or failed call. Callers pass the URL; only its host is kept.
pub fn record(url: &str, purpose: &str, status: Option<u16>) {
    let at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let call = OutboundCall {
        at_ms,
        host: host_of(url),
        purpose: purpose.into(),
        status,
    };
    if let Ok(mut log) = LOG.lock() {
        if log.len() == LOG_CAP {
            log.pop_front();
        }
        log.push_back(call);
    }
}

/// The most recent calls of this process, oldest first (at most 500).
pub fn outbound_log() -> Vec<OutboundCall> {
    LOG.lock()
        .map(|l| l.iter().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_offline_applies_to_this_thread_only_and_never_loosens() {
        // The environment of the test process is left alone throughout.
        let env_before = std::env::var_os("INVECTUS_OFFLINE");
        let base = offline();
        scoped_offline(true, || {
            assert!(offline());
            assert!(ensure_online("x").is_err());
            // An inner "online" scope cannot lift the outer restriction.
            scoped_offline(false, || assert!(offline()));
            // Another thread is not affected by this thread's scope.
            let other = std::thread::spawn(offline).join().unwrap();
            assert_eq!(other, base);
        });
        assert_eq!(offline(), base, "the scope is restored afterwards");
        // Restored even when the work panics.
        let _ = std::panic::catch_unwind(|| scoped_offline(true, || panic!("boom")));
        assert_eq!(offline(), base);
        assert_eq!(std::env::var_os("INVECTUS_OFFLINE"), env_before);
    }

    #[test]
    fn the_log_keeps_the_host_only() {
        record(
            "https://user:pw@api.example.com:8443/v1/x?token=SECRET#frag",
            "test: log",
            Some(200),
        );
        let last = outbound_log()
            .into_iter()
            .rev()
            .find(|c| c.purpose == "test: log")
            .unwrap();
        assert_eq!(last.host, "api.example.com");
        let all = serde_json::to_string(&last).unwrap();
        assert!(
            !all.contains("SECRET") && !all.contains("pw") && !all.contains("/v1"),
            "{all}"
        );
    }
}
