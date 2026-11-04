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
    assert_eq!(replies[1]["result"]["tools"].as_array().unwrap().len(), 7);
    assert_eq!(replies[2]["result"]["isError"], true);
    assert!(String::from_utf8_lossy(&o.stderr).contains("ready on stdio"));
}
