//! Calibration and verdicts end to end with a deterministic bag-of-words embedder (no model files).

use anyhow::Result;
use invectus_core::item::item_id;
use invectus_core::{
    CancelToken, Embedding, EmbeddingIdentity, InputKind, Item, Normalization, Pooling, Project,
    Role, TextEmbedder,
};
use invectus_index::{EmbedJobOptions, VectorStore, run_embed, select_table};
use invectus_unsearch::{CalibrateOptions, CheckOptions, Verdict, check, run_calibration};
use sha2::{Digest, Sha256};

/// Wide enough that hashed words rarely collide.
const DIM: usize = 2048;

struct BagOfWords(EmbeddingIdentity);

impl BagOfWords {
    fn new() -> Self {
        Self(EmbeddingIdentity {
            model_key: "bow".into(),
            repo: "test/bow".into(),
            revision: "1".into(),
            weights_sha256: "w".into(),
            tokenizer_sha256: "t".into(),
            document_prefix: String::new(),
            query_prefix: String::new(),
            text_template: "{title}. {text}".into(),
            pooling: Pooling::Mean,
            normalization: Normalization::L2,
            max_tokens: 512,
            truncation: "head".into(),
            dimensions: DIM,
            compute_precision: "fp32".into(),
            storage_precision: "f32".into(),
            runtime: "test".into(),
        })
    }
}

impl TextEmbedder for BagOfWords {
    fn identity(&self) -> &EmbeddingIdentity {
        &self.0
    }
    fn embed(&mut self, texts: &[String], _kind: InputKind) -> Result<Vec<Embedding>> {
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = vec![0.0f32; DIM];
                v[0] = 0.05;
                for w in t.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
                    if w.len() > 2 {
                        let h = Sha256::digest(w.as_bytes());
                        v[usize::from(u16::from_le_bytes([h[0], h[1]])) % DIM] += 1.0;
                    }
                }
                let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                Embedding {
                    vector: v.iter().map(|x| x / n).collect(),
                    tokens: 1,
                    truncated: false,
                }
            })
            .collect())
    }
}

fn item(i: usize) -> Item {
    let title = format!("term{i}a term{i}b term{i}c");
    let text = format!(
        "term{i}a term{i}b term{i}c term{i}d among patients in cohort topic{} with outcome measures",
        i % 6
    );
    Item {
        item_id: item_id("c", &i.to_string(), None),
        collection_id: "c".into(),
        source_id: i.to_string(),
        title: Some(title.clone()),
        input_hash: format!("{title}{text}"),
        text,
        url: None,
        date: None,
        role: if i.is_multiple_of(4) {
            Role::Coverage
        } else {
            Role::Demand
        },
        meta: serde_json::Value::Null,
        parent_id: None,
        chunk_index: None,
    }
}

