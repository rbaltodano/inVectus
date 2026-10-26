//! Embed job, search and space-safety tests with a deterministic bag-of-words embedder, so they
//! run without model files.

use anyhow::Result;
use invectus_core::item::item_id;
use invectus_core::{
    CancelToken, Embedding, EmbeddingIdentity, InputKind, Item, Normalization, Pooling, Project,
    Role, TextEmbedder,
};
use invectus_index::parity::run_parity;
use invectus_index::search::check_compatible;
use invectus_index::{
    EmbedJobOptions, SearchMode, SearchOptions, VectorStore, run_embed, search, select_table,
};
use sha2::{Digest, Sha256};

const DIM: usize = 64;

struct BagOfWords {
    identity: EmbeddingIdentity,
    /// Added to every vector: simulates another runtime's small numeric drift.
    noise: f32,
    calls: usize,
}

fn identity(runtime: &str) -> EmbeddingIdentity {
    EmbeddingIdentity {
        model_key: "bow".into(),
        repo: "test/bow".into(),
        revision: "1".into(),
        weights_sha256: format!("w-{runtime}"),
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
        runtime: runtime.into(),
    }
}

impl BagOfWords {
    fn new(runtime: &str, noise: f32) -> Self {
        Self {
            identity: identity(runtime),
            noise,
            calls: 0,
        }
    }
}

impl TextEmbedder for BagOfWords {
    fn identity(&self) -> &EmbeddingIdentity {
        &self.identity
    }
    fn embed(&mut self, texts: &[String], _kind: InputKind) -> Result<Vec<Embedding>> {
        self.calls += 1;
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = vec![self.noise; DIM];
                v[0] += 0.01;
                for w in t.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
                    if w.len() > 2 {
                        let h = Sha256::digest(w.as_bytes());
                        v[h[0] as usize % DIM] += 1.0;
                    }
                }
                let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                Embedding {
                    vector: v.iter().map(|x| x / n).collect(),
                    tokens: t.split_whitespace().count(),
                    truncated: false,
                }
            })
            .collect())
    }
}

fn item(coll: &str, src: &str, title: &str, text: &str, role: Role) -> Item {
    Item {
        item_id: item_id(coll, src, None),
        collection_id: coll.into(),
        source_id: src.into(),
        title: Some(title.into()),
        text: text.into(),
        url: None,
        date: None,
        role,
        meta: serde_json::Value::Null,
        parent_id: None,
        chunk_index: None,
        input_hash: format!("{title}{text}"),
    }
}

fn chunk(parent: &Item, i: usize, text: &str) -> Item {
    Item {
        item_id: item_id(&parent.collection_id, &parent.source_id, Some(i)),
        source_id: format!("{}#chunk{i}", parent.source_id),
        text: text.into(),
        parent_id: Some(parent.item_id),
        chunk_index: Some(i as i32),
        input_hash: text.into(),
        ..parent.clone()
    }
}

fn setup(n_extra: usize) -> (tempfile::TempDir, Project) {
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
    let topics = [
        (
            "myopia",
            "orthokeratology myopia control children axial length",
        ),
        (
            "glaucoma",
            "intraocular pressure glaucoma optic nerve trabeculectomy",
        ),
        ("cornea", "keratoconus corneal crosslinking ectasia hydrops"),
        (
            "retina",
            "diabetic retinopathy macular edema laser injection",
        ),
    ];
    let mut batch = Vec::new();
    for (i, (t, words)) in topics.iter().enumerate() {
        let role = if i == 0 { Role::Coverage } else { Role::Demand };
        batch.push((item("c", t, t, words, role), vec![]));
    }
    let long = item(
        "c",
        "long",
        "Long report",
        "general overview",
        Role::Neutral,
    );
    let chunks = vec![
        chunk(&long, 0, "general overview of clinic staffing"),
        chunk(&long, 1, "chalazion treatment eyelid steroid injection"),
    ];
    batch.push((long, chunks));
    for i in 0..n_extra {
        let words = format!("filler{i} topic{} noise{} common words", i % 17, i % 5);
        batch.push((
            item(
                "c",
                &format!("x{i}"),
                &format!("Doc {i}"),
                &words,
                Role::Neutral,
            ),
            vec![],
        ));
    }
    p.store.write_items(&batch).unwrap();
    (dir, p)
}

