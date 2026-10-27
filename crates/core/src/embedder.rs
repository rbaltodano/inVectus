//! The boundary between engine code and an embedding model.

use crate::identity::{EmbeddingIdentity, InputKind};
use anyhow::Result;

/// One embedded input.
#[derive(Clone, Debug)]
pub struct Embedding {
    /// Unit-length vector of `identity().dimensions` floats.
    pub vector: Vec<f32>,
    /// Tokens after truncation, including special tokens.
    pub tokens: usize,
    /// The input exceeded `max_tokens` and was truncated.
    pub truncated: bool,
}

/// Token boundaries from a model's tokenizer, used to size chunks in tokens.
pub trait TokenOffsets {
    /// Registry key of the model whose tokenizer this is.
    fn model_key(&self) -> &str;
    /// SHA-256 of the tokenizer file; recorded in the chunk policy.
    fn tokenizer_sha256(&self) -> &str;
    /// Byte ranges of the tokens of `text` in order, without special tokens.
    fn offsets(&self, text: &str) -> Result<Vec<(usize, usize)>>;
}

/// A loaded model that embeds text into exactly one vector space.
pub trait TextEmbedder: Send {
    fn identity(&self) -> &EmbeddingIdentity;
    /// Embed texts; the implementation applies the identity's document or query prefix.
    fn embed(&mut self, texts: &[String], kind: InputKind) -> Result<Vec<Embedding>>;
}
