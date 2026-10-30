//! Gap scan end to end: planted coverage, stale and bridge gaps with a bag-of-words embedder.

use anyhow::Result;
use invectus_core::item::item_id;
use invectus_core::{
    CancelToken, Embedding, EmbeddingIdentity, InputKind, Item, Normalization, Pooling, Project,
    Role, TextEmbedder,
};
use invectus_gaps::{GapKind, ScanOptions, detail, list, scan, set_verdict};
use invectus_index::{EmbedJobOptions, VectorStore, run_embed, select_table};
use invectus_regions::{RegionOptions, run_regions};
use sha2::{Digest, Sha256};

const DIM: usize = 1024;

struct BagOfWords(EmbeddingIdentity);

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

fn identity() -> EmbeddingIdentity {
    EmbeddingIdentity {
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
    }
}

/// A: no coverage (coverage gap). B: old coverage, newer demand (stale). C: recent coverage
/// (no gap). D: shares two words with A and is close to it, with nothing between (bridge).
const TOPICS: [&[&str]; 4] = [
    &[
        "keratoconus",
        "crosslinking",
        "cornea",
        "ectasia",
        "riboflavin",
        "topography",
        "pachymetry",
        "astigmatism",
    ],
    &[
        "glaucoma",
        "trabeculectomy",
        "pressure",
        "intraocular",
        "drainage",
        "tube",
        "angle",
        "perimetry",
    ],
    &[
        "retinopathy",
        "diabetic",
        "macular",
        "edema",
        "injection",
        "laser",
        "vitreous",
        "neovascular",
    ],
    &[
        "keratoconus",
        "crosslinking",
        "scleral",
        "contact",
        "fitting",
        "comfort",
        "vault",
        "haptic",
    ],
];

fn item(topic: usize, i: usize, role: Role, date: &str) -> Item {
    let words = TOPICS[topic];
    let text: Vec<&str> = (0..5).map(|j| words[(i + j * 3) % 8]).collect();
    let text = format!("{} uniq{topic}x{i}", text.join(" "));
    let src = format!("{topic}-{i}");
    Item {
        item_id: item_id("c", &src, None),
        collection_id: "c".into(),
        source_id: src,
        title: Some(format!("{} item {i}", words[i % 8])),
        input_hash: format!("{text}{date}"),
        text,
        url: None,
        date: Some(date.into()),
        role,
        meta: serde_json::Value::Null,
        parent_id: None,
        chunk_index: None,
    }
}

/// Demand dated so yearly counts grow toward 2024.
fn growing(i: usize) -> String {
    format!("{}-03-01", 2024 - (i * i / 150).min(5))
}

