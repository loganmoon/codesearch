//! Implementation of SearchApi trait for the server

use async_trait::async_trait;
use codesearch_core::{error::Result, search_models::*, SearchApi};
use std::sync::Arc;

use super::models::{BackendClients, SearchConfig};

/// Implementation of the SearchApi trait backed by storage and embedding services
pub struct SearchApiImpl {
    clients: Arc<BackendClients>,
    config: Arc<SearchConfig>,
}

impl SearchApiImpl {
    /// Create a new SearchApiImpl instance
    pub fn new(clients: Arc<BackendClients>, config: Arc<SearchConfig>) -> Self {
        Self { clients, config }
    }
}

#[async_trait]
impl SearchApi for SearchApiImpl {
    async fn search_semantic(
        &self,
        request: SemanticSearchRequest,
    ) -> Result<SemanticSearchResponse> {
        crate::api::semantic_search::search_semantic(request, &self.clients, &self.config).await
    }

    async fn query_graph(&self, request: GraphQueryRequest) -> Result<GraphQueryResponse> {
        let neo4j_client = self
            .clients
            .neo4j
            .as_ref()
            .ok_or_else(|| codesearch_core::Error::storage("Neo4j not available"))?;

        crate::api::graph_search::query_graph(
            request,
            neo4j_client,
            &self.clients.postgres,
            &self.clients.reranker,
        )
        .await
    }
}