fn run(
    p: &Project,
    v: &VectorStore,
    e: &mut BagOfWords,
    opts: &EmbedJobOptions,
) -> invectus_index::EmbedReport {
    run_embed(p, v, "c", e, opts, &CancelToken::default(), &mut |_| {}).unwrap()
}

#[test]
fn embed_is_idempotent_resumable_and_removes_stale_vectors() {
    let (_d, mut p) = setup(20);
    let v = VectorStore::open(&p).unwrap();
    let mut e = BagOfWords::new("a", 0.0);
    let opts = EmbedJobOptions {
        flush_every: 4,
        ann_min_rows: 1_000_000,
    };
    // Cancel after the first flushed batch.
    let cancel = CancelToken::default();
    let c2 = cancel.clone();
    let r = run_embed(&p, &v, "c", &mut e, &opts, &cancel, &mut |pr| {
        if pr.done > 0 {
            c2.cancel();
        }
    })
    .unwrap();
    assert!(r.cancelled);
    assert_eq!(r.embedded, 4);
    assert_eq!(p.store.job(&r.job_id).unwrap().unwrap().state, "cancelled");

    // Resume: only the remainder is embedded (5 topics/long + 2 chunks + 20 extra = 27 inputs).
    let r = run(&p, &v, &mut e, &opts);
    assert_eq!((r.inputs, r.already_embedded, r.embedded), (27, 4, 23));
    assert_eq!(r.vectors_in_table, 27);
    assert_eq!(p.store.job(&r.job_id).unwrap().unwrap().state, "completed");

    // Nothing changed: nothing is embedded and the model is not called.
    let calls = e.calls;
    let r = run(&p, &v, &mut e, &opts);
    assert_eq!(
        (r.embedded, r.already_embedded, r.removed_stale),
        (0, 27, 0)
    );
    assert_eq!(e.calls, calls);

    // A changed item replaces exactly its vector.
    let changed = item(
        "c",
        "glaucoma",
        "glaucoma",
        "angle closure glaucoma laser iridotomy",
        Role::Demand,
    );
    p.store.write_items(&[(changed, vec![])]).unwrap();
    let r = run(&p, &v, &mut e, &opts);
    assert_eq!(
        (r.embedded, r.removed_stale, r.vectors_in_table),
        (1, 1, 27)
    );
}

#[test]
fn search_returns_parents_with_chunk_evidence_and_respects_roles() {
    let (_d, p) = setup(30);
    let v = VectorStore::open(&p).unwrap();
    let mut e = BagOfWords::new("a", 0.0);
    run(&p, &v, &mut e, &EmbedJobOptions::default());
    let table = select_table(&p, "c", None).unwrap();

    let r = search(
        &p,
        &v,
        &table,
        &mut e,
        "keratoconus crosslinking",
        &SearchOptions::default(),
    )
    .unwrap();
    assert!(
        r.exhaustive,
        "small table without index is searched exhaustively"
    );
    assert_eq!(r.vectors_compared, 37);
    assert_eq!(r.hits[0].item.source_id, "cornea");
    assert!(
        r.hits
            .windows(2)
            .all(|w| w[0].similarity >= w[1].similarity)
    );
    assert_eq!(r.hits.len(), 10);
    assert!(r.notes.iter().any(|n| n.contains("not a calibrated")));

    // A phrase that only occurs in the second chunk returns the parent, marked via that chunk.
    let r = search(
        &p,
        &v,
        &table,
        &mut e,
        "chalazion eyelid steroid",
        &SearchOptions::default(),
    )
    .unwrap();
    assert_eq!(r.hits[0].item.source_id, "long");
    assert_eq!(r.hits[0].via_chunk, Some(1));
    assert!(r.hits.iter().filter(|h| h.item.source_id == "long").count() == 1);

    // Role filter: only coverage items are eligible.
    let opts = SearchOptions {
        role: Some(Role::Coverage),
        ..Default::default()
    };
    let r = search(&p, &v, &table, &mut e, "keratoconus crosslinking", &opts).unwrap();
    assert_eq!(r.hits.len(), 1);
    assert_eq!(r.hits[0].item.source_id, "myopia");

    let ann = SearchOptions {
        mode: SearchMode::Ann { nprobes: None },
        ..Default::default()
    };
    assert!(
        search(&p, &v, &table, &mut e, "x", &ann).is_err(),
        "no index yet"
    );
}

