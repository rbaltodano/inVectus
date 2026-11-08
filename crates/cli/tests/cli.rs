//! End-to-end CLI tests. The model-free flow always runs; the embed/search flow needs the pinned
//! BGE files (`INVECTUS_TEST_MODEL_SRC`, default `<repo>/.spike-data/models`) and skips otherwise.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cli(models: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_invectus"))
        .args(args)
        .env("INVECTUS_MODELS_DIR", models)
        .env_remove("INVECTUS_PROJECT")
        .output()
        .unwrap()
}

fn ok(o: Output) -> String {
    assert!(
        o.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap()
}

fn fail(o: Output) -> String {
    assert!(!o.status.success());
    String::from_utf8(o.stderr).unwrap()
}

fn setup() -> (tempfile::TempDir, PathBuf, String) {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("items.csv");
    std::fs::write(
        &csv,
        "id,title,body,kind\n\
         a,Orthokeratology for myopia,Overnight lenses slow axial elongation in children.,study\n\
         b,Glaucoma surgery outcomes,Trabeculectomy lowers intraocular pressure.,study\n\
         c,Review of myopia control,A systematic review of atropine and ortho-k.,review\n",
    )
    .unwrap();
    let models = dir.path().join("models");
    let project = dir.path().join("p.invectus").display().to_string();
    let o = ok(cli(
        &models,
        &["init", dir.path().join("p").to_str().unwrap()],
    ));
    assert!(o.contains("created"));
    let o = ok(cli(
        &models,
        &[
            "--offline",
            "import",
            "-p",
            &project,
            csv.to_str().unwrap(),
            "-c",
            "items",
            "--id",
            "id",
            "--title",
            "title",
            "--text",
            "body",
            "--chunk-chars",
            "2000",
            "--role-column",
            "kind",
            "--role-rules",
            "review=coverage,study=demand",
        ],
    ));
    assert!(o.contains("3 items (+3 new"), "{o}");
    (dir, models, project)
}

#[test]
fn import_status_and_offline_guards_without_models() {
    let (dir, models, project) = setup();
    let status: serde_json::Value =
        serde_json::from_str(&ok(cli(&models, &["status", "-p", &project, "--json"]))).unwrap();
    let c = &status["collections"][0];
    assert_eq!(c["items"], 3);
    assert_eq!(c["by_role"]["coverage"], 1);
    assert_eq!(status["config"]["network_enabled"], false);

    let e = fail(cli(
        &models,
        &[
            "--offline",
            "search",
            "-p",
            &project,
            "-c",
            "items",
            "myopia",
        ],
    ));
    assert!(e.contains("no vectors"), "{e}");
    let e = fail(cli(
        &models,
        &["--offline", "embed", "-p", &project, "-c", "items"],
    ));
    assert!(e.contains("not installed"), "{e}");
    let e = fail(cli(
        &models,
        &["--offline", "models", "download", "bge-small-en-v1.5"],
    ));
    assert!(e.contains("offline"), "{e}");
    let e = fail(cli(
        &models,
        &["--offline", "embed", "-p", &project, "-c", "nope"],
    ));
    assert!(e.contains("no collection"), "{e}");
    let e = fail(cli(
        &models,
        &[
            "--offline",
            "check",
            "-p",
            &project,
            "-c",
            "items",
            "myopia",
        ],
    ));
    assert!(e.contains("no vectors"), "{e}");
    let e = fail(cli(
        &models,
        &[
            "--offline",
            "import",
            "-p",
            &project,
            dir.path().join("items.csv").to_str().unwrap(),
            "-c",
            "tok",
            "--text",
            "body",
            "--chunk-tokenizer",
            "bge-small-en-v1.5",
        ],
    ));
    assert!(e.contains("not installed"), "{e}");
}

#[test]
fn embed_and_search_with_the_pinned_bge_model() {
    let src = std::env::var_os("INVECTUS_TEST_MODEL_SRC")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.spike-data/models")
        })
        .join("bge");
    if !src.join("model.onnx").is_file() {
        eprintln!("SKIP: no BGE model files at {}", src.display());
        return;
    }
    let (dir, models, project) = setup();
    ok(cli(
        &models,
        &[
            "models",
            "install",
            "bge-small-en-v1.5",
            "--from",
            src.to_str().unwrap(),
        ],
    ));
    let o = ok(cli(
        &models,
        &[
            "--offline",
            "embed",
            "-p",
            &project,
            "-c",
            "items",
            "--threads",
            "2",
        ],
    ));
    assert!(o.contains("3 embedded"), "{o}");
    let o = ok(cli(
        &models,
        &["--offline", "embed", "-p", &project, "-c", "items"],
    ));
    assert!(o.contains("3 already embedded, 0 embedded"), "{o}");

    let r: serde_json::Value = serde_json::from_str(&ok(cli(
        &models,
        &[
            "--offline",
            "search",
            "-p",
            &project,
            "-c",
            "items",
            "lenses to slow myopia in kids",
            "--json",
        ],
    )))
    .unwrap();
    assert_eq!(r["exhaustive"], true);
    assert_eq!(r["compatibility"], "same space");
    assert_eq!(r["hits"][0]["item"]["source_id"], "a");
    let r: serde_json::Value = serde_json::from_str(&ok(cli(
        &models,
        &[
            "--offline",
            "search",
            "-p",
            &project,
            "-c",
            "items",
            "lenses to slow myopia in kids",
            "--role",
            "coverage",
            "--json",
        ],
    )))
    .unwrap();
    assert_eq!(r["hits"].as_array().unwrap().len(), 1);
    assert_eq!(r["hits"][0]["item"]["source_id"], "c");
    let e = fail(cli(
        &models,
        &[
            "search",
            "-p",
            &project,
            "-c",
            "items",
            "x",
            "--nprobes",
            "4",
        ],
    ));
    assert!(e.contains("no ANN index"), "{e}");

    // A verdict needs a calibration, and three items are too few to fit one.
    let e = fail(cli(
        &models,
        &[
            "--offline",
            "check",
            "-p",
            &project,
            "-c",
            "items",
            "myopia",
        ],
    ));
    assert!(e.contains("invectus calibrate"), "{e}");
    let e = fail(cli(
        &models,
        &["--offline", "calibrate", "-p", &project, "-c", "items"],
    ));
    assert!(e.contains("30 or more"), "{e}");

    // New collections default to token chunks without an explicit tokenizer flag.
    let long = dir.path().join("long.csv");
    let body = "Orthokeratology lenses reshape the cornea overnight to slow axial elongation. "
        .repeat(120);
    std::fs::write(&long, format!("id,title,body\nL1,Long report,\"{body}\"\n")).unwrap();
    let o = ok(cli(
        &models,
        &[
            "--offline",
            "import",
            "-p",
            &project,
            long.to_str().unwrap(),
            "-c",
            "tok",
            "--id",
            "id",
            "--title",
            "title",
            "--text",
            "body",
            "--json",
        ],
    ));
    let r: serde_json::Value = serde_json::from_str(&o).unwrap();
    assert_eq!(r["items_chunked"], 1);
    {
        let p = invectus_core::Project::open(std::path::Path::new(&project)).unwrap();
        let policy = p.store.require_collection("tok").unwrap().chunking;
        assert_eq!(policy["tokens"]["model_key"], "bge-small-en-v1.5");
        assert_eq!(policy["tokens"]["max_tokens"], 448);
        assert_eq!(policy["tokens"]["overlap_tokens"], 48);
    }
    let again: serde_json::Value = serde_json::from_str(&ok(cli(
        &models,
        &[
            "--offline",
            "import",
            "-p",
            &project,
            long.to_str().unwrap(),
            "-c",
            "tok",
            "--id",
            "id",
            "--title",
            "title",
            "--text",
            "body",
            "--json",
        ],
    )))
    .unwrap();
    assert_eq!(again["unchanged"], 1);
    let chunks = r["chunks_written"].as_u64().unwrap();
    assert!((3..=8).contains(&chunks), "{chunks}");
    let r: serde_json::Value = serde_json::from_str(&ok(cli(
        &models,
        &["--offline", "embed", "-p", &project, "-c", "tok", "--json"],
    )))
    .unwrap();
    // Only the parent (head-truncated whole text) exceeds the cap; no chunk does.
    assert_eq!(r["truncated"], 1, "{r}");
}

