//! Data model, project store and schema contracts shared by every inVectus crate (SPEC §7.2).
pub mod embedder;
pub mod identity;
pub mod item;
pub mod job;
pub mod multimodal;
pub mod net;
pub mod project;
pub mod store;

pub use embedder::{Embedding, TextEmbedder, TokenOffsets};
pub use identity::{EmbeddingIdentity, InputKind, Normalization, Pooling};
pub use item::{Item, Role};
pub use job::{CancelToken, Progress};
pub use project::Project;
pub use store::Store;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
