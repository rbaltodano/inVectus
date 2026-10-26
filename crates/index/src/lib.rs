//! Vector tables (LanceDB), the embed job, search with exact re-scoring, and parity checks.
pub mod embed_job;
pub mod parity;
pub mod search;
pub mod vectors;

pub use embed_job::{EmbedJobOptions, EmbedReport, run_embed};
pub use search::{SearchMode, SearchOptions, SearchResult, search};
pub use vectors::{VectorRow, VectorStore};

use anyhow::{Result, bail};
use invectus_core::Project;
use invectus_core::store::VectorTableRecord;

/// Pick the vector table to search: the one in `space` (id prefix), or the only one.
pub fn select_table(
    project: &Project,
    collection: &str,
    space: Option<&str>,
) -> Result<VectorTableRecord> {
    project.store.require_collection(collection)?;
    let tables = project.store.vector_tables(Some(collection))?;
    let matching: Vec<_> = tables
        .iter()
        .filter(|t| space.is_none_or(|s| t.space_id.starts_with(s) || t.identity.model_key == s))
        .collect();
    match matching.as_slice() {
        [one] => Ok((*one).clone()),
        [] if tables.is_empty() => {
            bail!("collection {collection:?} has no vectors; run `invectus embed`")
        }
        [] => bail!(
            "no vector table in {collection:?} matches {:?}",
            space.unwrap_or_default()
        ),
        many => bail!(
            "collection {collection:?} has {} vector spaces; choose one with --space ({})",
            many.len(),
            many.iter()
                .map(|t| format!("{} = {}", &t.space_id[..12], t.identity.model_key))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