fn setup() -> (tempfile::TempDir, Project, VectorStore) {
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
    let mut batch = Vec::new();
    for t in 0..4 {
        for i in 0..28 {
            batch.push((item(t, i, Role::Demand, &growing(i)), vec![]));
        }
    }
    for i in 28..31 {
        batch.push((item(1, i, Role::Coverage, "2016-01-01"), vec![]));
    }
    for i in 28..34 {
        batch.push((item(2, i, Role::Coverage, "2024-09-01"), vec![]));
    }
    // A placeholder date in the future is ignored.
    batch.push((item(0, 40, Role::Demand, "2099-12-31"), vec![]));
    p.store.write_items(&batch).unwrap();
    let v = VectorStore::open(&p).unwrap();
    run_embed(
        &p,
        &v,
        "c",
        &mut BagOfWords(identity()),
        &EmbedJobOptions::default(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    (dir, p, v)
}

fn regions(p: &mut Project, v: &VectorStore) {
    let table = select_table(p, "c", None).unwrap();
    let r = run_regions(
        p,
        v,
        &table,
        &RegionOptions {
            k: 10,
            target_regions: Some(4),
            ..Default::default()
        },
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        r.fine,
        4,
        "{:?}",
        r.regions.iter().map(|x| &x.label).collect::<Vec<_>>()
    );
}

fn topic_of(label: &str) -> usize {
    TOPICS
        .iter()
        .enumerate()
        .max_by_key(|(_, w)| w.iter().filter(|x| label.contains(*x)).count())
        .unwrap()
        .0
}

#[test]
fn planted_gaps_are_found_explained_and_keep_their_verdicts() {
    let (_d, mut p, v) = setup();
    let table = select_table(&p, "c", None).unwrap();
    assert!(
        scan(&mut p, &v, &table, &ScanOptions::default()).is_err(),
        "needs regions"
    );
    regions(&mut p, &v);

    let r = scan(&mut p, &v, &table, &ScanOptions::default()).unwrap();
    assert_eq!(r.latest_date.as_deref(), Some("2024-09-01"));
    assert!(
        r.notes.iter().any(|n| n.contains("1 item(s) dated after")),
        "{:?}",
        r.notes
    );
    let kinds: Vec<(GapKind, String)> = r.gaps.iter().map(|g| (g.kind, g.label.clone())).collect();
    // A and D have no coverage at all; B's coverage predates its demand; C is covered recently.
    let coverage: Vec<usize> = r
        .gaps
        .iter()
        .filter(|g| g.kind == GapKind::Coverage)
        .map(|g| topic_of(&g.label))
        .collect();
    assert_eq!(coverage.len(), 2, "{kinds:?}");
    assert!(coverage.contains(&0) && coverage.contains(&3), "{kinds:?}");
    let stale: Vec<_> = r.gaps.iter().filter(|g| g.kind == GapKind::Stale).collect();
    assert_eq!(stale.len(), 1, "{kinds:?}");
    assert_eq!(topic_of(&stale[0].label), 1);
    assert_eq!(stale[0].last_coverage.as_deref(), Some("2016-01-01"));
    assert_eq!(stale[0].demand_since, stale[0].demand);
    assert!(
        stale[0]
            .explanation
            .contains("newer than the latest coverage")
    );
    assert!(
        !r.gaps
            .iter()
            .any(|g| g.kind != GapKind::Bridge && topic_of(&g.label) == 2)
    );
    let bridges: Vec<_> = r
        .gaps
        .iter()
        .filter(|g| g.kind == GapKind::Bridge)
        .collect();
    assert_eq!(bridges.len(), 1, "{kinds:?}");
    assert!(bridges[0].label.contains('↔'));

    // Scores are explained term by term, ranked, and use growing momentum.
    for g in &r.gaps {
        let sum: f64 = g.terms.iter().map(|t| t.contribution).sum();
        assert!((sum - g.score).abs() < 1e-9);
        assert!(!g.explanation.is_empty() && !g.core.is_empty());
    }
    assert!(r.gaps.windows(2).all(|w| w[0].score >= w[1].score));
    assert!(
        r.gaps
            .iter()
            .filter(|g| g.kind == GapKind::Coverage)
            .all(|g| g.momentum.unwrap() > 0.0)
    );
    let a = r
        .gaps
        .iter()
        .find(|g| g.kind == GapKind::Coverage && topic_of(&g.label) == 0)
        .unwrap();
    assert!(a.timeline.iter().map(|y| y.demand).sum::<usize>() == a.demand);

    invectus_gaps::saved::bookmark(&p, &table, a.gap_id).unwrap();
    let bookmarked = invectus_gaps::saved::overview(&p, &table).unwrap();
    assert_eq!(bookmarked.bookmarks.len(), 1);
    assert!(
        bookmarked.decisions.is_empty(),
        "bookmarking must not create a decision"
    );

    // Verdicts: validated, stored, and carried over to the same gap after re-clustering.
    assert!(set_verdict(&p, &table, a.gap_id, "maybe", "").is_err());
    set_verdict(&p, &table, a.gap_id, "confirmed_gap", "checked PubMed").unwrap();
    let d = detail(&p, &table, a.gap_id).unwrap();
    assert_eq!(d.view.verdict.as_ref().unwrap().verdict, "confirmed_gap");
    assert!(d.core_items.len() >= 10 && d.core_items[0].title.is_some());
    // Bookmarks are independent of verdicts, idempotent and persisted across reopening.
    invectus_gaps::saved::bookmark(&p, &table, a.gap_id).unwrap();
    invectus_gaps::saved::bookmark(&p, &table, a.gap_id).unwrap();
    let reopened = Project::open(&p.root).unwrap();
    let saved = invectus_gaps::saved::overview(&reopened, &table).unwrap();
    assert_eq!(saved.bookmarks.len(), 1);
    let serialized = serde_json::to_value(&saved).unwrap();
    let exact_core: Vec<i64> =
        serde_json::from_str(serialized["bookmarks"][0]["core_key"].as_str().unwrap()).unwrap();
    assert_eq!(
        exact_core, saved.bookmarks[0].record.core,
        "opaque keys preserve 63-bit ids through JavaScript"
    );
    assert_eq!(saved.bookmarks[0].gap_id, Some(a.gap_id));
    assert_eq!(saved.decisions.len(), 1);
    assert_eq!(saved.decisions[0].verdict.note, "checked PubMed");
    let mut other_space = table.clone();
    other_space.space_id = "another-embedding-space".into();
    let isolated = invectus_gaps::saved::overview(&reopened, &other_space).unwrap();
    assert!(isolated.bookmarks.is_empty() && isolated.decisions.is_empty());
    let mut other_collection = table.clone();
    other_collection.collection_id = "another-collection".into();
    let isolated = invectus_gaps::saved::overview(&reopened, &other_collection).unwrap();
    assert!(isolated.bookmarks.is_empty() && isolated.decisions.is_empty());
    drop(reopened);

    regions(&mut p, &v);
    let pending_scan = invectus_gaps::saved::overview(&p, &table).unwrap();
    assert_eq!(pending_scan.bookmarks.len(), 1);
    assert!(pending_scan.bookmarks[0].gap_id.is_none());
    assert_eq!(pending_scan.decisions.len(), 1);
    assert!(pending_scan.decisions[0].gap_id.is_none());
    scan(&mut p, &v, &table, &ScanOptions::default()).unwrap();
    let l = list(&p, &table).unwrap();
    let carried: Vec<_> = l.gaps.iter().filter(|g| g.verdict.is_some()).collect();
    assert_eq!(carried.len(), 1);
    assert_eq!(topic_of(&carried[0].gap.label), 0);
    assert_eq!(carried[0].verdict.as_ref().unwrap().note, "checked PubMed");

    let saved = invectus_gaps::saved::overview(&p, &table).unwrap();
    assert_eq!(saved.bookmarks[0].gap_id, Some(carried[0].gap.gap_id));
    let bookmark = &saved.bookmarks[0].record;
    invectus_gaps::saved::remove_bookmark(&p, &table, &bookmark.kind, &bookmark.core).unwrap();
    let saved = invectus_gaps::saved::overview(&p, &table).unwrap();
    assert!(saved.bookmarks.is_empty());
    assert_eq!(
        saved.decisions.len(),
        1,
        "removing a bookmark preserves its decision"
    );
    set_verdict(&p, &table, carried[0].gap.gap_id, "selected", "pursue this").unwrap();
    let saved = invectus_gaps::saved::overview(&p, &table).unwrap();
    assert_eq!(saved.decisions.len(), 1, "show the latest decision per gap");
    assert_eq!(saved.decisions[0].verdict.verdict, "selected");

    // Reports carry titles, verdicts and explanations, never the source text.
    let csv = invectus_gaps::export::csv(&l);
    assert!(csv.starts_with("rank,type,score") && csv.contains("confirmed_gap,checked PubMed"));
    let md = invectus_gaps::export::markdown(&p, "c", &l, 10).unwrap();
    assert!(md.contains("Verdict: confirmed_gap — checked PubMed") && md.contains("item "));
    assert!(
        !md.contains("uniq0x") && !csv.contains("uniq0x"),
        "no item text"
    );
}
