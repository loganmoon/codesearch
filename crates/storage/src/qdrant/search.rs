use crate::error::StorageError;
use crate::{ScoredEntity, StorageEntity};
use codesearch_core::Error;
use qdrant_client::qdrant::{SearchPointsBuilder, Value as QdrantValue};

use super::client::QdrantStorage;

/// Convert Qdrant payload to StorageEntity
fn payload_to_entity(
    payload: &std::collections::HashMap<String, QdrantValue>,
) -> Option<StorageEntity> {
    // Helper function to get string value from Qdrant Value
    let get_string = |key: &str| -> Option<String> {
        payload.get(key).and_then(|v| match v.kind.as_ref()? {
            qdrant_client::qdrant::value::Kind::StringValue(s) => Some(s.clone()),
            _ => None,
        })
    };

    // Helper function to get integer value from Qdrant Value
    let get_int = |key: &str| -> Option<i64> {
        payload.get(key).and_then(|v| match v.kind.as_ref()? {
            qdrant_client::qdrant::value::Kind::IntegerValue(i) => Some(*i),
            _ => None,
        })
    };

    Some(StorageEntity {
        id: get_string("id")?,
        name: get_string("name")?,
        kind: get_string("kind")?,
        file_path: get_string("file_path")?,
        start_line: get_int("start_line")? as usize,
        end_line: get_int("end_line")? as usize,
        content: get_string("content")?,
        embedding: None, // We don't store embeddings in the response
    })
}

/// Search for entities similar to the query vector
pub(super) async fn search_similar(
    storage: &QdrantStorage,
    query_vector: Vec<f32>,
    limit: usize,
    score_threshold: Option<f32>,
) -> Result<Vec<ScoredEntity>, Error> {
    let collection_name = &storage.config.collection_name;

    // Validate vector dimensions
    if query_vector.len() != storage.config.vector_size {
        return Err(StorageError::InvalidDimensions {
            expected: storage.config.vector_size,
            actual: query_vector.len(),
        }
        .into());
    }

    // Build search request
    let mut search_builder =
        SearchPointsBuilder::new(collection_name, query_vector, limit as u64).with_payload(true);

    if let Some(threshold) = score_threshold {
        search_builder = search_builder.score_threshold(threshold);
    }

    // Execute search
    let search_result = storage
        .client
        .search_points(search_builder)
        .await
        .map_err(|e| StorageError::BackendError(format!("Search failed: {e}")))?;

    // Convert results to ScoredEntity
    let mut scored_entities = Vec::new();
    for scored_point in search_result.result {
        if !scored_point.payload.is_empty() {
            if let Some(entity) = payload_to_entity(&scored_point.payload) {
                scored_entities.push(ScoredEntity {
                    entity,
                    score: scored_point.score,
                });
            }
        }
    }

    Ok(scored_entities)
}

/// Get a single entity by ID
pub(super) async fn get_entity_by_id(
    storage: &QdrantStorage,
    id: &str,
) -> Result<Option<StorageEntity>, Error> {
    let collection_name = &storage.config.collection_name;

    // Search for the entity by ID in payload
    let filter = qdrant_client::qdrant::Filter {
        must: vec![qdrant_client::qdrant::Condition {
            condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::Field(
                qdrant_client::qdrant::FieldCondition {
                    key: "id".to_string(),
                    r#match: Some(qdrant_client::qdrant::Match {
                        match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Text(
                            id.to_string(),
                        )),
                    }),
                    ..Default::default()
                },
            )),
        }],
        ..Default::default()
    };

    // Use scroll to find the entity
    let scroll_result = storage
        .client
        .scroll(
            qdrant_client::qdrant::ScrollPointsBuilder::new(collection_name)
                .filter(filter)
                .with_payload(true)
                .limit(1),
        )
        .await
        .map_err(|e| StorageError::BackendError(format!("Failed to get entity by ID: {e}")))?;

    // Convert the first result if found
    if let Some(point) = scroll_result.result.first() {
        if !point.payload.is_empty() {
            return Ok(payload_to_entity(&point.payload));
        }
    }

    Ok(None)
}

/// Get multiple entities by their IDs
pub(super) async fn get_entities_by_ids(
    storage: &QdrantStorage,
    ids: &[String],
) -> Result<Vec<StorageEntity>, Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let collection_name = &storage.config.collection_name;

    // Build filter for multiple IDs
    let conditions = ids
        .iter()
        .map(|id| qdrant_client::qdrant::Condition {
            condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::Field(
                qdrant_client::qdrant::FieldCondition {
                    key: "id".to_string(),
                    r#match: Some(qdrant_client::qdrant::Match {
                        match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Text(
                            id.clone(),
                        )),
                    }),
                    ..Default::default()
                },
            )),
        })
        .collect();

    let filter = qdrant_client::qdrant::Filter {
        should: conditions,
        ..Default::default()
    };

    // Use scroll to find all matching entities
    let scroll_result = storage
        .client
        .scroll(
            qdrant_client::qdrant::ScrollPointsBuilder::new(collection_name)
                .filter(filter)
                .with_payload(true)
                .limit(ids.len() as u32),
        )
        .await
        .map_err(|e| StorageError::BackendError(format!("Failed to get entities by IDs: {e}")))?;

    // Convert results to StorageEntity
    let mut entities = Vec::new();
    for point in scroll_result.result {
        if !point.payload.is_empty() {
            if let Some(entity) = payload_to_entity(&point.payload) {
                entities.push(entity);
            }
        }
    }

    Ok(entities)
}
