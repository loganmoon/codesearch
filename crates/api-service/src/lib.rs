//! Service layer for codesearch API operations
//!
//! This crate contains the business logic for search operations,
//! providing a clean interface for the REST API server.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

mod embedding_operations;
mod entity_operations;
mod fulltext_search;
mod graph_search;
mod models;
mod semantic_search;
mod unified_search;

pub use embedding_operations::generate_embeddings;
pub use entity_operations::{get_entities_batch, list_repositories};
pub use fulltext_search::search_fulltext;
pub use graph_search::query_graph;
pub use models::*;
pub use semantic_search::search_semantic;
pub use unified_search::search_unified;
