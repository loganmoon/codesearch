//! Test utilities for storage layer integration tests

use codesearch_core::{
    config::StorageConfig,
    entities::{CodeEntityBuilder, EntityType, Language, SourceLocation, Visibility},
    CodeEntity,
};
use codesearch_storage::EmbeddedEntity;
use std::path::PathBuf;
use uuid::Uuid;

/// Create a test CodeEntity with minimal required fields
#[allow(dead_code)]
pub fn create_test_entity(name: &str, entity_type: EntityType, repository_id: &str) -> CodeEntity {
    CodeEntityBuilder::default()
        .entity_id(format!("{}_{}", name, Uuid::new_v4()))
        .repository_id(repository_id.to_string())
        .name(name.to_string())
        .qualified_name(format!("test::{name}"))
        .entity_type(entity_type)
        .file_path(PathBuf::from("test.rs"))
        .location(SourceLocation {
            start_line: 1,
            end_line: 10,
            start_column: 0,
            end_column: 0,
        })
        .language(Language::Rust)
        .visibility(Visibility::Public)
        .content(Some(format!("fn {name}() {{}}")))
        .build()
        .expect("Failed to build test entity")
}

/// Create an EmbeddedEntity with a mock embedding
#[allow(dead_code)]
pub fn create_embedded_entity(entity: CodeEntity, dimension: usize) -> EmbeddedEntity {
    EmbeddedEntity {
        dense_embedding: mock_embedding(dimension),
        sparse_embedding: vec![(0, 0.5), (1, 0.3), (2, 0.2)], // Mock sparse embedding
        bm25_token_count: 50,                                 // Mock token count
        qdrant_point_id: Uuid::new_v4(),
        entity,
    }
}

/// Generate a deterministic mock embedding vector
///
/// Creates a simple pattern: [0.1, 0.2, 0.3, ..., 0.1, 0.2, 0.3, ...]
/// This ensures embeddings are valid but predictable for testing
#[allow(dead_code)]
pub fn mock_embedding(dimension: usize) -> Vec<f32> {
    (0..dimension)
        .map(|i| ((i % 10) as f32 + 1.0) / 10.0)
        .collect()
}

/// Create a StorageConfig from test container instances
///
/// Helper to build config with test container ports
/// Note: collection_name parameter is kept for API compatibility but not used
/// since collection_name was removed from StorageConfig. Tests should manage
/// collection names separately.
pub fn create_storage_config(
    qdrant_port: u16,
    qdrant_rest_port: u16,
    postgres_port: u16,
    postgres_database: &str,
) -> StorageConfig {
    StorageConfig {
        qdrant_host: "localhost".to_string(),
        qdrant_port,
        qdrant_rest_port,
        auto_start_deps: false,
        docker_compose_file: None,
        postgres_host: "localhost".to_string(),
        postgres_port,
        postgres_database: postgres_database.to_string(),
        postgres_user: "codesearch".to_string(),
        postgres_password: "codesearch".to_string(),
        neo4j_host: "localhost".to_string(),
        neo4j_http_port: 7474,
        neo4j_bolt_port: 7687,
        neo4j_user: "neo4j".to_string(),
        neo4j_password: "codesearch".to_string(),
        max_entities_per_db_operation: 1000,
    }
}

/// Create a test entity with custom file path for filtering tests
#[allow(dead_code)]
pub fn create_test_entity_with_file(
    name: &str,
    entity_type: EntityType,
    repository_id: &str,
    file_path: &str,
) -> CodeEntity {
    CodeEntityBuilder::default()
        .entity_id(format!("{}_{}", name, Uuid::new_v4()))
        .repository_id(repository_id.to_string())
        .name(name.to_string())
        .qualified_name(format!("test::{name}"))
        .entity_type(entity_type)
        .file_path(PathBuf::from(file_path))
        .location(SourceLocation {
            start_line: 1,
            end_line: 10,
            start_column: 0,
            end_column: 0,
        })
        .language(Language::Rust)
        .visibility(Visibility::Public)
        .content(Some(format!("fn {name}() {{}}")))
        .build()
        .expect("Failed to build test entity")
}