#[test]
fn ann_search_is_rescored_exactly_and_agrees_with_exhaustive_at_full_probes() {
    let (_d, p) = setup(400);
    let v = VectorStore::open(&p).unwrap();
    let mut e = BagOfWords::new("a", 0.0);
    let r = run(
        &p,
        &v,
        &mut e,
        &EmbedJobOptions {
            flush_every: 128,
            ann_min_rows: 100,
        },
    );
    let parts = r.ann_partitions.expect("index built");
    let table = select_table(&p, "c", None).unwrap();
    assert!(v.has_ann(&table).unwrap());

    let auto = search(
        &p,
        &v,
        &table,
        &mut e,
        "intraocular pressure glaucoma",
        &SearchOptions::default(),
    )
    .unwrap();
    assert!(!auto.exhaustive && auto.mode.starts_with("ann"));
    assert!(auto.notes.iter().any(|n| n.contains("exhaustive")));
    assert_eq!(auto.hits[0].item.source_id, "glaucoma");

    let full = SearchOptions {
        mode: SearchMode::Ann {
            nprobes: Some(parts as usize),
        },
        ..Default::default()
    };
    let exact = SearchOptions {
        mode: SearchMode::Exhaustive,
        ..Default::default()
    };
    for q in [
        "filler7 topic7 noise2",
        "diabetic macular edema",
        "common words topic3",
    ] {
        let a = search(&p, &v, &table, &mut e, q, &full).unwrap();
        let x = search(&p, &v, &table, &mut e, q, &exact).unwrap();
        assert!(x.exhaustive && x.vectors_compared == 407);
        let sims = |r: &invectus_index::SearchResult| {
            r.hits.iter().map(|h| h.similarity).collect::<Vec<_>>()
        };
        // Exact re-score: identical similarity values for every returned rank.
        assert_eq!(sims(&a), sims(&x), "{q}");
    }
}

#[test]
fn spaces_never_mix_without_a_passing_parity_check() {
    let (_d, p) = setup(10);
    let v = VectorStore::open(&p).unwrap();
    let mut a = BagOfWords::new("a", 0.0);
    run(&p, &v, &mut a, &EmbedJobOptions::default());
    let table = select_table(&p, "c", None).unwrap();

    // Different pooling: a different model space, always refused.
    let mut other = BagOfWords::new("a", 0.0);
    other.identity.pooling = Pooling::Cls;
    let err = search(
        &p,
        &v,
        &table,
        &mut other,
        "glaucoma",
        &SearchOptions::default(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("never compared"), "{err}");

    // Same text policy, different runtime: refused until parity is checked.
    let mut close = BagOfWords::new("b", 0.001);
    let err = check_compatible(&p, &table, &close)
        .unwrap_err()
        .to_string();
    assert!(err.contains("invectus parity"), "{err}");
    let rep = run_parity(&p, &v, &table, &mut close, 50).unwrap();
    assert!(rep.record.passed, "{:?}", rep.record);
    assert!(
        check_compatible(&p, &table, &close)
            .unwrap()
            .starts_with("parity-verified")
    );
    assert!(
        search(
            &p,
            &v,
            &table,
            &mut close,
            "glaucoma",
            &SearchOptions::default()
        )
        .is_ok()
    );

    // A runtime whose vectors drift too far fails parity and stays refused.
    let mut far = BagOfWords::new("c", 0.3);
    let rep = run_parity(&p, &v, &table, &mut far, 50).unwrap();
    assert!(!rep.record.passed);
    assert!(check_compatible(&p, &table, &far).is_err());

    // A second space in the same collection gets its own table; selection becomes explicit.
    let mut other_space = BagOfWords::new("d", 0.0);
    run(&p, &v, &mut other_space, &EmbedJobOptions::default());
    assert!(select_table(&p, "c", None).is_err());
    assert_eq!(
        select_table(&p, "c", Some(&table.space_id[..12]))
            .unwrap()
            .space_id,
        table.space_id
    );
    assert_eq!(v.table_names().unwrap().len(), 2);
}