/// `invectus mcp` speaks newline-delimited JSON-RPC on stdout and logs only to stderr.
#[test]
fn mcp_server_over_stdio() {
    use std::io::Write;
    use std::process::Stdio;
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_invectus"))
        .args(["--offline", "mcp"])
        .env("INVECTUS_MODELS_DIR", dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for line in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"check_idea","arguments":{"collection":"c","idea":"x"}}}"#,
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success());
    let replies: Vec<serde_json::Value> = String::from_utf8(o.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        replies.len(),
        3,
        "one reply per request, none for the notification"
    );
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "invectus");
    assert_eq!(replies[1]["result"]["tools"].as_array().unwrap().len(), 21);
    assert_eq!(replies[2]["result"]["isError"], true);
    assert!(String::from_utf8_lossy(&o.stderr).contains("ready on stdio"));
}

/// A small MCP client over the server's stdio.
struct Mcp {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    stdout: std::io::BufReader<std::process::ChildStdout>,
    next: i64,
}

impl Mcp {
    fn start(models: &Path) -> Self {
        use std::process::Stdio;
        let mut child = Command::new(env!("CARGO_BIN_EXE_invectus"))
            .args(["--offline", "mcp"])
            .env("INVECTUS_MODELS_DIR", models)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut m = Self {
            child,
            stdin,
            stdout,
            next: 0,
        };
        m.request(
            "initialize",
            serde_json::json!({ "protocolVersion": "2025-06-18", "clientInfo": { "name": "t", "version": "0" } }),
        );
        m
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        use std::io::{BufRead, Write};
        self.next += 1;
        let msg = serde_json::json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params });
        writeln!(self.stdin.as_mut().unwrap(), "{msg}").unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let reply: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["id"], self.next);
        reply["result"].clone()
    }

    /// A tool's structured result, asserting it did not fail.
    fn tool(&mut self, name: &str, args: serde_json::Value) -> serde_json::Value {
        let r = self.request(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": args }),
        );
        assert_eq!(r["isError"], false, "{name}: {r}");
        r["structuredContent"].clone()
    }

    fn tool_error(&mut self, name: &str, args: serde_json::Value) -> String {
        let r = self.request(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": args }),
        );
        assert_eq!(r["isError"], true, "{name}: {r}");
        r["content"][0]["text"].as_str().unwrap().to_string()
    }

    fn close(mut self) {
        drop(self.stdin.take());
        assert!(self.child.wait().unwrap().success());
    }
}

