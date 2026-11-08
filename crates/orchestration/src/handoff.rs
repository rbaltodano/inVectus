//! The app hand-off socket (target product §8 stage 2, roadmap A7). While the desktop app runs
//! it listens on a per-user Unix domain socket (never a network port); `invectus mcp` asks it
//! which project is open and, for that project, sends tool calls to it, so the agent's work
//! runs in the app's session and job board and shows up there. The socket file is created
//! owner-only (0600) in an owner-only folder, and every connection's peer user id must match.
//! One JSON request per line, one JSON reply per line:
//!
//! - `{"op":"status"}` → `{"ok":{"project": "<root>" | null, "version": "…"}}`
//! - `{"op":"tool","client":"…","name":"…","arguments":{…}}` → `{"ok": <result>}` or
//!   `{"error": "…"}`
//! - `{"op":"activity","event":{…}}` → `{"ok":{}}`: an agent's call in another project,
//!   shown in the app's sidebar; nothing runs
//!
//! Unix only for now; elsewhere the MCP server always opens projects itself.

use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;

/// `INVECTUS_AGENT_SOCKET`, else `<OS data dir>/inVectus/agent.sock`.
pub fn socket_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("INVECTUS_AGENT_SOCKET") {
        return Ok(PathBuf::from(p));
    }
    let base = dirs::data_dir().ok_or_else(|| anyhow::anyhow!("no OS data folder"))?;
    Ok(base.join("inVectus").join("agent.sock"))
}

/// What the app does with a request (status or a tool call).
pub type Handler = std::sync::Arc<dyn Fn(Value) -> Result<Value, String> + Send + Sync>;

#[cfg(unix)]
mod imp {
    use super::*;
    use anyhow::Context;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::time::Duration;

    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn peer_is_us(stream: &UnixStream) -> bool {
        use std::os::unix::io::AsRawFd;
        let mut credentials = std::mem::MaybeUninit::<libc::ucred>::uninit();
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: the socket is live, and the buffer and length match ucred.
        let rc = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                credentials.as_mut_ptr().cast(),
                &mut len,
            )
        };
        if rc != 0 || len as usize != std::mem::size_of::<libc::ucred>() {
            return false;
        }
        // SAFETY: getsockopt succeeded and filled the complete credentials structure.
        unsafe { credentials.assume_init().uid == libc::getuid() }
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    fn peer_is_us(stream: &UnixStream) -> bool {
        use std::os::unix::io::AsRawFd;
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: a valid connected socket descriptor and two out-parameters.
        let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
        // SAFETY: getuid has no preconditions.
        rc == 0 && uid == unsafe { libc::getuid() }
    }

    /// Listen on `path` and answer requests with `handler`, one thread per connection. A stale
    /// socket left by a crashed app is replaced; one that answers (another app instance) is
    /// refused. Returns the listener's thread.
    pub fn serve(path: &Path, handler: Handler) -> Result<std::thread::JoinHandle<()>> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("securing {}", dir.display()))?;
        }
        if path.exists() {
            if UnixStream::connect(path).is_ok() {
                anyhow::bail!(
                    "another inVectus app is already listening at {}",
                    path.display()
                );
            }
            std::fs::remove_file(path)?;
        }
        let listener =
            UnixListener::bind(path).with_context(|| format!("listening at {}", path.display()))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if !peer_is_us(&stream) {
                    continue;
                }
                let handler = handler.clone();
                std::thread::spawn(move || connection(stream, handler));
            }
        }))
    }

    fn connection(stream: UnixStream, handler: Handler) {
        let Ok(mut writer) = stream.try_clone() else {
            return;
        };
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { return };
            if line.trim().is_empty() {
                continue;
            }
            let reply = match serde_json::from_str::<Value>(&line) {
                Ok(req) => match handler(req) {
                    Ok(v) => serde_json::json!({ "ok": v }),
                    Err(e) => serde_json::json!({ "error": e }),
                },
                Err(e) => serde_json::json!({ "error": format!("bad request: {e}") }),
            };
            if writeln!(writer, "{reply}")
                .and_then(|_| writer.flush())
                .is_err()
            {
                return;
            }
        }
    }

    /// A connection to the running app.
    pub struct Client {
        reader: BufReader<UnixStream>,
        writer: UnixStream,
    }

    impl Client {
        /// `None` when no app is listening.
        pub fn connect(path: &Path) -> Option<Self> {
            let stream = UnixStream::connect(path).ok()?;
            stream.set_read_timeout(None).ok()?;
            stream
                .set_write_timeout(Some(Duration::from_secs(10)))
                .ok()?;
            Some(Self {
                writer: stream.try_clone().ok()?,
                reader: BufReader::new(stream),
            })
        }

        pub fn request(&mut self, req: &Value) -> Result<Result<Value, String>> {
            writeln!(self.writer, "{req}")?;
            self.writer.flush()?;
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                anyhow::bail!("the inVectus app closed the connection");
            }
            let reply: Value = serde_json::from_str(&line)?;
            Ok(match reply.get("ok") {
                Some(v) => Ok(v.clone()),
                None => Err(reply["error"].as_str().unwrap_or("app error").to_string()),
            })
        }

        /// The project the app has open, if any.
        pub fn open_project(&mut self) -> Option<PathBuf> {
            self.request(&serde_json::json!({ "op": "status" }))
                .ok()?
                .ok()?
                .get("project")?
                .as_str()
                .map(PathBuf::from)
        }
    }
}

