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
    let (_dir, models, project) = setup();
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
    let (_dir, models, project) = setup();
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
}
