//! Verification against outside sources (SPEC §6.6): PubMed and OpenAlex connectors with a
//! project cache, and gap verification that re-ranks their titles with the local model.
pub mod connectors;
pub mod gap;

pub use connectors::{
    CacheUsage, ExternalHit, KeywordQuery, Source, SourceResult, cache_usage, clear_cache,
};
pub use gap::{Fetched, Prepared, Verification, VerifyOptions, fetch, finish, prepare, verify_gap};
