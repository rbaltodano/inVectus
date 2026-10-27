//! Additive multimodal contracts. These are not registered with the text index or verdict API.
//!
//! A new identity is an explicit envelope around an unchanged text identity. Merely adding a
//! field to `EmbeddingIdentity` would change the JSON hashed by old projects. Keep that wire
//! format and its space ids intact; future media tables must opt into this separate identity.

use crate::{Embedding, EmbeddingIdentity, InputKind, TextEmbedder};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Text,
    Image,
    Video,
    Audio,
}

/// The caller provides decoded/validated local content, never a network URL. Media bytes must
/// be verified against their stored source hash before encoding. Decoders enforce their own
/// size limits; the interface does not imply that any particular encoder supports every kind.
#[derive(Clone, Copy, Debug)]
pub enum EmbeddingInput<'a> {
    Text(&'a str),
    Image(&'a Path),
    Video(&'a Path),
    Audio(&'a Path),
}

impl EmbeddingInput<'_> {
    pub fn modality(&self) -> Modality {
        match self {
            Self::Text(_) => Modality::Text,
            Self::Image(_) => Modality::Image,
            Self::Video(_) => Modality::Video,
            Self::Audio(_) => Modality::Audio,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactIdentity {
    pub repo: String,
    pub revision: String,
    pub path: String,
    pub sha256: String,
}

/// All policies that can affect embeddings belong here, including orientation, alpha
/// compositing, resize/filter, RGB/channel order, scaling, text instructions, sampling and
/// segment limits. Keys are sorted so insertion order cannot change a space id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModalityPolicy {
    pub processor_artifact: String,
    pub parameters: BTreeMap<String, String>,
}

/// V1 of a *new* identity format, separate from the unversioned legacy text format. It has no
/// automatic parity exemption: runtime, artifact or policy changes always create a new space.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultimodalIdentity {
    pub version: u32,
    pub text: EmbeddingIdentity,
    /// Named artifacts, e.g. `text_weights`, `vision_weights`, `tokenizer`, `processor`.
    pub artifacts: BTreeMap<String, ArtifactIdentity>,
    pub modalities: BTreeMap<Modality, ModalityPolicy>,
}

impl MultimodalIdentity {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported multimodal identity version {}",
            self.version
        );
        ensure!(
            self.text.dimensions > 0,
            "embedding dimensions must be positive"
        );
        ensure!(!self.artifacts.is_empty(), "no pinned artifacts");
        ensure!(!self.modalities.is_empty(), "no supported modalities");
        for a in self.artifacts.values() {
            ensure!(
                !a.repo.is_empty() && !a.revision.is_empty() && !a.path.is_empty(),
                "artifact must have a pinned source"
            );
            ensure!(
                a.sha256.len() == 64 && a.sha256.bytes().all(|c| c.is_ascii_hexdigit()),
                "artifact must have a SHA-256"
            );
        }
        for policy in self.modalities.values() {
            ensure!(
                self.artifacts.contains_key(&policy.processor_artifact),
                "modality processor is not in the artifact manifest"
            );
            ensure!(
                !policy.parameters.is_empty(),
                "modality preprocessing policy is empty"
            );
        }
        Ok(())
    }

    pub fn space_id(&self) -> Result<String> {
        self.validate()?;
        let mut h = Sha256::new();
        h.update(b"invectus.multimodal.v1\0");
        h.update(serde_json::to_vec(self)?);
        Ok(hex::encode(h.finalize()))
    }

    /// Source-content changes invalidate just that input. Callers hash the complete original
    /// bytes, not the filename or thumbnail. Query/document mode is part of the key.
    pub fn input_hash(
        &self,
        source_sha256: &str,
        modality: Modality,
        kind: InputKind,
    ) -> Result<String> {
        ensure!(
            self.modalities.contains_key(&modality),
            "unsupported modality {modality:?}"
        );
        ensure!(
            source_sha256.len() == 64 && source_sha256.bytes().all(|c| c.is_ascii_hexdigit()),
            "source must have a SHA-256"
        );
        let payload = serde_json::to_vec(&(
            self.space_id()?,
            source_sha256,
            modality,
            match kind {
                InputKind::Query => "query",
                InputKind::Document => "document",
            },
        ))?;
        Ok(hex::encode(Sha256::digest(payload)))
    }
}

/// The type makes the legacy identity explicit; no legacy hash or serialization is rewritten.
#[derive(Clone, Debug)]
pub enum EncoderIdentity<'a> {
    LegacyText(&'a EmbeddingIdentity),
    Multimodal(&'a MultimodalIdentity),
}

/// Capability-aware boundary for future media jobs. Unsupported modes fail before inference.
/// Text search continues to use `TextEmbedder`; this trait does not enable media verdicts.
pub trait MultimodalEmbedder: Send {
    fn encoder_identity(&self) -> EncoderIdentity<'_>;
    fn supports(&self, modality: Modality, kind: InputKind) -> bool;
    fn embed_inputs(
        &mut self,
        inputs: &[EmbeddingInput<'_>],
        kind: InputKind,
    ) -> Result<Vec<Embedding>>;
}

/// Existing text models can participate without modifying their identity or text policy.
pub struct TextAdapter<T>(pub T);

impl<T: TextEmbedder> MultimodalEmbedder for TextAdapter<T> {
    fn encoder_identity(&self) -> EncoderIdentity<'_> {
        EncoderIdentity::LegacyText(self.0.identity())
    }

    fn supports(&self, modality: Modality, _kind: InputKind) -> bool {
        modality == Modality::Text
    }

    fn embed_inputs(
        &mut self,
        inputs: &[EmbeddingInput<'_>],
        kind: InputKind,
    ) -> Result<Vec<Embedding>> {
        let texts: Vec<String> = inputs
            .iter()
            .map(|input| match input {
                EmbeddingInput::Text(text) => Ok((*text).to_owned()),
                EmbeddingInput::Image(_) | EmbeddingInput::Video(_) | EmbeddingInput::Audio(_) => {
                    bail!("text encoder does not support local media")
                }
            })
            .collect::<Result<_>>()?;
        self.0.embed(&texts, kind)
    }
}
