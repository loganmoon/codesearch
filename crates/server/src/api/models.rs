//! Request and response models for API operations

use codesearch_core::config::{HybridSearchConfig, RerankingConfig, RerankingRequestConfig};
use codesearch_core::entities::{
    EntityType, FunctionSignature, Language, SourceLocation, Visibility,
};
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use codesearch_embeddings::EmbeddingManager;
use codesearch_reranking::RerankerProvider;
use codesearch_storage::{
    Neo4jClientTrait, PostgresClientTrait, SearchFilters as StorageSearchFilters, StorageClient,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

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
    pub default_bge_instruction: String,
}

/// Query specification with text and optional pre-computed embedding
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct QuerySpec {
    pub text: String,
    pub instruction: Option<String>,
    pub embedding: Option<Vec<f32>>,
}

/// Search filters for entity matching
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct SearchFilters {
    #[cfg_attr(feature = "openapi", schema(value_type = Option<Vec<EntityType>>))]
    pub entity_type: Option<Vec<EntityType>>,
    pub language: Option<String>,
    pub file_path: Option<String>,
    pub implements_trait: Option<String>,
    pub called_by: Option<String>,
    pub calls: Option<String>,
    pub in_module: Option<String>,
}

/// Semantic search request
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct SemanticSearchRequest {
    pub repository_ids: Option<Vec<Uuid>>,
    pub query: QuerySpec,
    pub filters: Option<SearchFilters>,
    pub limit: usize,
    pub prefetch_multiplier: Option<usize>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub rerank: Option<RerankingRequestConfig>,
}

/// Entity result with score and metadata
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct EntityResult {
    pub entity_id: String,
    pub repository_id: Uuid,
    pub qualified_name: String,
    pub name: String,
    pub entity_type: EntityType,
    pub language: Language,
    pub file_path: String,
    pub location: SourceLocation,
    pub content: Option<String>,
    pub signature: Option<FunctionSignature>,
    pub documentation_summary: Option<String>,
    pub visibility: Visibility,
    pub score: f32,
    pub reranked: bool,
}

impl TryFrom<CodeEntity> for EntityResult {
    type Error = Error;

    fn try_from(entity: CodeEntity) -> Result<Self> {
        let repository_id = Uuid::parse_str(&entity.repository_id).map_err(|e| {
            Error::invalid_input(format!(
                "Invalid repository UUID '{}': {}",
                entity.repository_id, e
            ))
        })?;

        Ok(Self {
            entity_id: entity.entity_id.clone(),
            repository_id,
            qualified_name: entity.qualified_name.clone(),
            name: entity.name.clone(),
            entity_type: entity.entity_type,
            language: entity.language,
            file_path: entity.file_path.display().to_string(),
            location: entity.location,
            content: entity.content.clone(),
            signature: entity.signature.clone(),
            documentation_summary: entity.documentation_summary.clone(),
            visibility: entity.visibility,
            score: 0.0,
            reranked: false,
        })
    }
}

/// Response metadata for semantic search
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ResponseMetadata {
    pub total_results: usize,
    pub repositories_searched: usize,
    pub reranked: bool,
    pub query_time_ms: u64,
}

/// Semantic search response
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct SemanticSearchResponse {
    pub results: Vec<EntityResult>,
    pub metadata: ResponseMetadata,
}

/// Full-text search request
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct FulltextSearchRequest {
    pub repository_id: Uuid,
    pub query: String,
    pub limit: usize,
}

/// Full-text search response
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct FulltextSearchResponse {
    pub results: Vec<EntityResult>,
    pub metadata: ResponseMetadata,
}

/// Unified search request (combines full-text + semantic)
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct UnifiedSearchRequest {
    pub repository_id: Uuid,
    pub query: QuerySpec,
    pub filters: Option<SearchFilters>,
    pub limit: usize,
    pub enable_fulltext: bool,
    pub enable_semantic: bool,
    pub fulltext_limit: Option<usize>,
    pub semantic_limit: Option<usize>,
    pub rrf_k: Option<usize>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub rerank: Option<RerankingRequestConfig>,
}

/// Response metadata for unified search
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct UnifiedResponseMetadata {
    pub total_results: usize,
    pub fulltext_count: usize,
    pub semantic_count: usize,
    pub merged_via_rrf: bool,
    pub reranked: bool,
    pub query_time_ms: u64,
}

/// Unified search response
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct UnifiedSearchResponse {
    pub results: Vec<EntityResult>,
    pub metadata: UnifiedResponseMetadata,
}

/// Graph query types
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum GraphQueryType {
    FindFunctionCallers,
    FindFunctionCallees,
    FindTraitImplementations,
    FindClassHierarchy,
    FindModuleContents,
    FindModuleDependencies,
    FindUnusedFunctions,
    FindCircularDependencies,
}

/// Graph query parameters
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct GraphQueryParameters {
    pub qualified_name: String,
    pub max_depth: Option<usize>,
}

/// Graph query request
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct GraphQueryRequest {
    pub repository_id: Uuid,
    pub query_type: GraphQueryType,
    pub parameters: GraphQueryParameters,
    pub return_entities: bool,
    pub semantic_filter: Option<String>,
    pub limit: usize,
}

/// Graph result with optional full entity
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct GraphResult {
    pub qualified_name: String,
    pub relevance_score: Option<f32>,
    pub entity: Option<EntityResult>,
}

/// Graph query response
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct GraphQueryResponse {
    pub results: Vec<GraphResult>,
    pub metadata: GraphResponseMetadata,
}

/// Response metadata for graph queries
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct GraphResponseMetadata {
    pub total_results: usize,
    pub semantic_filter_applied: bool,
    pub query_time_ms: u64,
    pub warning: Option<String>,
}

/// Batch entity request
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct BatchEntityRequest {
    pub entity_refs: Vec<(Uuid, String)>,
}

/// Batch entity response
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct BatchEntityResponse {
    pub entities: Vec<EntityResult>,
    pub metadata: ResponseMetadata,
}

/// List repositories response
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ListRepositoriesResponse {
    pub repositories: Vec<RepositoryInfo>,
    pub total: usize,
}

/// Repository information
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct RepositoryInfo {
    pub repository_id: Uuid,
    pub repository_name: String,
    pub repository_path: String,
    pub collection_name: String,
    pub last_indexed_commit: Option<String>,
}

/// Embedding generation request
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct EmbeddingRequest {
    pub texts: Vec<String>,
    pub instruction: Option<String>,
}

/// Embedding generation response
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct EmbeddingResponse {
    pub embeddings: Vec<Vec<f32>>,
    pub dimension: usize,
}

/// Convert API search filters to storage search filters
pub fn build_storage_filters(filters: &Option<SearchFilters>) -> Option<StorageSearchFilters> {
    filters.as_ref().map(|f| StorageSearchFilters {
        entity_type: f
            .entity_type
            .clone()
            .and_then(|types| types.first().cloned()),
        language: f.language.clone(),
        file_path: f.file_path.as_ref().map(std::path::PathBuf::from),
    })
}
