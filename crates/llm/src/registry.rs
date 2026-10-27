//! Pinned local language models. Like the embedding registry, every file has a repository
//! revision and SHA-256, and nothing is fetched implicitly.

#[derive(Debug)]
pub struct LlmSpec {
    pub key: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    /// File name in the repository at `revision`.
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub license: &'static str,
    /// Context window used by inVectus (tokens), not necessarily the model's maximum.
    pub context: u32,
    pub note: &'static str,
}

pub const DEFAULT_LLM: &str = "gemma-4-12b-it-qat-q4_0";

pub const MODELS: &[LlmSpec] = &[LlmSpec {
    key: "gemma-4-12b-it-qat-q4_0",
    repo: "google/gemma-4-12B-it-qat-q4_0-gguf",
    revision: "29d097773436b69ff9feafd636ab4cf873786537",
    file: "gemma-4-12b-it-qat-q4_0.gguf",
    bytes: 6_975_879_296,
    sha256: "93567e57a8fe10b23569b9d9ec38cd005deedf71e29477c421a4b83f418a538b",
    license: "apache-2.0",
    context: 8192,
    note: "Google's quantization-aware-trained 4-bit GGUF; text only (vision projector not used)",
}];

pub fn spec(key: &str) -> anyhow::Result<&'static LlmSpec> {
    MODELS.iter().find(|m| m.key == key).ok_or_else(|| {
        let known: Vec<_> = MODELS.iter().map(|m| m.key).collect();
        anyhow::anyhow!(
            "unknown language model {key:?} (known: {})",
            known.join(", ")
        )
    })
}
