//! Qdrant storage client implementation for CRUD operations

use crate::{EmbeddedEntity, SearchFilters, StorageClient};
use async_trait::async_trait;
use codesearch_core::{
    error::{Error, Result},
    CodeEntity,
};
use qdrant_client::{
    qdrant::{Filter, PointId, PointStruct, SearchPoints, Value as QdrantValue},
    Payload, Qdrant,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct MinimalEntityPayload {
    entity_id: String,
    repository_id: String,
    name: String,
    qualified_name: String,
    entity_type: String,
    file_path: String,
    line_range_start: usize,
    line_range_end: usize,
}

/// Qdrant storage client implementing CRUD operations only
pub(crate) struct QdrantStorageClient {
    qdrant_client: Arc<Qdrant>,
    collection_name: String,
}

impl QdrantStorageClient {
    /// Create a new Qdrant storage client
    pub async fn new(connection: Arc<Qdrant>, collection_name: String) -> Result<Self> {
        Ok(Self {
            qdrant_client: connection,
            collection_name,
        })
    }

    /// Convert entity to minimal Qdrant payload (display fields only)
    fn entity_to_minimal_payload(entity: &CodeEntity) -> Payload {
        let mut map = serde_json::Map::new();

        // Core identifiers
        map.insert("entity_id".to_string(), json!(entity.entity_id));
        map.insert("repository_id".to_string(), json!(entity.repository_id));

        // Display fields for search results
        map.insert("name".to_string(), json!(entity.name));
        map.insert("qualified_name".to_string(), json!(entity.qualified_name));
        map.insert(
            "entity_type".to_string(),
            json!(format!("{:?}", entity.entity_type)),
        );
        map.insert(
            "language".to_string(),
            json!(format!("{:?}", entity.language)),
        );
        map.insert(
            "file_path".to_string(),
            json!(entity.file_path.display().to_string()),
        );
        map.insert(
            "line_range_start".to_string(),
            json!(entity.location.start_line),
        );
        map.insert(
            "line_range_end".to_string(),
            json!(entity.location.end_line),
        );

        // DO NOT include: content, signature, dependencies, metadata, documentation_summary

        Payload::from(map)
    }

    /// Convert Qdrant payload to minimal entity payload
    fn payload_to_minimal_entity(
        payload: &HashMap<String, QdrantValue>,
    ) -> Result<MinimalEntityPayload> {
        let mut json_map = serde_json::Map::new();
        for (key, value) in payload {
            if let Ok(json_value) = Self::qdrant_value_to_json(value) {
                json_map.insert(key.clone(), json_value);
            }
        }
        serde_json::from_value(serde_json::Value::Object(json_map))
            .map_err(|e| Error::storage(format!("Failed to deserialize minimal payload: {e}")))
    }

    /// Convert Qdrant Value to serde_json Value
    fn qdrant_value_to_json(value: &QdrantValue) -> Result<serde_json::Value> {
        use qdrant_client::qdrant::value::Kind;

        match &value.kind {
            Some(Kind::NullValue(_)) => Ok(serde_json::Value::Null),
            Some(Kind::BoolValue(b)) => Ok(serde_json::Value::Bool(*b)),
            Some(Kind::IntegerValue(i)) => Ok(json!(*i)),
            Some(Kind::DoubleValue(d)) => Ok(json!(*d)),
            Some(Kind::StringValue(s)) => Ok(serde_json::Value::String(s.clone())),
            Some(Kind::ListValue(list)) => {
                let values: Result<Vec<_>> =
                    list.values.iter().map(Self::qdrant_value_to_json).collect();
                Ok(serde_json::Value::Array(values?))
            }
            Some(Kind::StructValue(s)) => {
                let mut map = serde_json::Map::new();
                for (k, v) in &s.fields {
                    if let Ok(json_v) = Self::qdrant_value_to_json(v) {
                        map.insert(k.clone(), json_v);
                    }
                }
                Ok(serde_json::Value::Object(map))
            }
            None => Ok(serde_json::Value::Null),
        }
    }

    /// Build Qdrant filter from SearchFilters
    fn build_filter(filters: &SearchFilters) -> Option<Filter> {
        let mut conditions = vec![];

        if let Some(entity_type) = &filters.entity_type {
            conditions.push(qdrant_client::qdrant::Condition::matches(
                "entity_type",
                entity_type.to_string(),
            ));
        }

        if let Some(language) = &filters.language {
            conditions.push(qdrant_client::qdrant::Condition::matches(
                "language",
                language.clone(),
            ));
        }

        if let Some(file_path) = &filters.file_path {
            conditions.push(qdrant_client::qdrant::Condition::matches(
                "file_path",
                file_path.to_string_lossy().to_string(),
            ));
        }

        if conditions.is_empty() {
            None
        } else {
            Some(Filter::must(conditions))
        }
    }
}

#[async_trait]
impl StorageClient for QdrantStorageClient {
    async fn bulk_load_entities(
        &self,
        embedded_entities: Vec<EmbeddedEntity>,
    ) -> Result<Vec<(String, Uuid)>> {
        if embedded_entities.is_empty() {
            return Ok(vec![]);
        }

        let points: Vec<_> = embedded_entities
            .into_iter()
            .map(|embedded| {
                let point_id = embedded.qdrant_point_id;
                let entity_id = embedded.entity.entity_id.clone();
                let point = PointStruct::new(
                    PointId::from(point_id.to_string()),
                    embedded.embedding,
                    Self::entity_to_minimal_payload(&embedded.entity),
                );
                (entity_id, point_id, point)
            })
            .collect();

        let entity_point_map: Vec<(String, Uuid)> = points
            .iter()
            .map(|(eid, pid, _)| (eid.clone(), *pid))
            .collect();

        let qdrant_points: Vec<PointStruct> = points.into_iter().map(|(_, _, p)| p).collect();

        // Use upsert to handle duplicates gracefully
        self.qdrant_client
            .upsert_points(qdrant_client::qdrant::UpsertPoints::from(
                qdrant_client::qdrant::UpsertPointsBuilder::new(
                    self.collection_name.clone(),
                    qdrant_points,
                ),
            ))
            .await
            .map_err(|e| Error::storage(e.to_string()))?;

        Ok(entity_point_map)
    }

    async fn search_similar(
        &self,
        query_embedding: Vec<f32>,
        limit: usize,
        filters: Option<SearchFilters>,
    ) -> Result<Vec<(String, String, f32)>> {
        let filter = filters.and_then(|f| Self::build_filter(&f));

        let search_result = self
            .qdrant_client
            .search_points(SearchPoints::from(
                qdrant_client::qdrant::SearchPointsBuilder::new(
                    self.collection_name.clone(),
                    query_embedding,
                    limit as u64,
                )
                .filter(filter.unwrap_or_default())
                .with_payload(true),
            ))
            .await
            .map_err(|e| Error::storage(e.to_string()))?;

        let mut results = Vec::new();
        for point in search_result.result {
            if !point.payload.is_empty() {
                if let Ok(payload) = Self::payload_to_minimal_entity(&point.payload) {
                    results.push((payload.entity_id, payload.repository_id, point.score));
                }
            }
        }

        Ok(results)
    }

    async fn get_entity(&self, _entity_id: &str) -> Result<Option<CodeEntity>> {
        // Not implemented - entities should be fetched from Postgres
        // Qdrant only stores minimal payload for search
        Err(Error::storage(
            "get_entity not supported for Qdrant storage - use Postgres client instead",
        ))
    }

    /// Delete entities from Qdrant by entity_id
    async fn delete_entities(&self, entity_ids: &[String]) -> Result<()> {
        use qdrant_client::qdrant::{
            condition::ConditionOneOf, points_selector::PointsSelectorOneOf, r#match::MatchValue,
            Condition, DeletePointsBuilder, FieldCondition, Filter, Match, PointsIdsList,
            ScrollPoints,
        };

        if entity_ids.is_empty() {
            return Ok(());
        }

        // Search for points by entity_id to get point_ids
        // Use a single batched filter with OR conditions instead of N queries
        let filter = Filter {
            should: entity_ids
                .iter()
                .map(|entity_id| Condition {
                    condition_one_of: Some(ConditionOneOf::Field(FieldCondition {
                        key: "entity_id".to_string(),
                        r#match: Some(Match {
                            match_value: Some(MatchValue::Keyword(entity_id.clone())),
                        }),
                        ..Default::default()
                    })),
                })
                .collect(),
            ..Default::default()
        };

        let search_result = self
            .qdrant_client
            .scroll(ScrollPoints {
                collection_name: self.collection_name.clone(),
                filter: Some(filter),
                limit: Some(entity_ids.len() as u32),
                with_payload: Some(false.into()),
                with_vectors: Some(false.into()),
                ..Default::default()
            })
            .await
            .map_err(|e| Error::storage(e.to_string()))?;

        let point_ids_to_delete: Vec<_> = search_result
            .result
            .into_iter()
            .filter_map(|point| point.id)
            .collect();

        if !point_ids_to_delete.is_empty() {
            self.qdrant_client
                .delete_points(
                    DeletePointsBuilder::new(self.collection_name.clone())
                        .points(PointsSelectorOneOf::Points(PointsIdsList {
                            ids: point_ids_to_delete,
                        }))
                        .build(),
                )
                .await
                .map_err(|e| Error::storage(e.to_string()))?;
        }

        Ok(())
    }
}
