//! Backtest end to end: coverage that arrives after the cutoff fills its region's gap.

use anyhow::Result;
use invectus_core::item::item_id;
use invectus_core::{
    CancelToken, Embedding, EmbeddingIdentity, InputKind, Item, Normalization, Pooling, Project,
    Role, TextEmbedder,
};
use invectus_eval::{BacktestOptions, backtest};
use invectus_gaps::ScanOptions;
use invectus_index::{EmbedJobOptions, VectorStore, run_embed, select_table};
use invectus_regions::RegionOptions;
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

#[test]
fn coverage_after_the_cutoff_fills_its_gap() {
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
            let date = format!("{}-06-01", 2014 + i / 4);
            batch.push((item(t, i, Role::Demand, &date), vec![]));
        }
    }
    // Topic 0 gets reviews two years after the cutoff; the others never do.
    for i in 28..31 {
        // Reviews cover the whole topic, so they sit near the region's centre.
        let mut review = item(0, i, Role::Coverage, "2022-05-01");
        review.text = format!("{} review{i}", TOPICS[0].join(" "));
        batch.push((review, vec![]));
    }
    // Undated items are left out of every past.
    let mut undated = item(1, 40, Role::Demand, "x");
    undated.date = None;
    batch.push((undated, vec![]));
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
    let table = select_table(&p, "c", None).unwrap();
    let r = backtest(
        &p,
        &v,
        &table,
        &BacktestOptions {
            cutoffs: vec!["2020-12-31".into(), "2023-12-31".into()],
            horizon_years: 2,
            regions: RegionOptions {
                k: 10,
                target_regions: Some(4),
                ..Default::default()
            },
            scan: ScanOptions::default(),
        },
    )
    .unwrap();
    let c = &r.cutoffs[0];
    assert_eq!(c.items, 112, "only dated items up to the cutoff");
    assert_eq!(
        (c.regions, c.gaps, c.filled, c.future_coverage),
        (4, 4, 1, 3),
        "{c:?}"
    );
    assert_eq!(c.base_rate, 0.25);
    assert!(c.model.auc.is_finite() && c.raw_count.auc.is_finite());
    // After 2023 the reviews are in the past: nothing new arrives, so that cutoff is not
    // informative and is left out of the means.
    let later = &r.cutoffs[1];
    assert_eq!(
        (later.items, later.filled, later.future_coverage),
        (115, 0, 0)
    );
    assert!(r.mean_model.is_some());
    assert!(r.notes.iter().any(|n| n.contains("left out of the means")));
}
