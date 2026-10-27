//! A model's tokenizer on its own, for sizing chunks in tokens at import time.

use crate::registry;
use crate::store::ModelStore;
use anyhow::{Result, anyhow};
use invectus_core::TokenOffsets;
use tokenizers::Tokenizer;

pub struct ModelTokenizer {
    key: &'static str,
    sha256: &'static str,
    tokenizer: Tokenizer,
}

impl ModelTokenizer {
    /// Load the installed, checksum-verified tokenizer of registry model `key` (no network).
    pub fn load(store: &ModelStore, key: &str) -> Result<Self> {
        let spec = registry::spec(key)?;
        let dir = store.verified_dir(spec)?;
        let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow!("loading tokenizer: {e}"))?;
        // Count every token: no truncation, no padding.
        tokenizer
            .with_truncation(None)
            .map_err(|e| anyhow!(e))?
            .with_padding(None);
        Ok(Self {
            key: spec.key,
            sha256: spec.file("tokenizer.json").sha256,
            tokenizer,
        })
    }
}

impl TokenOffsets for ModelTokenizer {
    fn model_key(&self) -> &str {
        self.key
    }

    fn tokenizer_sha256(&self) -> &str {
        self.sha256
    }

    fn offsets(&self, text: &str) -> Result<Vec<(usize, usize)>> {
        let enc = self
            .tokenizer
            .encode(text, false)
            .map_err(|e| anyhow!("tokenizing: {e}"))?;
        Ok(enc.get_offsets().to_vec())
    }
}
