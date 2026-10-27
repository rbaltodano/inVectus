//! Gap descriptions end to end with a bag-of-words embedder and a scripted language model.

use anyhow::Result;
use invectus_core::item::item_id;
use invectus_core::{
    CancelToken, Embedding, EmbeddingIdentity, InputKind, Item, Normalization, Pooling, Project,
    Role, TextEmbedder,
};
use invectus_index::{EmbedJobOptions, VectorStore, run_embed, select_table};
use invectus_llm::{GenOptions, Generation, LlmIdentity, TextGenerator};
use invectus_midpoint::{DescribeOptions, Target, describe};
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

/// Replies from a script, recording the prompts it saw.
struct Script {
    replies: Vec<String>,
    prompts: Vec<String>,
    identity: LlmIdentity,
}

impl TextGenerator for Script {
    fn identity(&self) -> &LlmIdentity {
        &self.identity
    }
    fn generate(&mut self, _: Option<&str>, user: &str, _: &GenOptions) -> Result<Generation> {
        self.prompts.push(user.to_string());
        Ok(Generation {
            text: self.replies.remove(0),
            prompt_tokens: 0,
            generated_tokens: 0,
            seconds: 0.0,
            truncated: false,
        })
    }
}

fn script(replies: &[&str]) -> Script {
    Script {
        replies: replies.iter().map(|s| s.to_string()).collect(),
        prompts: Vec::new(),
        identity: LlmIdentity {
            model_key: "script".into(),
            repo: "test".into(),
            revision: "1".into(),
            sha256: "0".into(),
            runtime: "test".into(),
        },
    }
}

#[test]
fn descriptions_are_checked_and_retried_and_stored() {
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
            batch.push((item(t, i, Role::Demand, "2020-01-01"), vec![]));
        }
    }
    p.store.write_items(&batch).unwrap();
    let v = VectorStore::open(&p).unwrap();
    let mut e = BagOfWords(identity());
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
    let table = select_table(&p, "c", None).unwrap();
    let r = run_regions(
        &mut p,
        &v,
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
    let region_of = |topic: usize| {
        r.regions
            .iter()
            .find(|x| {
                x.level == "fine" && x.terms.iter().any(|t| TOPICS[topic].contains(&t.as_str()))
            })
            .unwrap()
            .region_id
    };
    let (a, b) = (region_of(0), region_of(1));

    // Between two regions: one side's words only is off target; both sides' words are on it.
    let mut g = script(&[
        "Title: Keratoconus crosslinking\nDescription: keratoconus crosslinking cornea ectasia riboflavin topography",
        "**Title:** Crosslinking and glaucoma drainage\nDescription: keratoconus crosslinking cornea ectasia glaucoma trabeculectomy pressure drainage",
    ]);
    let out = describe(
        &p,
        &v,
        &table,
        &mut e,
        &mut g,
        &Target::Between { a, b, t: 0.5 },
        &DescribeOptions::default(),
    )
    .unwrap();
    assert_eq!(out.attempts.len(), 2);
    assert!(
        !out.attempts[0].on_target.passed,
        "{:?}",
        out.attempts[0].on_target
    );
    assert_eq!(out.accepted, Some(1), "{:?}", out.attempts[1].on_target);
    assert_eq!(out.attempts[1].title, "Crosslinking and glaucoma drainage");
    assert!(out.attempts[1].novelty.is_none());
    assert!(out.notes.iter().any(|n| n.contains("no calibration")));
    // The prompt shows both sides' titles and never the item text.
    assert!(g.prompts[0].contains("Side A") && g.prompts[0].contains("Side B"));
    assert!(!g.prompts[0].contains("uniq0x"), "no item text in prompts");

    // One region's missing coverage: on target when its nearest region is that region.
    let mut g = script(&[
        "Title: Review of myopia control\nDescription: myopia atropine orthokeratology axial children spectacle",
    ]);
    let out = describe(
        &p,
        &v,
        &table,
        &mut e,
        &mut g,
        &Target::Region {
            region: region_of(3),
        },
        &DescribeOptions::default(),
    )
    .unwrap();
    assert_eq!(out.accepted, Some(0), "{:?}", out.attempts[0].on_target);
    assert!(
        g.prompts[0].contains("(none)"),
        "no coverage items exist yet"
    );

    // Everything was stored with the model identities.
    let stored = p.store.midpoints("c", &table.space_id).unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].1["llm"]["model_key"], "script");
    assert!(
        describe(
            &p,
            &v,
            &table,
            &mut e,
            &mut script(&[]),
            &Target::Between { a, b, t: 1.5 },
            &DescribeOptions::default()
        )
        .is_err()
    );
}
