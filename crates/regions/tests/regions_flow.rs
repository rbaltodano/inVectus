//! Region job end to end with a deterministic bag-of-words embedder and planted topics.

use anyhow::Result;
use invectus_core::item::item_id;
use invectus_core::{
    CancelToken, Embedding, EmbeddingIdentity, InputKind, Item, Normalization, Pooling, Project,
    Role, TextEmbedder,
};
use invectus_index::{EmbedJobOptions, VectorStore, run_embed, select_table};
use invectus_regions::{RegionOptions, map_data, nearest_regions, run_regions};
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
        "myopia",
        "atropine",
        "orthokeratology",
        "axial",
        "children",
        "spectacle",
        "refraction",
        "elongation",
    ],
];

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
    for (t, words) in TOPICS.iter().enumerate() {
        for i in 0..30 {
            // Five of the topic's eight words, rotating, plus a word unique to the item.
            let text: Vec<&str> = (0..5).map(|j| words[(i + j * 3) % 8]).collect();
            let text = format!("{} uniq{t}x{i}", text.join(" "));
            let src = format!("{t}-{i}");
            batch.push((
                Item {
                    item_id: item_id("c", &src, None),
                    collection_id: "c".into(),
                    source_id: src.clone(),
                    title: Some(format!("{} report {i}", words[i % 8])),
                    input_hash: text.clone(),
                    text,
                    url: None,
                    date: None,
                    role: Role::Neutral,
                    meta: serde_json::Value::Null,
                    parent_id: None,
                    chunk_index: None,
                },
                vec![],
            ));
        }
    }
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

#[test]
fn planted_topics_become_stable_labelled_regions_with_a_map() {
    let (_d, mut p, v) = setup();
    let table = select_table(&p, "c", None).unwrap();
    let opts = RegionOptions {
        k: 10,
        resolution: Some(1.0),
        ..Default::default()
    };
    let mut phases = Vec::new();
    let r = run_regions(
        &mut p,
        &v,
        &table,
        &opts,
        &CancelToken::default(),
        &mut |pr| phases.push(pr.phase.clone()),
    )
    .unwrap();
    assert_eq!(r.items, 120);
    assert_eq!(r.fine, 4, "{:?}", r.regions);
    assert_eq!(r.noise, 0);
    assert_eq!(r.stable, 4);
    assert!(r.mean_stability > 0.95 && r.modularity > 0.5);
    assert!(r.trustworthiness_at_15 > 0.8, "{}", r.trustworthiness_at_15);
    assert!(phases.contains(&"cluster".to_string()) && phases.contains(&"layout".to_string()));
    // Each fine region's label comes from one planted topic.
    for region in r.regions.iter().filter(|x| x.level == "fine") {
        let topic = TOPICS
            .iter()
            .position(|w| w.contains(&region.terms[0].as_str()))
            .unwrap_or_else(|| panic!("{:?}", region.terms));
        assert!(
            region
                .terms
                .iter()
                .take(3)
                .all(|t| TOPICS[topic].contains(&t.as_str()))
        );
        assert_eq!(region.size, 30);
        assert!(region.parent_id.is_some());
    }
    assert!(r.regions.iter().any(|x| x.level == "broad"));

    // Stored map: one point per item, inside its region's circle.
    let map = map_data(&p, &table).unwrap();
    assert_eq!(map.points.len(), 120);
    for pt in &map.points {
        let reg = map
            .regions
            .iter()
            .find(|x| x.level == "fine" && x.region_id == pt.fine_id)
            .unwrap();
        let d = (f64::from(pt.x) - reg.x).hypot(f64::from(pt.y) - reg.y);
        assert!(d <= reg.radius + 1e-3);
    }
    // Stored centroids answer "which region is this idea in".
    let mut e = BagOfWords(identity());
    let q = e
        .embed(
            &["glaucoma trabeculectomy drainage".into()],
            InputKind::Query,
        )
        .unwrap()
        .remove(0)
        .vector;
    let near = nearest_regions(&map.regions, &q, "fine", 2);
    assert!(near[0].label.contains("glaucoma") || near[0].label.contains("trabeculectomy"));

    // Automatic resolution finds the planted count when asked for it.
    let auto = RegionOptions {
        k: 10,
        target_regions: Some(4),
        ..Default::default()
    };
    let ra = run_regions(
        &mut p,
        &v,
        &table,
        &auto,
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        (ra.fine, ra.target_regions),
        (4, Some(4)),
        "{}",
        ra.resolution
    );

    // A rerun replaces the regions; a cancelled run keeps them.
    let r2 = run_regions(
        &mut p,
        &v,
        &table,
        &opts,
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_ne!(r2.run_id, r.run_id);
    assert_eq!(map_data(&p, &table).unwrap().points.len(), 120);
    let cancel = CancelToken::default();
    cancel.cancel();
    let r3 = run_regions(&mut p, &v, &table, &opts, &cancel, &mut |_| {}).unwrap();
    assert!(r3.cancelled);
    assert_eq!(map_data(&p, &table).unwrap().run.run_id, r2.run_id);
}
