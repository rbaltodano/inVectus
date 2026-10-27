//! Real-model checks. They need the pinned BGE files, which are not in the repository: set
//! `INVECTUS_TEST_MODEL_SRC` to a folder of model folders (default: `<repo>/.spike-data/models`).
//! Without them the tests print a skip notice and pass.

use invectus_core::{InputKind, TextEmbedder, TokenOffsets};
use invectus_embed::{EmbedOptions, ModelStore, ModelTokenizer, OnnxEmbedder, registry};
use std::path::PathBuf;

fn bge() -> Option<(tempfile::TempDir, OnnxEmbedder)> {
    let src = std::env::var_os("INVECTUS_TEST_MODEL_SRC")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.spike-data/models")
        });
    let src = src.join("bge");
    if !src.join("model.onnx").is_file() {
        eprintln!("SKIP: no BGE model files at {}", src.display());
        return None;
    }
    let root = tempfile::tempdir().unwrap();
    let store = ModelStore::new(root.path());
    let spec = registry::spec("bge-small-en-v1.5").unwrap();
    store.install_from_dir(spec, &src).unwrap();
    let opts = EmbedOptions {
        threads: 4,
        batch_size: 4,
    };
    let e = OnnxEmbedder::load(&store, spec.key, &opts).unwrap();
    Some((root, e))
}

fn cos(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[test]
fn bge_embeds_unit_vectors_with_semantic_order_and_reports_truncation() {
    let Some((_root, mut e)) = bge() else { return };
    assert_eq!(e.identity().dimensions, 384);
    let texts: Vec<String> = [
        "Myopia control with orthokeratology lenses in children",
        "Orthokeratology slows axial elongation in myopic children",
        "Quarterly revenue of a retail bank",
    ]
    .map(String::from)
    .to_vec();
    let v = e.embed(&texts, InputKind::Document).unwrap();
    for x in &v {
        let n: f32 = x.vector.iter().map(|a| a * a).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-4);
        assert!(!x.truncated);
    }
    assert!(cos(&v[0].vector, &v[1].vector) > cos(&v[0].vector, &v[2].vector) + 0.1);

    // Batching and ordering do not change results.
    let single = e.embed(&texts[2..3], InputKind::Document).unwrap();
    assert!(cos(&single[0].vector, &v[2].vector) > 0.9999);

    // The query prefix is applied: same text, different vector.
    let q = e.embed(&texts[..1], InputKind::Query).unwrap();
    assert!(cos(&q[0].vector, &v[0].vector) < 0.9999);

    let long = vec!["word ".repeat(2000)];
    let t = e.embed(&long, InputKind::Document).unwrap();
    assert!(t[0].truncated);
    assert_eq!(t[0].tokens, 512);
}

#[test]
fn bge_tokenizer_offsets_cover_the_text_and_match_the_embedder() {
    let Some((root, mut e)) = bge() else { return };
    let store = ModelStore::new(root.path());
    let t = ModelTokenizer::load(&store, "bge-small-en-v1.5").unwrap();
    assert_eq!(
        t.tokenizer_sha256(),
        registry::spec("bge-small-en-v1.5")
            .unwrap()
            .file("tokenizer.json")
            .sha256
    );
    let text = "Kératocône: crosslinking slows ectasia.  Orthokeratology works.";
    let offsets = t.offsets(text).unwrap();
    // Byte ranges on char boundaries, in order, and every word is covered.
    assert!(offsets.windows(2).all(|w| w[0].1 <= w[1].0));
    let pieces: Vec<&str> = offsets.iter().map(|&(a, b)| &text[a..b]).collect();
    assert!(pieces.contains(&":") && pieces.contains(&"."));
    assert_eq!(pieces.concat().replace(' ', ""), text.replace(' ', ""));
    // Same count the embedder sees, minus [CLS] and [SEP].
    let v = e.embed(&[text.to_string()], InputKind::Document).unwrap();
    assert_eq!(offsets.len() + 2, v[0].tokens);
    assert_eq!(t.offsets(&"word ".repeat(2000)).unwrap().len(), 2000);
}