fn setup(n: usize) -> (tempfile::TempDir, Project, VectorStore) {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::create(&dir.path().join("t")).unwrap();
    p.store
        .upsert_collection(&invectus_core::store::CollectionRecord {
            collection_id: "c".into(),
            source: serde_json::json!({}),
            mapping: serde_json::json!({}),
            chunking: serde_json::json!({}),
        })
        .unwrap();
    let batch: Vec<_> = (0..n).map(|i| (item(i), vec![])).collect();
    p.store.write_items(&batch).unwrap();
    let v = VectorStore::open(&p).unwrap();
    run_embed(
        &p,
        &v,
        "c",
        &mut BagOfWords::new(),
        &EmbedJobOptions::default(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    (dir, p, v)
}

fn opts() -> CalibrateOptions {
    CalibrateOptions {
        min_text_chars: 20,
        ..Default::default()
    }
}

#[test]
fn calibrated_verdicts_separate_existing_from_new_ideas() {
    let (_d, p, v) = setup(80);
    let table = select_table(&p, "c", None).unwrap();
    let mut e = BagOfWords::new();

    let err = check(&p, &v, &table, &mut e, "anything", &CheckOptions::default())
        .unwrap_err()
        .to_string();
    assert!(err.contains("invectus calibrate"), "{err}");

    let mut ticks = 0;
    let r = run_calibration(
        &p,
        &v,
        &table,
        &mut e,
        &opts(),
        &CancelToken::default(),
        &mut |_| ticks += 1,
    )
    .unwrap();
    assert_eq!(r.items, 80);
    assert!(ticks > 0);
    assert!(r.holdout.auc > 0.95, "{:?}", r.holdout);
    assert!(
        r.own_text.p50 > r.nearest_other.p90,
        "{:?} {:?}",
        r.own_text,
        r.nearest_other
    );
    assert!(r.pairwise_auc > 0.95 && r.related.n > 0);
    assert_eq!(r.known_item.recall_at_1, 1.0);
    assert!(r.curve.points.windows(2).all(|w| w[0].1 < w[1].1));
    let job = p
        .store
        .job(&invectus_unsearch::calibrate::job_id("c", &table.space_id));
    assert_eq!(job.unwrap().unwrap().state, "completed");

    // An item's own title: exists, with that item as the top evidence.
    let c = check(
        &p,
        &v,
        &table,
        &mut e,
        "term7a term7b term7c",
        &CheckOptions::default(),
    )
    .unwrap();
    assert_eq!(c.verdict, Verdict::Exists, "{c:?}");
    assert_eq!(c.search.hits[0].item.source_id, "7");
    assert!(c.search.exhaustive);
    assert!(c.p_exists >= 0.7);
    assert!(c.calibration.quality.contains("without a language model"));
    assert!(!c.calibration.stale);
    assert!(c.between.is_empty() && c.position.is_none());
    assert!(c.notes.iter().any(|n| n.contains("invectus regions")));
    assert_eq!(c.local_density.is_some(), c.drop_to_10th.is_some());

    // Words no item uses (chosen so their hash buckets collide with no item word): open.
    let c = check(
        &p,
        &v,
        &table,
        &mut e,
        "zebra xylophone quantum giraffe",
        &CheckOptions::default(),
    )
    .unwrap();
    assert_eq!(c.verdict, Verdict::Open, "{c:?}");
    assert!(c.p_exists < 0.3);
    assert!(c.next_step.contains("outside sources"));

    // Against coverage only, item 7 (demand) no longer counts.
    let c = check(
        &p,
        &v,
        &table,
        &mut e,
        "term7a term7b term7c",
        &CheckOptions {
            role: Some(Role::Coverage),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(c.search.hits.iter().all(|h| h.item.role == Role::Coverage));
    assert_ne!(c.verdict, Verdict::Exists);
}

#[test]
fn calibration_goes_stale_and_cancels_cleanly() {
    let (_d, mut p, v) = setup(40);
    let table = select_table(&p, "c", None).unwrap();
    let mut e = BagOfWords::new();

    let cancel = CancelToken::default();
    cancel.cancel();
    let r = run_calibration(&p, &v, &table, &mut e, &opts(), &cancel, &mut |_| {}).unwrap();
    assert!(r.cancelled);
    assert!(
        p.store
            .latest_calibration("c", &table.space_id)
            .unwrap()
            .is_none()
    );

    run_calibration(
        &p,
        &v,
        &table,
        &mut e,
        &opts(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    p.store.write_items(&[(item(500), vec![])]).unwrap();
    run_embed(
        &p,
        &v,
        "c",
        &mut e,
        &EmbedJobOptions::default(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    let c = check(
        &p,
        &v,
        &table,
        &mut e,
        "term3a term3b",
        &CheckOptions::default(),
    )
    .unwrap();
    assert!(c.calibration.stale);
    assert!(
        c.notes
            .iter()
            .any(|n| n.contains("re-run `invectus calibrate`"))
    );

    // Too few titled items to calibrate.
    let (_d2, p2, v2) = setup(10);
    let t2 = select_table(&p2, "c", None).unwrap();
    let err = run_calibration(
        &p2,
        &v2,
        &t2,
        &mut e,
        &opts(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("30 or more"), "{err}");
}
