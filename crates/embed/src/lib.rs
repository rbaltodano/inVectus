//! Embedding models: pinned registry, explicit installation, offline ONNX inference (SPEC §7.2).
pub mod onnx;
pub mod registry;
pub mod store;

pub use onnx::{EmbedOptions, OnnxEmbedder};
pub use registry::{DEFAULT_MODEL, MODELS, ModelSpec};
pub use store::ModelStore;
