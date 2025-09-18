use crate::StorageEntity;
use codesearch_core::{CodeEntity, Error};
use qdrant_client::qdrant::{PointStruct, UpsertPointsBuilder};
use serde_json::json;

use super::client::QdrantStorage;

/// Convert CodeEntity to Qdrant PointStruct
fn entity_to_point(entity: &CodeEntity, index: u64) -> PointStruct {
    let storage_entity = StorageEntity::from(entity);

    // Create payload with entity fields using serde_json::Map for compatibility
    let mut payload = serde_json::Map::new();
    payload.insert("id".to_string(), json!(storage_entity.id));
    payload.insert("name".to_string(), json!(storage_entity.name));
    payload.insert("kind".to_string(), json!(storage_entity.kind));
    payload.insert("file_path".to_string(), json!(storage_entity.file_path));
    payload.insert("start_line".to_string(), json!(storage_entity.start_line));
    payload.insert("end_line".to_string(), json!(storage_entity.end_line));
    payload.insert("content".to_string(), json!(storage_entity.content));

    // For now, generate random vectors for testing (will be replaced with real embeddings)
    let vector_size = 768; // Default for all-minilm-l6-v2
    let vector: Vec<f32> = (0..vector_size)
        .map(|i| (i as f32 + index as f32) / 1000.0)
        .collect();

    PointStruct::new(index, vector, payload)
}

/// Handle bulk loading of entities with proper batching
pub(super) async fn bulk_load_entities(
    storage: &QdrantStorage,
    entities: &[CodeEntity],
    functions: &[CodeEntity],
    types: &[CodeEntity],
    variables: &[CodeEntity],
    _relationships: &[(String, String, String)],
) -> Result<(), Error> {
    // Combine all entities
    let mut all_entities = Vec::new();
    all_entities.extend_from_slice(entities);
    all_entities.extend_from_slice(functions);
    all_entities.extend_from_slice(types);
    all_entities.extend_from_slice(variables);

    if all_entities.is_empty() {
        return Ok(());
    }

    let batch_size = storage.config.batch_size;
    let collection_name = &storage.config.collection_name;

    // Process in batches
    for (batch_idx, chunk) in all_entities.chunks(batch_size).enumerate() {
        let points: Vec<PointStruct> = chunk
            .iter()
            .enumerate()
            .map(|(i, entity)| {
                let index = (batch_idx * batch_size + i) as u64;
                entity_to_point(entity, index)
            })
            .collect();

        // Upsert points to Qdrant
        let upsert_operation = UpsertPointsBuilder::new(collection_name, points);

        storage
            .client
            .upsert_points(upsert_operation)
            .await
            .map_err(|e| Error::storage(format!("Failed to upsert batch {batch_idx}: {e}")))?;
    }

    Ok(())
}
