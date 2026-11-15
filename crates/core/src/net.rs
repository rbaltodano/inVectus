//! Network gate. Import, embedding, search, regions and gap scans never call the network.
//! Model downloads, pulls, listings and outside-source checks call [`ensure_online`]. GitHub
//! starring and client sign-in probes check [`offline`]. All of them refuse the work when
//! offline mode is in force (SPEC §7.7), and all of them are logged ([`record`],
//! [`record_command`]); the app's log includes the CLI and agent processes ([`share_log`]).
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
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OutboundCall {
    pub at_ms: u64,
    pub host: String,
    pub purpose: String,
    /// The HTTP status, or `None` when no answer came (refused, timed out, failed) or the call
    /// was made by another program whose answer inVectus cannot see (`via: "command"`).
    pub status: Option<u16>,
    /// Which inVectus process made it: `app`, `cli` or `agent` (the MCP server).
    #[serde(default)]
    pub process: String,
    /// `http` for inVectus's own requests; `command` when another program on this computer
    /// (the `claude`, `codex`, `grok` or `gh` command) was asked and may have used the network.
    #[serde(default = "http")]
    pub via: String,
}

fn http() -> String {
    "http".into()
}

const LOG_CAP: usize = 500;
/// The shared file is cut back to [`LOG_CAP`] entries once it passes this size.
const SHARED_MAX_BYTES: u64 = 256 * 1024;
const SHARED_FILE: &str = "network-log.jsonl";

/// This process's calls; `true` once the call is also in the shared file.
static LOG: std::sync::Mutex<std::collections::VecDeque<(OutboundCall, bool)>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

struct Shared {
    file: std::path::PathBuf,
    process: &'static str,
    since_ms: u64,
}
static SHARED: std::sync::OnceLock<Shared> = std::sync::OnceLock::new();

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Share this process's calls with the other inVectus processes on this computer, so the app's
/// Network activity also lists what the CLI and agent (MCP) processes did. Called once at
/// start by the app (`"app"`), the CLI (`"cli"`) and `invectus mcp` (`"agent"`). The shared
/// file sits in the owner-only settings folder ([`crate::paths::app_data_dir`]), is owner-only
/// itself, holds the same host/purpose/status fields as the in-memory log, and keeps about the
/// last 500 calls. Library code and tests never call this, so they never touch the file.
pub fn share_log(process: &'static str) {
    let Some(dir) = crate::paths::app_data_dir() else {
        return;
    };
    let _ = SHARED.set(Shared {
        file: dir.join(SHARED_FILE),
        process,
        since_ms: now_ms(),
    });
}

/// The host of `url` only (`api.notion.com`), or `"?"`.
pub fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "?".into())
}

/// Record a finished or failed call. Callers pass the URL; only its host is kept.
pub fn record(url: &str, purpose: &str, status: Option<u16>) {
    push(host_of(url), purpose, status, "http");
}

/// Record a call made by another program that inVectus started (`gh`, `claude auth status`),
/// which may have used the network. `host` is the service when known (`api.github.com`), else
/// the program's name; `status` is an HTTP status only when the program reported one.
pub fn record_command(host: &str, purpose: &str, status: Option<u16>) {
    push(host.to_string(), purpose, status, "command");
}

fn push(host: String, purpose: &str, status: Option<u16>, via: &str) {
    let shared = SHARED.get();
    let call = OutboundCall {
        at_ms: now_ms(),
        host,
        purpose: purpose.into(),
        status,
        process: shared.map_or("", |s| s.process).into(),
        via: via.into(),
    };
    let persisted = shared.is_some_and(|s| append_shared(&s.file, &call).is_ok());
    if let Ok(mut log) = LOG.lock() {
        if log.len() == LOG_CAP {
            log.pop_front();
        }
        log.push_back((call, persisted));
    }
}

/// Append one line. A single `O_APPEND` write per call keeps lines from different processes
/// whole. A link in place of the file is refused rather than followed.
fn append_shared(file: &std::path::Path, call: &OutboundCall) -> Result<()> {
    use std::io::Write;
    let dir = file.parent().ok_or_else(|| anyhow::anyhow!("no folder"))?;
    crate::private::create_dir_all(dir)?;
    match std::fs::symlink_metadata(file) {
        Ok(m) if !m.file_type().is_file() => bail!("{} is not a plain file", file.display()),
        Ok(m) if m.len() > SHARED_MAX_BYTES => trim_shared(file)?,
        _ => {}
    }
    let mut line = serde_json::to_vec(call)?;
    line.push(b'\n');
    let mut o = std::fs::OpenOptions::new();
    o.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(file)?;
    crate::private::restrict(file)?;
    f.write_all(&line)?;
    Ok(())
}

