use invectus_core::multimodal::*;
use invectus_core::store::VectorTableRecord;
use invectus_core::{Embedding, EmbeddingIdentity, InputKind, Project, TextEmbedder};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Deserialize)]
struct LegacySpace {
    space_id: String,
    identity: EmbeddingIdentity,
}

fn legacy() -> Vec<LegacySpace> {
    serde_json::from_str(include_str!("fixtures/legacy-spaces.json")).unwrap()
}

fn identity() -> MultimodalIdentity {
    MultimodalIdentity {
        version: 1,
        text: legacy().remove(0).identity,
        artifacts: BTreeMap::from([(
            "processor".into(),
            ArtifactIdentity {
                repo: "example/model".into(),
                revision: "pinned".into(),
                path: "processor.json".into(),
                sha256: "a".repeat(64),
            },
        )]),
        modalities: BTreeMap::from([(
            Modality::Image,
            ModalityPolicy {
                processor_artifact: "processor".into(),
                parameters: BTreeMap::from([
                    ("resize".into(), "224x224-bilinear".into()),
                    ("alpha".into(), "white".into()),
                ]),
            },
        )]),
    }
}

#[test]
fn real_legacy_space_ids_survive_serialization_and_project_reopening() {
    // These exact identities and space ids were read from the pre-change demo project.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("legacy.invectus");
    let project = Project::create(&path).unwrap();
    for fixture in legacy() {
        assert_eq!(fixture.identity.space_id(), fixture.space_id);
        let json = serde_json::to_string(&fixture.identity).unwrap();
        let roundtrip: EmbeddingIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.space_id(), fixture.space_id);
        project
            .store
            .register_vector_table(&VectorTableRecord {
                collection_id: "legacy".into(),
                table_name: format!("legacy__{}", fixture.identity.short_space_id()),
                space_id: fixture.space_id,
                identity: fixture.identity,
                origin: "embedded".into(),
                ann: None,
            })
            .unwrap();
    }
    drop(project);
    let project = Project::open(&path).unwrap();
    let tables = project.store.vector_tables(Some("legacy")).unwrap();
    assert_eq!(tables.len(), legacy().len());
    for table in tables {
        assert_eq!(table.identity.space_id(), table.space_id);
        assert!(legacy().iter().any(|l| l.space_id == table.space_id));
    }
}

#[test]
fn multimodal_identity_is_versioned_and_artifact_policy_sensitive() {
    let base = identity();
    let id = base.space_id().unwrap();
    assert_ne!(id, base.text.space_id());
    let roundtrip: MultimodalIdentity =
        serde_json::from_str(&serde_json::to_string(&base).unwrap()).unwrap();
    assert_eq!(id, roundtrip.space_id().unwrap());
    type Change = Box<dyn Fn(&mut MultimodalIdentity)>;
    for change in [
        Box::new(|i: &mut MultimodalIdentity| {
            i.artifacts.get_mut("processor").unwrap().sha256 = "b".repeat(64)
        }) as Change,
        Box::new(|i| {
            i.modalities
                .get_mut(&Modality::Image)
                .unwrap()
                .parameters
                .insert("alpha".into(), "black".into());
        }),
        Box::new(|i| i.text.runtime = "another runtime".into()),
        Box::new(|i| i.text.dimensions += 1),
    ] {
        let mut other = base.clone();
        change(&mut other);
        assert_ne!(id, other.space_id().unwrap());
    }
    let mut future = base.clone();
    future.version = 2;
    assert!(future.space_id().is_err());
    let mut missing = base.clone();
    missing.artifacts.clear();
    assert!(missing.space_id().is_err());
    let mut unpinned = base.clone();
    unpinned.artifacts.get_mut("processor").unwrap().sha256 = "unrecorded".into();
    assert!(unpinned.space_id().is_err());
}

#[test]
fn content_mode_and_modality_control_invalidation() {
    let i = identity();
    let a = i
        .input_hash(&"a".repeat(64), Modality::Image, InputKind::Document)
        .unwrap();
    assert_eq!(
        a,
        i.input_hash(&"a".repeat(64), Modality::Image, InputKind::Document)
            .unwrap()
    );
    assert_ne!(
        a,
        i.input_hash(&"b".repeat(64), Modality::Image, InputKind::Document)
            .unwrap()
    );
    assert_ne!(
        a,
        i.input_hash(&"a".repeat(64), Modality::Image, InputKind::Query)
            .unwrap()
    );
    assert!(
        i.input_hash(&"a".repeat(64), Modality::Audio, InputKind::Query)
            .is_err()
    );
}

struct TextModel {
    identity: EmbeddingIdentity,
    calls: usize,
}
impl TextEmbedder for TextModel {
    fn identity(&self) -> &EmbeddingIdentity {
        &self.identity
    }
    fn embed(&mut self, texts: &[String], kind: InputKind) -> anyhow::Result<Vec<Embedding>> {
        self.calls += 1;
        assert_eq!(kind, InputKind::Query);
        assert_eq!(texts, &["original query"]);
        Ok(vec![Embedding {
            vector: vec![1.0],
            tokens: 2,
            truncated: false,
        }])
    }
}

#[test]
fn legacy_adapter_keeps_identity_and_rejects_media_before_any_inference() {
    let identity = legacy().remove(0).identity;
    let id = identity.space_id();
    let mut adapter = TextAdapter(TextModel { identity, calls: 0 });
    let EncoderIdentity::LegacyText(i) = adapter.encoder_identity() else {
        panic!("wrong identity kind")
    };
    assert_eq!(i.space_id(), id);
    assert!(adapter.supports(Modality::Text, InputKind::Query));
    for input in [
        EmbeddingInput::Image(Path::new("private.png")),
        EmbeddingInput::Audio(Path::new("private.wav")),
        EmbeddingInput::Video(Path::new("private.mp4")),
    ] {
        assert!(!adapter.supports(input.modality(), InputKind::Query));
        assert!(
            adapter
                .embed_inputs(
                    &[EmbeddingInput::Text("original query"), input],
                    InputKind::Query
                )
                .is_err()
        );
    }
    assert_eq!(adapter.0.calls, 0);
    let out = adapter
        .embed_inputs(&[EmbeddingInput::Text("original query")], InputKind::Query)
        .unwrap();
    assert_eq!(out[0].vector, vec![1.0]);
    assert_eq!(adapter.0.calls, 1);
}
