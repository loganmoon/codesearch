//! Search API trait definition
//!
//! This trait defines the interface for all search operations.
//! Implementations can be found in the server crate.

use crate::error::Result;
use async_trait::async_trait;

pub use super::search_models::*;

/// Trait defining search API operations
#[async_trait]
pub trait SearchApi: Send + Sync {
    /// Perform semantic search using vector embeddings
    async fn search_semantic(
        &self,
        request: SemanticSearchRequest,
    ) -> Result<SemanticSearchResponse>;

    /// Query the code graph for relationships
    async fn query_graph(&self, request: GraphQueryRequest) -> Result<GraphQueryResponse>;
}
