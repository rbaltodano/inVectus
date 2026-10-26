//! Embedding identity: everything that decides which vector space a vector lives in.
//!
//! Two vectors may only be compared when they share a [`EmbeddingIdentity::space_id`]. The id
//! hashes the complete identity (weights checksum, tokenizer, prefixes, pooling, normalization,
//! token policy, dimensions, precision and runtime), so any change creates a new space and a new
//! vector table instead of silently mixing spaces.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pooling {
    /// First token (`[CLS]`) hidden state.
    Cls,
    /// Attention-masked mean over token hidden states.
    Mean,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Normalization {
    /// Unit L2 norm.
    L2,
    /// Parameter-free layer norm over the full dimension, then L2 (Nomic's Matryoshka recipe).
    LayerNormL2,
}

/// Whether text is a stored document or a query; selects the model's prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    Document,
    Query,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingIdentity {
    /// Short registry key, e.g. `bge-small-en-v1.5`.
    pub model_key: String,
    /// Source repository, e.g. `Xenova/bge-small-en-v1.5`.
    pub repo: String,
    /// Pinned repository commit.
    pub revision: String,
    /// SHA-256 of the weights file the vectors were computed with.
    pub weights_sha256: String,
    /// SHA-256 of `tokenizer.json`.
    pub tokenizer_sha256: String,
    /// Prepended to every document before tokenization.
    pub document_prefix: String,
    /// Prepended to every query before tokenization.
    pub query_prefix: String,
    /// How title and body form the document text: `{title}` and `{text}` placeholders.
    pub text_template: String,
    pub pooling: Pooling,
    pub normalization: Normalization,
    /// Tokens per input including special tokens; longer inputs are truncated.
    pub max_tokens: usize,
    /// Truncation side; only `head` (keep the beginning) is implemented.
    pub truncation: String,
    pub dimensions: usize,
    /// Numeric precision of the forward pass, e.g. `fp32`.
    pub compute_precision: String,
    /// Precision of stored vectors, e.g. `f32`.
    pub storage_precision: String,
    /// Inference runtime and device policy, e.g. `onnxruntime-cpu (ort 2.0.0-rc.15, PreferCPU)`.
    pub runtime: String,
}

impl EmbeddingIdentity {
    /// Stable id of the vector space (hex SHA-256 of the canonical JSON of every field).
    pub fn space_id(&self) -> String {
        let canonical = serde_json::to_vec(self).expect("identity serializes");
        hex::encode(Sha256::digest(canonical))
    }

    /// Short form of [`Self::space_id`] used in table names and messages.
    pub fn short_space_id(&self) -> String {
        self.space_id()[..12].to_string()
    }

    /// Document text before the prefix is applied.
    pub fn format_document(&self, title: Option<&str>, text: &str) -> String {
        let title = title.unwrap_or("").trim();
        let text = text.trim();
        if title.is_empty() {
            return text.to_string();
        }
        if text.is_empty() {
            return title.to_string();
        }
        self.text_template
            .replace("{title}", title)
            .replace("{text}", text)
    }

    pub fn prefix(&self, kind: InputKind) -> &str {
        match kind {
            InputKind::Document => &self.document_prefix,
            InputKind::Query => &self.query_prefix,
        }
    }

    /// Hash of exactly what the model sees for a document in this space. The embed job skips
    /// items whose stored hash matches (SPEC §7.6).
    pub fn input_hash(&self, document_text: &str) -> String {
        let mut h = Sha256::new();
        h.update(self.space_id().as_bytes());
        h.update([0]);
        h.update(self.document_prefix.as_bytes());
        h.update(document_text.as_bytes());
        hex::encode(&h.finalize()[..16])
    }

    /// Fields that must agree for a query encoder to be *considered* for another space via a
    /// recorded parity check (same model, tokenizer, text policy, pooling and dimensions; the
    /// weights file, runtime, precision or query prefix may differ).
    pub fn parity_compatible(&self, other: &Self) -> bool {
        self.repo == other.repo
            && self.revision == other.revision
            && self.tokenizer_sha256 == other.tokenizer_sha256
            && self.document_prefix == other.document_prefix
            && self.text_template == other.text_template
            && self.pooling == other.pooling
            && self.normalization == other.normalization
            && self.max_tokens == other.max_tokens
            && self.dimensions == other.dimensions
    }
}

#[cfg(test)]
pub(crate) fn test_identity() -> EmbeddingIdentity {
    EmbeddingIdentity {
        model_key: "test".into(),
        repo: "example/test".into(),
        revision: "abc".into(),
        weights_sha256: "00".into(),
        tokenizer_sha256: "11".into(),
        document_prefix: String::new(),
        query_prefix: "query: ".into(),
        text_template: "{title}. {text}".into(),
        pooling: Pooling::Cls,
        normalization: Normalization::L2,
        max_tokens: 512,
        truncation: "head".into(),
        dimensions: 8,
        compute_precision: "fp32".into(),
        storage_precision: "f32".into(),
        runtime: "test".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_changes_the_space() {
        let base = test_identity();
        let id = base.space_id();
        assert_eq!(id, test_identity().space_id(), "space id is deterministic");
        let variants: Vec<Box<dyn Fn(&mut EmbeddingIdentity)>> = vec![
            Box::new(|i| i.revision = "def".into()),
            Box::new(|i| i.weights_sha256 = "01".into()),
            Box::new(|i| i.tokenizer_sha256 = "12".into()),
            Box::new(|i| i.document_prefix = "search_document: ".into()),
            Box::new(|i| i.query_prefix = String::new()),
            Box::new(|i| i.pooling = Pooling::Mean),
            Box::new(|i| i.normalization = Normalization::LayerNormL2),
            Box::new(|i| i.max_tokens = 256),
            Box::new(|i| i.dimensions = 16),
            Box::new(|i| i.compute_precision = "fp16".into()),
            Box::new(|i| i.runtime = "other".into()),
        ];
        for change in variants {
            let mut other = test_identity();
            change(&mut other);
            assert_ne!(other.space_id(), id);
        }
    }

    #[test]
    fn input_hash_depends_on_space_and_text() {
        let a = test_identity();
        let mut b = test_identity();
        b.pooling = Pooling::Mean;
        assert_eq!(a.input_hash("x"), a.input_hash("x"));
        assert_ne!(a.input_hash("x"), a.input_hash("y"));
        assert_ne!(a.input_hash("x"), b.input_hash("x"));
    }

    #[test]
    fn document_format_handles_missing_parts() {
        let i = test_identity();
        assert_eq!(i.format_document(Some("T"), "body"), "T. body");
        assert_eq!(i.format_document(None, "body"), "body");
        assert_eq!(i.format_document(Some("T"), "  "), "T");
    }

    #[test]
    fn parity_compatibility_ignores_runtime_only() {
        let a = test_identity();
        let mut b = test_identity();
        b.runtime = "sentence-transformers".into();
        b.weights_sha256 = "ff".into();
        b.compute_precision = "fp16".into();
        assert!(a.parity_compatible(&b));
        b.document_prefix = "clustering: ".into();
        assert!(!a.parity_compatible(&b));
    }
}