/// The A3 goal: an agent creates a project, imports a CSV, follows the background job and
/// reports gaps, then exports them. A project held by another process is refused. Needs the
/// default model in INVECTUS_MODELS_DIR (the dev store); skipped otherwise.
#[test]
fn agent_imports_a_csv_and_reports_gaps_over_mcp() {
    let Some(models) = std::env::var_os("INVECTUS_MODELS_DIR")
        .map(PathBuf::from)
        .filter(|m| m.join("bge-small-en-v1.5").is_dir())
    else {
        eprintln!("SKIP: no bge-small-en-v1.5 in INVECTUS_MODELS_DIR");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("notes.csv");
    let topics = [
        "retinal detachment surgery outcomes",
        "glaucoma intraocular pressure medication",
        "myopia progression in children",
        "corneal transplant rejection",
    ];
    let mut rows = String::from("id,title,body,kind\n");
    for i in 0..240 {
        let t = topics[i % 4];
        let kind = if i % 10 == 0 { "review" } else { "study" };
        rows.push_str(&format!(
            "{i},Study {i} on {t},\"Patients in cohort {i} were followed for {t}. We report \
             baseline characteristics, the intervention schedule, adverse events and outcomes \
             at one and two years for case series {i}, with {} participants.\",{kind}\n",
            40 + i
        ));
    }
    std::fs::write(&csv, rows).unwrap();
    let project = dir.path().join("agent").display().to_string();

    let mut m = Mcp::start(&models);
    let info = m.tool(
        "open_project",
        serde_json::json!({ "project": project, "create": true }),
    );
    assert_eq!(info["collections"], serde_json::json!([]));
    let src = m.tool("inspect_source", serde_json::json!({ "source": csv }));
    assert_eq!(
        src["columns"],
        serde_json::json!(["id", "title", "body", "kind"])
    );
    let out = m.tool(
        "import_collection",
        serde_json::json!({
            "collection": "notes", "source": csv, "id": "id", "title": "title",
            "text": ["body"], "role_column": "kind", "role_rules": "review=coverage,study=demand"
        }),
    );
    assert_eq!(out["report"]["inserted"], 240, "{out}");
    assert!(out["job"].is_object(), "{out}");
    // A second job is refused while the first runs; reads keep answering.
    let start = std::time::Instant::now();
    let job = loop {
        let v = m.tool("job_status", serde_json::json!({}));
        if v["current"]["state"] != "running" {
            break v["current"].clone();
        }
        assert!(start.elapsed().as_secs() < 300, "job did not finish");
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    assert_eq!(job["state"], "completed", "{job}");
    let status = m.tool("project_status", serde_json::json!({}));
    let table = &status["tables"][0];
    assert_eq!(table["index_status"], "indexed");
    assert!(table["calibration"].is_object(), "{table}");
    assert!(table["regions"]["scanned"] == true, "{table}");

    let gaps = m.tool(
        "list_gaps",
        serde_json::json!({ "collection": "notes", "limit": 5 }),
    );
    assert!(
        gaps["scope"]
            .as_str()
            .unwrap()
            .contains("this collection only")
    );
    let rescan = m.tool("scan_gaps", serde_json::json!({ "collection": "notes" }));
    assert!(rescan["gaps"].is_array(), "{rescan}");
    let verdict = m.tool(
        "check_idea",
        serde_json::json!({ "collection": "notes", "idea": "eye pressure lowering drugs" }),
    );
    assert!(verdict["verdict"].is_string());
    // Verification needs the network: refused here (offline, and the switch is off).
    let id = gaps["gaps"][0]["gap_id"]
        .as_i64()
        .expect("the scan found gaps");
    {
        let e = m.tool_error(
            "verify_gap",
            serde_json::json!({ "collection": "notes", "gap_id": id }),
        );
        assert!(e.contains("network") || e.contains("offline"), "{e}");
    }
    // Provenance: the suggestion is recorded as the agent's and leaves the decision alone.
    {
        let v = m.tool(
            "suggest_decision",
            serde_json::json!({
                "collection": "notes", "gap_id": id, "verdict": "already_exists",
                "reasoning": "Study 0 already covers this.", "model": "test-model"
            }),
        );
        assert_eq!(v["suggestion"]["author"], "agent:t/test-model", "{v}");
        assert!(v["verdict"].is_null());
        let d = m.tool(
            "gap_detail",
            serde_json::json!({ "collection": "notes", "gap_id": id }),
        );
        assert_eq!(d["suggestion"]["note"], "Study 0 already covers this.");
    }
    // Agent-written descriptions (A5): same evidence and checks as the local model's.
    let ctx = m.tool(
        "gap_context",
        serde_json::json!({ "collection": "notes", "gap_id": id }),
    );
    assert!(
        ctx["prompt"].as_str().unwrap().contains("Reply exactly as"),
        "{ctx}"
    );
    assert!(!ctx["checks"].as_array().unwrap().is_empty());
    let off = m.tool(
        "submit_description",
        serde_json::json!({
            "collection": "notes", "gap_id": id, "model": "test-model",
            "title": "Lattice gauge theory on quantum hardware",
            "description": "Simulating quark confinement with superconducting qubits and error mitigation."
        }),
    );
    assert_eq!(off["stored"], false, "{off}");
    assert!(
        off["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("rejected")),
        "{off}"
    );
    let label = gaps["gaps"][0]["label"]
        .as_str()
        .unwrap()
        .replace(" · ", " ");
    let on = m.tool(
        "submit_description",
        serde_json::json!({
            "collection": "notes", "gap_id": id, "model": "test-model",
            "title": format!("Systematic review of {label}"),
            "description": format!("A systematic review pooling cohorts on {label}, with outcomes at one and two years.")
        }),
    );
    assert_eq!(on["attempts"][0]["on_target"]["passed"], true, "{on}");
    assert_eq!(on["author"], "agent:t/test-model");
    assert_eq!(on["stored"], on["accepted"].is_number());

    // Agent paraphrases (A5): the calibration sample, paged; the curve is refitted from them.
    let mut items = Vec::new();
    loop {
        let page = m.tool(
            "paraphrase_sample",
            serde_json::json!({ "collection": "notes", "offset": items.len(), "limit": 100 }),
        );
        let got = page["items"].as_array().unwrap().clone();
        if got.is_empty() {
            assert_eq!(items.len(), page["total"].as_u64().unwrap() as usize);
            break;
        }
        items.extend(got);
    }
    assert_eq!(items.len(), 240);
    assert!(
        items[0]["item_id"].is_string(),
        "ids are strings for JavaScript clients"
    );
    let mut paraphrases: Vec<serde_json::Value> = items
        .iter()
        .map(|i| {
            let topic = i["title"].as_str().unwrap().split(" on ").nth(1).unwrap_or("it");
            serde_json::json!({ "item_id": i["item_id"], "paraphrase": format!("Following a patient cohort for {topic} over two years") })
        })
        .collect();
    paraphrases.push(serde_json::json!({ "item_id": "12", "paraphrase": "not sampled" }));
    let cal = m.tool(
        "submit_paraphrases",
        serde_json::json!({ "collection": "notes", "paraphrases": paraphrases, "model": "test-model" }),
    );
    assert_eq!(cal["accepted"], 240, "{cal}");
    assert_eq!(
        cal["rejected"][0]["reason"],
        "not in the calibration sample"
    );
    let start = std::time::Instant::now();
    let job = loop {
        let v = m.tool("job_status", serde_json::json!({}));
        if v["current"]["state"] != "running" {
            break v["current"].clone();
        }
        assert!(
            start.elapsed().as_secs() < 300,
            "calibration did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    assert_eq!(job["state"], "completed", "{job}");
    let status = m.tool("project_status", serde_json::json!({}));
    let method = status["tables"][0]["calibration"]["method"]
        .as_str()
        .unwrap();
    assert_eq!(method, "llm paraphrase (agent:t/test-model)");
    // The report compares with the curve it replaced (the pipeline's title calibration).
    assert!(
        job["report"]["previous"]["method"]
            .as_str()
            .unwrap()
            .contains("title"),
        "{job}"
    );

    // Agent region names (A5), recorded with the agent as writer.
    let regions = m.tool(
        "region_context",
        serde_json::json!({ "collection": "notes" }),
    );
    let first = &regions["regions"][0];
    assert!(
        !first["central_titles"].as_array().unwrap().is_empty(),
        "{first}"
    );
    let names: Vec<serde_json::Value> = regions["regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| serde_json::json!({ "region": r["region"], "name": format!("\"{} studies.\"", r["terms"][0].as_str().unwrap()) }))
        .chain([serde_json::json!({ "region": 999, "name": "nowhere" })])
        .collect();
    let named = m.tool(
        "submit_region_names",
        serde_json::json!({ "collection": "notes", "names": names, "model": "test-model" }),
    );
    assert_eq!(named["rejected"].as_array().unwrap().len(), 1, "{named}");
    let all = m.tool(
        "region_context",
        serde_json::json!({ "collection": "notes", "unnamed_only": false }),
    );
    let r0 = &all["regions"][0];
    assert_eq!(r0["named_by"], "agent:t/test-model");
    assert!(
        !r0["name"].as_str().unwrap().ends_with('.'),
        "cleaned like the model's"
    );
    let unnamed = m.tool(
        "region_context",
        serde_json::json!({ "collection": "notes" }),
    );
    assert_eq!(unnamed["total"], 0);
    let exported = m.tool(
        "export_gaps",
        serde_json::json!({ "collection": "notes", "format": "markdown" }),
    );
    let path = PathBuf::from(exported["path"].as_str().unwrap());
    assert!(path.starts_with(dir.path().join("agent.invectus").join("exports")));
    assert!(std::fs::read_to_string(&path).unwrap().contains('#'));
    m.close();

    // Single writer: while another process holds the project, the server refuses it.
    let held = invectus_core::Project::open(&dir.path().join("agent.invectus")).unwrap();
    let mut m = Mcp::start(&models);
    let e = m.tool_error("project_status", serde_json::json!({ "project": project }));
    assert!(e.contains("one inVectus process at a time"), "{e}");
    m.close();
    drop(held);

    // A client that leaves mid-job: the server cancels it and waits, so no job record is left
    // "running" (an interrupted index).
    let mut m = Mcp::start(&models);
    m.tool(
        "import_collection",
        serde_json::json!({
            "project": project, "collection": "again", "source": csv, "id": "id",
            "title": "title", "text": ["body"]
        }),
    );
    m.close();
    let p = invectus_core::Project::open(&dir.path().join("agent.invectus")).unwrap();
    let jobs = p.store.jobs().unwrap();
    assert!(
        jobs.iter().all(|j| j.state != "running"),
        "{:?}",
        jobs.iter().map(|j| &j.state).collect::<Vec<_>>()
    );
}

/// `invectus connect` (A6) against a temporary home: shows the change, writes only with
/// --yes, reports status, and builds a .mcpb holding this binary.
#[test]
fn connect_writes_only_when_asked_and_bundles() {
    let home = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_invectus"))
            .args(args)
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("APPDATA", home.path().join("AppData/Roaming"))
            .env("CODEX_HOME", home.path().join(".codex"))
            .env("GROK_HOME", home.path().join(".grok"))
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
    };
    let cursor = home.path().join(".cursor/mcp.json");
    let o = ok(run(&["connect", "add", "cursor"]));
    assert!(o.contains("--yes") && o.contains("\"mcpServers\""), "{o}");
    assert!(!cursor.exists(), "nothing written without --yes");
    ok(run(&["connect", "add", "cursor", "--yes"]));
    let c: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cursor).unwrap()).unwrap();
    let registered = c["mcpServers"]["invectus"]["command"].as_str().unwrap();
    assert_eq!(
        std::fs::canonicalize(registered).unwrap(),
        std::fs::canonicalize(env!("CARGO_BIN_EXE_invectus")).unwrap()
    );
    let s: serde_json::Value =
        serde_json::from_str(&ok(run(&["connect", "status", "--json"]))).unwrap();
    let cs = s
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["client"] == "cursor")
        .unwrap();
    assert_eq!(
        (cs["connected"].clone(), cs["matches_this_install"].clone()),
        (true.into(), true.into())
    );
    // Claude Code without `claude` on PATH: the command to run is shown, nothing happens.
    let o = ok(run(&["connect", "add", "claude-code"]));
    assert!(o.contains("claude mcp add --scope user invectus --"), "{o}");
    assert!(
        fail(run(&["connect", "add", "claude-code", "--yes"])).contains("cannot be configured")
    );
    ok(run(&["connect", "remove", "cursor", "--yes"]));
    assert!(
        !std::fs::read_to_string(&cursor)
            .unwrap()
            .contains("invectus\"")
    );
    assert!(fail(run(&["connect", "add", "vim"])).contains("unknown client"));

    let bundle = home.path().join("invectus.mcpb");
    ok(run(&[
        "connect",
        "bundle",
        "--out",
        bundle.to_str().unwrap(),
    ]));
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&bundle).unwrap()).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_reader(archive.by_name("manifest.json").unwrap()).unwrap();
    let entry = manifest["server"]["entry_point"].as_str().unwrap();
    assert_eq!(
        entry,
        format!("server/invectus{}", std::env::consts::EXE_SUFFIX)
    );
    let mut binary = archive.by_name(entry).unwrap();
    assert_eq!(binary.unix_mode().unwrap() & 0o777, 0o755);
    let mut contents = Vec::new();
    std::io::Read::read_to_end(&mut binary, &mut contents).unwrap();
    assert_eq!(
        contents,
        std::fs::read(env!("CARGO_BIN_EXE_invectus")).unwrap()
    );
}

