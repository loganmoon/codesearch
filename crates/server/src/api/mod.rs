//! API service layer for search operations
//!
//! This module contains the business logic for search operations,
//! providing a clean interface for the REST API server.

mod agentic_search;
mod embedding_operations;
mod entity_operations;
mod fulltext_search;
mod graph_search;
pub mod models;
mod reranking_helpers;
pub mod search_impl;
mod semantic_search;
mod unified_search;

pub use agentic_search::{
    search_agentic, AgenticSearchApiMetadata, AgenticSearchApiRequest, AgenticSearchApiResponse,
};
pub use embedding_operations::generate_embeddings;
pub use entity_operations::{get_entities_batch, list_repositories};
pub use fulltext_search::search_fulltext;
pub use graph_search::query_graph;
pub use models::*;
pub use search_impl::SearchApiImpl;
pub use semantic_search::search_semantic;
pub use unified_search::search_unified;