#[cfg(unix)]
pub use imp::{Client, serve};

#[cfg(not(unix))]
pub struct Client;

#[cfg(not(unix))]
impl Client {
    pub fn connect(_: &std::path::Path) -> Option<Self> {
        None
    }
    pub fn request(&mut self, _: &Value) -> Result<Result<Value, String>> {
        anyhow::bail!("the app hand-off is not available on this platform")
    }
    pub fn open_project(&mut self) -> Option<PathBuf> {
        None
    }
}

#[cfg(not(unix))]
pub fn serve(_: &std::path::Path, _: Handler) -> Result<std::thread::JoinHandle<()>> {
    anyhow::bail!("the app hand-off is not available on this platform")
}

/// The app's request handler: `status` reports its open project; `tool` runs the tool in the
/// app's session (pinned to that project) and passes each call to `activity`.
pub fn app_handler(
    session: crate::Shared,
    board: crate::SharedBoard,
    emit: crate::Emit,
    activity: std::sync::Arc<dyn Fn(&Value) + Send + Sync>,
) -> Handler {
    std::sync::Arc::new(move |req: Value| {
        let root = || {
            session
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(|s| s.project.root.clone()))
        };
        match req["op"].as_str() {
            // An agent working in another project, outside this app: recorded for the sidebar,
            // nothing is run. Only the fields the activity display uses are passed on.
            Some("activity") => {
                let e = &req["event"];
                activity(&serde_json::json!({
                    "client": e["client"].as_str(),
                    "tool": e["tool"].as_str().unwrap_or_default(),
                    "project": e["project"].as_str().unwrap_or_default(),
                    "query": e["query"].as_str(),
                    "gap_id": e["gap_id"].as_i64(),
                    "ok": e["ok"].as_bool().unwrap_or(true),
                    "error": e["error"].as_str(),
                    "at_ms": crate::jobs::now_ms(),
                }));
                Ok(serde_json::json!({}))
            }
            Some("status") => Ok(serde_json::json!({
                "project": root().map(|p| p.display().to_string()),
                "version": invectus_core::VERSION,
            })),
            Some("tool") => {
                let _transition = session.transition.lock().map_err(crate::err)?;
                let Some(pinned) = root() else {
                    return Err("no project is open in the inVectus app".into());
                };
                if let Some(expected) = req["project"].as_str()
                    && pinned != std::path::Path::new(expected)
                {
                    return Err(
                        "the app switched projects; explicitly open the intended project again"
                            .into(),
                    );
                }
                let name = req["name"].as_str().unwrap_or_default().to_string();
                let mut agent = crate::agent::Agent {
                    session: session.clone(),
                    board: board.clone(),
                    emit: emit.clone(),
                    client: req["client"].as_str().map(String::from),
                    pinned: Some(pinned.clone()),
                };
                let args = req.get("arguments").cloned().unwrap_or(Value::Null);
                static NEXT_CALL: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(1);
                let call_id = NEXT_CALL
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    .to_string();
                activity(&serde_json::json!({
                    "call_id": call_id, "phase": "started",
                    "client": agent.client, "tool": name, "project": pinned.display().to_string(),
                    "query": args.get("query").or_else(|| args.get("idea")), "gap_id": args.get("gap_id"),
                    "ok": true, "error": null, "at_ms": crate::jobs::now_ms(),
                }));
                let result = agent.run_tool(&name, args.clone());
                activity(&serde_json::json!({
                    "call_id": call_id, "phase": "finished",
                    "client": agent.client,
                    "tool": name,
                    "project": pinned.display().to_string(),
                    "query": args.get("query").or_else(|| args.get("idea")),
                    "gap_id": args.get("gap_id"),
                    "ok": result.is_ok(),
                    "error": result.as_ref().err(),
                    "at_ms": crate::jobs::now_ms(),
                }));
                result
            }
            _ => Err("unknown request".into()),
        }
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_call_blocks_switching_and_stale_project_requests_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let session = crate::Shared::default();
        let board = crate::SharedBoard::default();
        let a = crate::open_project(
            &session,
            &board,
            dir.path().join("a").to_str().unwrap(),
            true,
        )
        .unwrap()
        .root;
        let other = dir.path().join("b");
        let s2 = session.clone();
        let b2 = board.clone();
        let target = other.clone();
        let handler = app_handler(
            session.clone(),
            board.clone(),
            crate::agent::log_emitter(),
            Arc::new(move |event| {
                if event["phase"] == "started" {
                    let error = crate::open_project(&s2, &b2, target.to_str().unwrap(), true)
                        .err()
                        .unwrap();
                    assert!(error.contains("agent"), "{error}");
                }
            }),
        );
        handler(
            serde_json::json!({"op":"tool", "name":"project_status", "project":a, "arguments":{}}),
        )
        .unwrap();
        let b = crate::open_project(&session, &board, other.to_str().unwrap(), true)
            .unwrap()
            .root;
        let (job, token) = board
            .lock()
            .unwrap()
            .begin(crate::JobInfo::new(
                "index",
                Some(b.clone()),
                "replacement".into(),
            ))
            .unwrap();
        let error = handler(serde_json::json!({"op":"tool", "name":"cancel_job", "project":a, "arguments":{"job_id":job.id}})).unwrap_err();
        assert!(error.contains("switched projects"));
        assert!(!token.is_cancelled());
        let form: crate::TableForm =
            serde_json::from_value(serde_json::json!({"project":a,"collection":"papers"})).unwrap();
        assert!(crate::with_session_wait(&session, |s| form.ensure_project(&s.project)).is_err());
    }

    #[test]
    fn app_answers_status_and_pinned_tools_over_a_private_socket() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("run/agent.sock");
        let session = crate::Shared::default();
        let board = crate::SharedBoard::default();
        crate::open_project(
            &session,
            &board,
            dir.path().join("a").to_str().unwrap(),
            true,
        )
        .unwrap();
        invectus_core::Project::create(&dir.path().join("b")).unwrap();
        let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
        let s2 = seen.clone();
        let handler = app_handler(
            session.clone(),
            board.clone(),
            crate::agent::log_emitter(),
            Arc::new(move |e: &Value| s2.lock().unwrap().push(e.clone())),
        );
        // A stale socket file (no listener) is replaced.
        std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
        std::fs::write(&sock, "").unwrap();
        serve(&sock, handler.clone()).unwrap();
        let mode = std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = std::fs::metadata(sock.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert!(serve(&sock, handler).is_err(), "a second app is refused");

        let mut c = Client::connect(&sock).unwrap();
        let root = c.open_project().unwrap();
        assert!(root.ends_with("a.invectus"));
        let r = c
            .request(&serde_json::json!({
                "op": "tool", "client": "test-client", "name": "project_status", "arguments": {}
            }))
            .unwrap()
            .unwrap();
        assert!(r["root"].as_str().unwrap().ends_with("a.invectus"));
        // Pinned: the app never switches projects for an agent.
        let other = dir.path().join("b").display().to_string();
        let e = c
            .request(&serde_json::json!({
                "op": "tool", "name": "open_project", "arguments": { "project": other }
            }))
            .unwrap()
            .unwrap_err();
        assert!(e.contains("app has"), "{e}");
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
        let events = seen.lock().unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0]["client"], "test-client");
        assert_eq!(events[0]["tool"], "project_status");
        assert_eq!(events[0]["phase"], "started");
        assert_eq!(events[1]["phase"], "finished");
        assert_eq!(events[0]["call_id"], events[1]["call_id"]);
        assert_eq!(events[3]["ok"], false);
    }
}
