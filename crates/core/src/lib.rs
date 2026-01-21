//! Core types and traits for the Code Context semantic indexing system
//!
//! This crate provides the foundational abstractions used throughout the
//! codesearch system, including:
//!
//! - **Entities**: Code entities like functions, classes, and modules
//! - **Chunks**: Semantic units of code with metadata and embeddings
//! - **Traits**: Core traits for language-specific implementations
//! - **Configuration**: System configuration management
//! - **Error handling**: Unified error types
//!

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

pub mod config;
pub mod entities;
pub mod entity_id;
pub mod error;
pub mod project_manifest;
pub mod qualified_name;
pub mod resolution;
pub mod search_api;
pub mod search_models;

// Re-export main types for convenience
pub use config::{
    Config, EmbeddingsConfig, HybridSearchConfig, IndexerConfig, RerankingConfig, StorageConfig,
    UpdateStrategy, WatcherConfig,
};
pub use entities::{
    CodeEntity, CodeRelationship, EntityMetadata, EntityRelationshipData, EntityType,
    FunctionSignature, Language, ReferenceType, RelationshipType, SourceLocation, SourceReference,
    Visibility,
};
pub use entity_id::{generate_anonymous_entity_id, generate_entity_id, ScopeContext};
pub use error::{Error, Result, ResultExt};
pub use project_manifest::{
    detect_manifest, PackageInfo, PackageMap, ProjectManifest, ProjectType,
};
pub use qualified_name::{PathSeparator, QualifiedName};
pub use resolution::{definitions as relationship_definitions, LookupStrategy, RelationshipDef};
pub use search_api::SearchApi;
pub use search_models::{
    BatchEntityRequest, BatchEntityResponse, EmbeddingRequest, EmbeddingResponse, EntityResult,
    GraphQueryParameters, GraphQueryRequest, GraphQueryResponse, GraphQueryType,
    GraphResponseMetadata, GraphResult, ListRepositoriesResponse, QuerySpec, RepositoryInfo,
    ResponseMetadata, SearchFilters, SemanticSearchRequest, SemanticSearchResponse,
};

/// Version of the core library
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
