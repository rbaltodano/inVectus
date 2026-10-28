//! Verification against outside sources (SPEC §6.6): PubMed and OpenAlex connectors with a
//! project cache, and gap verification that re-ranks their titles with the local model.
pub mod connectors;
pub mod gap;

pub use connectors::{ExternalHit, KeywordQuery, Source, SourceResult};
pub use gap::{Verification, VerifyOptions, verify_gap};
