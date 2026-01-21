//! Pipeline stage functions for repository indexing
//!
//! The indexing pipeline consists of 5 stages that run concurrently:
//! 1. **Discovery** - Find all files to index
//! 2. **Extract** - Extract code entities from files
//! 3. **Embed** - Generate dense and sparse embeddings
//! 4. **Store** - Store entities in the database
//! 5. **Snapshots** - Update file snapshots and mark stale entities

mod discovery;
mod embed;
mod extract;
mod snapshots;
mod store;

pub(crate) use discovery::stage_file_discovery;
pub(crate) use embed::stage_generate_embeddings;
pub(crate) use extract::stage_extract_entities;
pub(crate) use snapshots::stage_update_snapshots;
pub(crate) use store::stage_store_entities;

// Re-export for tests
#[cfg(test)]
pub(crate) use extract::create_crate_root_entities;
