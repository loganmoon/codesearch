//! Request and response models for API operations

// Re-export search models from core
pub use codesearch_core::search_models::*;

use codesearch_core::config::{
    HybridSearchConfig, QueryPreprocessingConfig, RerankingConfig, SparseEmbeddingsConfig,
    SpecificityConfig,
};
use codesearch_embeddings::EmbeddingManager;
use codesearch_reranking::RerankerProvider;
use codesearch_storage::{
    Neo4jClientTrait, PostgresClientTrait, SearchFilters as StorageSearchFilters, StorageClient,
};
use std::sync::Arc;

/// Container for backend storage and infrastructure clients
pub struct BackendClients {
    pub postgres: Arc<dyn PostgresClientTrait>,
    pub qdrant: Arc<dyn StorageClient>,
    pub neo4j: Option<Arc<dyn Neo4jClientTrait>>,
    pub embedding_manager: Arc<EmbeddingManager>,
    pub reranker: Option<Arc<dyn RerankerProvider>>,
}

/// Container for search configuration
pub struct SearchConfig {
    pub hybrid_search: HybridSearchConfig,
    pub reranking: RerankingConfig,
    pub query_preprocessing: QueryPreprocessingConfig,
    pub specificity: SpecificityConfig,
    pub sparse_embeddings: SparseEmbeddingsConfig,
    pub default_bge_instruction: String,
    pub max_batch_size: usize,
}

/// Convert API search filters to storage search filters
pub fn build_storage_filters(filters: &Option<SearchFilters>) -> Option<StorageSearchFilters> {
    filters.as_ref().map(|f| StorageSearchFilters {
        entity_types: f.entity_type.clone(),
        language: f.language.clone(),
        file_path: f.file_path.as_ref().map(std::path::PathBuf::from),
    })
}