/// Keep the newest [`LOG_CAP`] lines, replacing the file atomically (owner-only). A line
/// another process appends during the swap can be lost; the log is a convenience record.
fn trim_shared(file: &std::path::Path) -> Result<()> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let keep = &lines[lines.len().saturating_sub(LOG_CAP)..];
    let mut out = keep.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    crate::private::replace(file, out.as_bytes())
}

fn read_shared(file: &std::path::Path, since_ms: u64) -> Vec<OutboundCall> {
    match std::fs::symlink_metadata(file) {
        Ok(m) if m.file_type().is_file() => {}
        _ => return Vec::new(),
    }
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<OutboundCall>(l).ok())
        .filter(|c| c.at_ms >= since_ms)
        .collect()
}

/// The most recent calls, oldest first (at most 500): this process's, plus, once
/// [`share_log`] was called, every inVectus process's since this one started.
pub fn outbound_log() -> Vec<OutboundCall> {
    let mine: Vec<(OutboundCall, bool)> = LOG
        .lock()
        .map(|l| l.iter().cloned().collect())
        .unwrap_or_default();
    let Some(shared) = SHARED.get() else {
        return mine.into_iter().map(|(c, _)| c).collect();
    };
    let mut all = read_shared(&shared.file, shared.since_ms);
    all.extend(mine.into_iter().filter(|(_, p)| !p).map(|(c, _)| c));
    all.sort_by_key(|c| c.at_ms);
    let cut = all.len().saturating_sub(LOG_CAP);
    all.split_off(cut)
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

    fn call(at_ms: u64, host: &str, process: &str) -> OutboundCall {
        OutboundCall {
            at_ms,
            host: host.into(),
            purpose: "test: shared".into(),
            status: Some(200),
            process: process.into(),
            via: "http".into(),
        }
    }

    #[test]
    fn the_shared_file_merges_processes_and_is_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings").join(SHARED_FILE);
        append_shared(&file, &call(10, "old.example.com", "cli")).unwrap();
        append_shared(&file, &call(20, "api.openalex.org", "agent")).unwrap();
        append_shared(&file, &call(30, "api.notion.com", "app")).unwrap();
        // Calls from before this process started are not shown.
        let seen = read_shared(&file, 20);
        let hosts: Vec<_> = seen
            .iter()
            .map(|c| (c.host.as_str(), c.process.as_str()))
            .collect();
        assert_eq!(
            hosts,
            [("api.openalex.org", "agent"), ("api.notion.com", "app")]
        );
        // Lines written by an older version (no `process`/`via`) still read.
        std::fs::write(
            &file,
            r#"{"at_ms":40,"host":"h.example.com","purpose":"p","status":null}"#.to_owned() + "\n",
        )
        .unwrap();
        let old = read_shared(&file, 0);
        assert_eq!((old[0].via.as_str(), old[0].process.as_str()), ("http", ""));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode =
                |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            // A file loosened by someone else is tightened on the next write.
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            append_shared(&file, &call(50, "x.example.com", "cli")).unwrap();
            assert_eq!(mode(&file), 0o600);
            assert_eq!(mode(file.parent().unwrap()), 0o700);
        }
    }

    #[test]
    fn the_shared_file_stays_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(SHARED_FILE);
        let mut i = 0u64;
        while std::fs::metadata(&file).map_or(0, |m| m.len()) <= SHARED_MAX_BYTES {
            append_shared(&file, &call(i, "api.example.com", "cli")).unwrap();
            i += 1;
        }
        append_shared(&file, &call(i, "last.example.com", "cli")).unwrap();
        let kept = read_shared(&file, 0);
        assert_eq!(kept.len(), LOG_CAP + 1);
        assert_eq!(kept.last().unwrap().host, "last.example.com");
        assert_eq!(kept[0].at_ms, i - LOG_CAP as u64);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_the_shared_file_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere.txt");
        std::fs::write(&elsewhere, "keep\n").unwrap();
        let file = dir.path().join(SHARED_FILE);
        std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
        assert!(append_shared(&file, &call(1, "api.example.com", "cli")).is_err());
        assert!(read_shared(&file, 0).is_empty());
        assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "keep\n");
    }
}
