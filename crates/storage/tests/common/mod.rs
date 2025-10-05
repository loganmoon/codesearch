//! Test utilities for storage integration tests

use anyhow::Result;
use codesearch_core::{entities::EntityType, CodeEntity, CodeEntityBuilder, Location};
use codesearch_storage::EmbeddedEntity;
use std::path::PathBuf;
use uuid::Uuid;

/// Create a test CodeEntity with the given parameters
pub fn create_test_entity(
    name: &str,
    entity_type: EntityType,
    file_path: &str,
    language: &str,
    repository_id: Uuid,
) -> Result<CodeEntity> {
    CodeEntityBuilder::default()
        .entity_id(format!("{}::{}", file_path, name))
        .repository_id(repository_id.to_string())
        .name(name.to_string())
        .qualified_name(name.to_string())
        .entity_type(entity_type)
        .location(Location {
            file_path: PathBuf::from(file_path),
            start_line: 1,
            end_line: 10,
        })
        .language(language.to_string())
        .content(format!("fn {}() {{ /* test content */ }}", name))
        .signature(format!("fn {}()", name))
        .build()
}

/// Create an EmbeddedEntity with a mock embedding
pub fn create_embedded_entity(entity: CodeEntity, point_id: Uuid) -> EmbeddedEntity {
    EmbeddedEntity {
        entity,
        qdrant_point_id: point_id,
        embedding: mock_embedding(1536),
    }
}

/// Generate a deterministic mock embedding vector
pub fn mock_embedding(dim: usize) -> Vec<f32> {
    (0..dim).map(|i| (i as f32) / (dim as f32)).collect()
}

/// Generate a unique mock embedding for different entities
pub fn mock_embedding_for_entity(entity_id: &str, dim: usize) -> Vec<f32> {
    let hash = entity_id.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32));
    (0..dim)
        .map(|i| ((i as u32).wrapping_add(hash) as f32) / (dim as f32))
        .collect()
}