/// A7: while "the app" (this test process) holds a project, `invectus mcp` reaches it through
/// the hand-off socket instead of failing on the one-process lock; other projects open locally.
#[cfg(unix)]
#[test]
fn mcp_hands_calls_for_the_apps_project_to_the_app() {
    use invectus_orchestration as orch;
    use std::sync::{Arc, Mutex};
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("agent.sock");
    let session = orch::Shared::default();
    let board = orch::SharedBoard::default();
    let app_project = dir.path().join("held");
    orch::open_project(&session, &board, app_project.to_str().unwrap(), true).unwrap();
    let other = dir.path().join("free");
    invectus_core::Project::create(&other).unwrap();
    let seen: Arc<Mutex<Vec<serde_json::Value>>> = Arc::default();
    let s2 = seen.clone();
    orch::handoff::serve(
        &sock,
        orch::handoff::app_handler(
            session.clone(),
            board.clone(),
            orch::agent::log_emitter(),
            Arc::new(move |e: &serde_json::Value| s2.lock().unwrap().push(e.clone())),
        ),
    )
    .unwrap();

    let run = |lines: Vec<serde_json::Value>, socket: Option<&Path>| {
        use std::io::Write;
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_invectus"));
        cmd.args(["--offline", "mcp"])
            .env("INVECTUS_MODELS_DIR", dir.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        match socket {
            Some(s) => cmd.env("INVECTUS_AGENT_SOCKET", s),
            None => cmd.env("INVECTUS_AGENT_SOCKET", dir.path().join("nobody.sock")),
        };
        let mut child = cmd.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":0,"method":"initialize","params":{{"clientInfo":{{"name":"handoff-test"}}}}}}"#).unwrap();
        for (i, l) in lines.iter().enumerate() {
            let msg =
                serde_json::json!({"jsonrpc":"2.0","id":i+1,"method":"tools/call","params":l});
            writeln!(stdin, "{msg}").unwrap();
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .filter(|r| r["id"] != 0)
            .map(|r| r["result"].clone())
            .collect::<Vec<_>>()
    };
    let held = format!("{}", app_project.with_extension("invectus").display());
    let free = format!("{}", other.with_extension("invectus").display());
    let replies = run(
        vec![
            serde_json::json!({"name":"project_status","arguments":{"project": held}}),
            serde_json::json!({"name":"project_status","arguments":{}}),
            serde_json::json!({"name":"job_status","arguments":{}}),
            serde_json::json!({"name":"project_status","arguments":{"project": free}}),
        ],
        Some(&sock),
    );
    assert_eq!(replies[0]["isError"], false, "{}", replies[0]);
    assert!(
        replies[0]["structuredContent"]["root"]
            .as_str()
            .unwrap()
            .ends_with("held.invectus")
    );
    assert_eq!(replies[1]["isError"], false, "no project named: the app's");
    assert_eq!(replies[2]["isError"], false);
    // Another project opens locally in the server (the app keeps its own).
    assert_eq!(replies[3]["isError"], false, "{}", replies[3]);
    assert!(
        replies[3]["structuredContent"]["root"]
            .as_str()
            .unwrap()
            .ends_with("free.invectus")
    );
    let events = seen.lock().unwrap().clone();
    // Three calls ran in the app; the local one in the other project is reported to it too.
    assert_eq!(events.len(), 7, "{events:?}");
    assert!(events.iter().all(|e| e["client"] == "handoff-test"));
    assert!(
        events[6]["project"]
            .as_str()
            .unwrap()
            .ends_with("free.invectus"),
        "{events:?}"
    );
    assert_eq!(events[6]["tool"], "project_status");

    // Without the app, the held project is refused by the one-process lock, as before.
    let replies = run(
        vec![serde_json::json!({"name":"project_status","arguments":{"project": held}})],
        None,
    );
    assert_eq!(replies[0]["isError"], true);
    assert!(
        replies[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("one inVectus process"),
        "{}",
        replies[0]
    );
}
