//! Pinned embedding models. Every file has a repository commit and SHA-256; nothing is fetched
//! implicitly. Values match `docs/spikes/model-manifest.json`.

use invectus_core::{Normalization, Pooling};

#[derive(Debug)]
pub struct ModelFile {
    /// Path inside the repository at `revision`.
    pub repo_path: &'static str,
    /// File name inside the installed model folder.
    pub local: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

#[derive(Debug)]
pub struct ModelSpec {
    pub key: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    pub files: &'static [ModelFile],
    pub document_prefix: &'static str,
    pub query_prefix: &'static str,
    pub pooling: Pooling,
    pub normalization: Normalization,
    pub max_tokens: usize,
    pub dimensions: usize,
    pub note: &'static str,
}

impl ModelSpec {
    pub fn file(&self, local: &str) -> &ModelFile {
        self.files
            .iter()
            .find(|f| f.local == local)
            .expect("registry entry lists the file")
    }
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }
}

/// Provisional CPU default from the P0 spike (71.95 items/s vs Nomic's 26.69 items/s on the
/// spike sample); retrieval quality has not been compared (SPEC §14 Q1 stays open).
pub const DEFAULT_MODEL: &str = "bge-small-en-v1.5";

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        key: "bge-small-en-v1.5",
        repo: "Xenova/bge-small-en-v1.5",
        revision: "ea104dacec62c0de699686887e3f920caeb4f3e3",
        files: &[
            ModelFile {
                repo_path: "onnx/model.onnx",
                local: "model.onnx",
                bytes: 133_093_490,
                sha256: "828e1496d7fabb79cfa4dcd84fa38625c0d3d21da474a00f08db0f559940cf35",
            },
            ModelFile {
                repo_path: "tokenizer.json",
                local: "tokenizer.json",
                bytes: 711_396,
                sha256: "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66",
            },
            ModelFile {
                repo_path: "config.json",
                local: "config.json",
                bytes: 683,
                sha256: "fa73f90bf92c8cace1fbcb709626306f2bdbc9ea3e5b5f94b440df9b6aa56350",
            },
        ],
        document_prefix: "",
        // BGE v1.5 card: instruction for short queries retrieving passages.
        query_prefix: "Represent this sentence for searching relevant passages: ",
        pooling: Pooling::Cls,
        normalization: Normalization::L2,
        max_tokens: 512,
        dimensions: 384,
        note: "ONNX export of BAAI/bge-small-en-v1.5 (MIT); provisional CPU default",
    },
    ModelSpec {
        key: "nomic-embed-text-v1.5",
        repo: "nomic-ai/nomic-embed-text-v1.5",
        revision: "e9b6763023c676ca8431644204f50c2b100d9aab",
        files: &[
            ModelFile {
                repo_path: "onnx/model.onnx",
                local: "model.onnx",
                bytes: 547_310_275,
                sha256: "147d5aa88c2101237358e17796cf3a227cead1ec304ec34b465bb08e9d952965",
            },
            ModelFile {
                repo_path: "tokenizer.json",
                local: "tokenizer.json",
                bytes: 711_396,
                sha256: "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66",
            },
            ModelFile {
                repo_path: "config.json",
                local: "config.json",
                bytes: 2_538,
                sha256: "9ab00bd92cee80a569f708140b7b6c1661a65891ff3765b1519e181ba2f2c92b",
            },
        ],
        document_prefix: "search_document: ",
        query_prefix: "search_query: ",
        // sentence-transformers configuration of the repository: mean pooling + Normalize.
        pooling: Pooling::Mean,
        normalization: Normalization::L2,
        max_tokens: 512,
        dimensions: 768,
        note: "Apache-2.0; gapmap's model; ~2.7x slower than BGE-small on CPU in P0",
    },
];

pub fn spec(key: &str) -> anyhow::Result<&'static ModelSpec> {
    MODELS.iter().find(|m| m.key == key).ok_or_else(|| {
        let known: Vec<_> = MODELS.iter().map(|m| m.key).collect();
        anyhow::anyhow!("unknown model {key:?} (known: {})", known.join(", "))
    })
}

/// Registry entry for a repository commit (used to find a query encoder for an imported space).
pub fn spec_for_revision(repo: &str, revision: &str) -> Option<&'static ModelSpec> {
    MODELS
        .iter()
        .find(|m| m.repo.eq_ignore_ascii_case(repo) && m.revision == revision)
}
