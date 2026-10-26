//! Import: table files, folders and gapmap projects into a project's item store (SPEC §7.2).
pub mod chunk;
pub mod gapmap;
pub mod import;
pub mod mapping;
pub mod source;

pub use chunk::ChunkPolicy;
pub use import::{ImportReport, ImportRequest, Preview, import, preview};
pub use mapping::{Mapping, MetaMapping, RoleMapping};
pub use source::Source;
