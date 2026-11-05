//! `invectus mcp`: a Model Context Protocol server over stdio (target product §5). One
//! JSON-RPC message per line on stdin, one reply per line on stdout; logs go to stderr. It
//! opens no network port. The tools live in `invectus_orchestration::agent`; this file speaks
//! the protocol and routes calls: when the desktop app is running with the requested project
//! open, calls go to the app over its private hand-off socket (§8 stage 2) and run in its
//! session, so its jobs, decisions and suggestions show there; otherwise the server opens the
//! project itself.

use anyhow::{Context, Result};
use invectus_orchestration::agent::{Agent, tools};
use invectus_orchestration::handoff;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// Protocol revisions this server speaks; the newest is offered when the client's is unknown.
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

pub fn serve() -> Result<()> {
    let mut server = Server {
        socket: handoff::socket_path().ok(),
        ..Server::default()
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    eprintln!("invectus mcp {}: ready on stdio", invectus_core::VERSION);
    for line in stdin.lock().lines() {
        let line = line.context("reading stdin")?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_line(&line) {
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    server.agent.stop_jobs();
    Ok(())
}

#[derive(Default)]
pub struct Server {
    agent: Agent,
    /// Where the app listens; `None` disables the hand-off (tests, other platforms).
    socket: Option<PathBuf>,
    app: Option<handoff::Client>,
    /// The last project-bound call went to the app (so job_status/cancel_job follow it).
    via_app: bool,
}

/// The same project folder, with or without the `.invectus` suffix.
fn same_project(a: &Path, b: &Path) -> bool {
    let canon = |x: &Path| std::fs::canonicalize(x).ok();
    let a = canon(a).or_else(|| canon(&a.with_extension("invectus")));
    let b = canon(b).or_else(|| canon(&b.with_extension("invectus")));
    a.is_some() && a == b
}

impl Server {
    /// The reply to one line, or `None` for notifications.
    pub fn handle_line(&mut self, line: &str) -> Option<Value> {
        match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> =
                    batch.into_iter().filter_map(|m| self.handle(m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(message) => self.handle(message),
            Err(e) => Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
        }
    }

    fn handle(&mut self, message: Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        // Notifications (no id) get no reply; replies from the client are ignored too.
        let id = id?;
        let result = match method.as_str() {
            "initialize" => {
                self.agent.client = params["clientInfo"]["name"]
                    .as_str()
                    .map(|n| n.trim().to_string())
                    .filter(|n| !n.is_empty());
                Ok(initialize(&params))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => Ok(self.call(&params)),
            "" => Err((-32600, "invalid request: no method".to_string())),
            other => Err((-32601, format!("method not found: {other}"))),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, msg)) => error(id, code, &msg),
        })
    }

    /// A tool call; tool failures are results with `isError`, so the agent can read and retry.
    fn call(&mut self, params: &Value) -> Value {
        let name = params["name"].as_str().unwrap_or_default();
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        match self.route(name, args) {
            Ok(v) => json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }],
                "structuredContent": v,
                "isError": false,
            }),
            Err(e) => json!({
                "content": [{ "type": "text", "text": e }],
                "isError": true,
            }),
        }
    }

    /// Run a tool here or in the app.
    fn route(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let project_bound = !matches!(name, "list_projects" | "inspect_source");
        let follows_jobs = matches!(name, "job_status" | "cancel_job");
        if project_bound && let Some(app_root) = self.app_project() {
            let wanted = args["project"]
                .as_str()
                .map(invectus_orchestration::user_path);
            let local = self.agent.open_root();
            let to_app = if follows_jobs {
                self.via_app
            } else {
                match (&wanted, &local) {
                    (Some(w), _) => same_project(w, &app_root),
                    // No project named: the one this agent works in, else the app's.
                    (None, Some(l)) => same_project(l, &app_root),
                    (None, None) => true,
                }
            };
            if to_app {
                self.via_app = true;
                return self.forward(name, args);
            }
        }
        if project_bound && !follows_jobs {
            self.via_app = false;
        }
        let (query, gap_id) = (
            args.get("query").or_else(|| args.get("idea")).cloned(),
            args.get("gap_id").cloned(),
        );
        let result = self.agent.run_tool(name, args);
        if project_bound && !follows_jobs {
            self.notify_app(name, query, gap_id, &result);
        }
        result
    }

    /// The project the running app has open, reconnecting once if the app restarted.
    fn app_project(&mut self) -> Option<PathBuf> {
        let socket = self.socket.clone()?;
        for _ in 0..2 {
            if self.app.is_none() {
                self.app = handoff::Client::connect(&socket);
            }
            match self.app.as_mut()?.open_project() {
                Some(p) => return Some(p),
                None => self.app = None,
            }
        }
        None
    }

    /// Tell a running app about a call made here (another project than the app's), so its
    /// sidebar shows which agent works where. Best effort; never affects the call.
    fn notify_app(
        &mut self,
        name: &str,
        query: Option<Value>,
        gap_id: Option<Value>,
        result: &Result<Value, String>,
    ) {
        let Some(project) = self.agent.open_root() else {
            return;
        };
        if self.app_project().is_none() {
            return;
        }
        let event = json!({
            "op": "activity",
            "event": {
                "client": self.agent.client, "tool": name, "project": project.display().to_string(),
                "query": query, "gap_id": gap_id, "ok": result.is_ok(), "error": result.as_ref().err(),
            },
        });
        if let Some(app) = self.app.as_mut()
            && app.request(&event).is_err()
        {
            self.app = None;
        }
    }

    fn forward(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let app = self
            .app
            .as_mut()
            .ok_or("the inVectus app is not reachable")?;
        let req = json!({
            "op": "tool", "client": self.agent.client, "name": name, "arguments": args,
        });
        match app.request(&req) {
            Ok(r) => r,
            Err(e) => {
                self.app = None;
                Err(format!("lost the connection to the inVectus app: {e:#}"))
            }
        }
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn initialize(params: &Value) -> Value {
    let asked = params["protocolVersion"].as_str().unwrap_or_default();
    let version = PROTOCOLS
        .iter()
        .find(|v| **v == asked)
        .unwrap_or(&PROTOCOLS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "invectus", "version": invectus_core::VERSION },
        "instructions": "inVectus answers whether an idea already exists in a local collection \
            and where its gaps are. Open a project (list_projects, open_project), read \
            project_status for collections and readiness, then use check_idea for a calibrated \
            verdict (exists / near / open) or search for nearest items. Verdicts hold for this \
            collection only. You can also write for the collection: gap descriptions \
            (gap_context, submit_description), calibration paraphrases (paraphrase_sample, \
            submit_paraphrases) and region names (region_context, submit_region_names); the \
            engine checks your writing and records you as its author. Suggest decisions with \
            suggest_decision; the person makes them. Everything runs offline on this computer.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use invectus_orchestration as orch;

    fn call(s: &mut Server, id: i64, method: &str, params: Value) -> Value {
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        s.handle_line(&line.to_string()).unwrap()
    }

    #[test]
    fn handshake_tools_and_errors() {
        let mut s = Server::default();
        let r = call(
            &mut s,
            1,
            "initialize",
            json!({ "protocolVersion": "2025-03-26" }),
        );
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        let r = call(
            &mut s,
            2,
            "initialize",
            json!({ "protocolVersion": "1999-01-01" }),
        );
        assert_eq!(r["result"]["protocolVersion"], PROTOCOLS[0]);
        assert!(
            s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .is_none()
        );
        let r = call(&mut s, 3, "tools/list", json!({}));
        let names: Vec<&str> = r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "list_projects",
                "open_project",
                "project_status",
                "search",
                "check_idea",
                "list_gaps",
                "gap_detail",
                "inspect_source",
                "import_collection",
                "job_status",
                "cancel_job",
                "scan_gaps",
                "verify_gap",
                "suggest_decision",
                "gap_context",
                "submit_description",
                "paraphrase_sample",
                "submit_paraphrases",
                "region_context",
                "submit_region_names",
                "export_gaps"
            ]
        );
        assert_eq!(call(&mut s, 4, "nope", json!({}))["error"]["code"], -32601);
        assert_eq!(s.handle_line("{not json").unwrap()["error"]["code"], -32700);
        // Tool failures are results the agent can read.
        let r = call(
            &mut s,
            5,
            "tools/call",
            json!({ "name": "project_status", "arguments": {} }),
        );
        assert_eq!(r["result"]["isError"], true);
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("no project is open")
        );
        let r = call(
            &mut s,
            6,
            "tools/call",
            json!({ "name": "search", "arguments": { "collection": "c" , "bogus": 1 } }),
        );
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("invalid arguments")
        );
    }

    #[test]
    fn lists_and_opens_projects() {
        let dir = tempfile::tempdir().unwrap();
        invectus_core::Project::create(&dir.path().join("b")).unwrap();
        invectus_core::Project::create(&dir.path().join("a")).unwrap();
        std::fs::create_dir(dir.path().join("not-a-project")).unwrap();
        let mut s = Server::default();
        let folder = dir.path().display().to_string();
        let r = call(
            &mut s,
            1,
            "tools/call",
            json!({ "name": "list_projects", "arguments": { "folder": folder } }),
        );
        let projects = &r["result"]["structuredContent"]["projects"];
        assert_eq!(projects.as_array().unwrap().len(), 2);
        assert_eq!(projects[0]["name"], "a");
        assert_eq!(projects[0]["open"], false);
        let a = projects[0]["root"].as_str().unwrap().to_string();
        let r = call(
            &mut s,
            2,
            "tools/call",
            json!({ "name": "project_status", "arguments": { "project": a } }),
        );
        assert_eq!(r["result"]["isError"], false, "{r}");
        assert_eq!(r["result"]["structuredContent"]["name"], "a");
        let r = call(
            &mut s,
            3,
            "tools/call",
            json!({ "name": "list_projects", "arguments": { "folder": folder } }),
        );
        assert_eq!(
            r["result"]["structuredContent"]["projects"][0]["open"],
            true
        );
        // Without a path, the open project answers.
        let r = call(
            &mut s,
            4,
            "tools/call",
            json!({ "name": "project_status", "arguments": {} }),
        );
        assert_eq!(r["result"]["structuredContent"]["collections"], json!([]));
    }

    /// The A2 goal: an agent asks whether an idea exists and gets a verdict with its quality
    /// and evidence. Import → index → map → calibrate through the shared pipeline, then
    /// `check_idea` over the server. Needs the dev BGE model (INVECTUS_MODELS_DIR).
    #[test]
    fn check_idea_answers_with_verdict_quality_and_evidence() {
        use invectus_embed::{ModelStore, registry};
        let installed = ModelStore::default_root().ok().is_some_and(|r| {
            ModelStore::new(r).is_installed(registry::spec(registry::DEFAULT_MODEL).unwrap())
        });
        if !installed {
            eprintln!("skipped: {} is not installed", registry::DEFAULT_MODEL);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("notes.csv");
        let topics = [
            "retinal detachment surgery outcomes",
            "glaucoma intraocular pressure medication",
            "myopia progression in children",
            "corneal transplant rejection",
        ];
        let mut rows = String::from("id,title,text\n");
        for i in 0..240 {
            let t = topics[i % 4];
            rows.push_str(&format!(
                "{i},Study {i} on {t},\"Patients in cohort {i} were followed for {t}. We \
                 report baseline characteristics, the intervention schedule, adverse events and \
                 outcomes at one and two years for case series {i}, with {} participants.\"\n",
                40 + i
            ));
        }
        std::fs::write(&csv, rows).unwrap();
        let mut s = Server::default();
        let root = dir.path().join("p");
        orch::open_project(
            &s.agent.session,
            &s.agent.board,
            root.to_str().unwrap(),
            true,
        )
        .unwrap();
        let form: orch::ImportForm = serde_json::from_value(json!({
            "collection": "notes", "source_path": csv, "text": ["text"], "title": "title",
            "id": "id", "date": null, "url": null, "role": "neutral", "role_column": null,
            "role_rules": null, "chunk_chars": 2000, "chunk_overlap": 200, "limit": null
        }))
        .unwrap();
        let req = orch::request_for_session(&s.agent.session, form).unwrap();
        let emit: orch::Emit = std::sync::Arc::new(|_: &orch::JobInfo| {});
        let out = orch::import_then_index(&emit, &s.agent.session, &s.agent.board, req).unwrap();
        assert!(out.job.is_some(), "{:?}", out.index_error);
        let start = std::time::Instant::now();
        while s.agent.board.lock().unwrap().running().is_some() {
            assert!(start.elapsed().as_secs() < 300, "job did not finish");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let r = call(
            &mut s,
            1,
            "tools/call",
            json!({ "name": "check_idea", "arguments": {
                "collection": "notes", "idea": "glaucoma eye pressure drugs", "k": 5
            } }),
        );
        assert_eq!(r["result"]["isError"], false, "{r}");
        let v = &r["result"]["structuredContent"];
        assert!(
            ["exists", "near", "open"].contains(&v["verdict"].as_str().unwrap()),
            "{v}"
        );
        assert!(v["p_exists"].is_number());
        assert!(!v["calibration"]["quality"].as_str().unwrap().is_empty());
        assert_eq!(v["search"]["exhaustive"], true);
        let top = v["search"]["hits"][0]["item"]["title"].as_str().unwrap();
        assert!(top.contains("glaucoma"), "{top}");
        assert!(
            v["scope"]
                .as_str()
                .unwrap()
                .contains("this collection only")
        );

        let r = call(
            &mut s,
            2,
            "tools/call",
            json!({ "name": "list_gaps", "arguments": { "collection": "notes", "limit": 3 } }),
        );
        assert_eq!(r["result"]["isError"], false, "{r}");
        let gaps = r["result"]["structuredContent"]["gaps"].as_array().unwrap();
        assert!(gaps.len() <= 3);
        let id = gaps
            .first()
            .and_then(|g| g["gap_id"].as_i64())
            .expect("gaps found");
        {
            let r = call(
                &mut s,
                3,
                "tools/call",
                json!({ "name": "gap_detail", "arguments": { "collection": "notes", "gap_id": id } }),
            );
            assert_eq!(r["result"]["isError"], false, "{r}");
        }
    }
}
