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

    /// Convert CodeEntity to Qdrant point payload
    fn entity_to_payload(entity: &CodeEntity) -> Payload {
        // Serialize the entire entity as JSON, then convert to Qdrant Value
        if let Ok(json) = serde_json::to_value(entity) {
            if let Ok(map) =
                serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(json)
            {
                return Payload::from(map);
            }
        }

        Payload::from(serde_json::Map::new())
    }

    /// Convert Qdrant payload back to CodeEntity
    fn payload_to_entity(payload: &HashMap<String, QdrantValue>) -> Result<CodeEntity> {
        // Convert Qdrant Values to serde_json Values
        let mut json_map = serde_json::Map::new();
        for (key, value) in payload {
            if let Ok(json_value) = Self::qdrant_value_to_json(value) {
                json_map.insert(key.clone(), json_value);
            }
        }

        serde_json::from_value(serde_json::Value::Object(json_map))
            .map_err(|e| Error::storage(format!("Failed to deserialize entity: {e}")))
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
    async fn bulk_load_entities(&self, embedded_entities: Vec<EmbeddedEntity>) -> Result<()> {
        if embedded_entities.is_empty() {
            return Ok(());
        }

        let points: Vec<PointStruct> = embedded_entities
            .iter()
            .map(|embedded| {
                // Generate a new UUID for the Qdrant point ID
                let id = PointId::from(Uuid::new_v4().to_string());
                PointStruct::new(
                    id,
                    embedded.embedding.clone(),
                    Self::entity_to_payload(&embedded.entity),
                )
            })
            .collect();

        // Use upsert to handle duplicates gracefully
        self.qdrant_client
            .upsert_points(qdrant_client::qdrant::UpsertPoints::from(
                qdrant_client::qdrant::UpsertPointsBuilder::new(
                    self.collection_name.clone(),
                    points,
                ),
            ))
            .await
            .map_err(|e| Error::storage(e.to_string()))?;

        Ok(())
    }

    async fn search_similar(
        &self,
        query_embedding: Vec<f32>,
        limit: usize,
        filters: Option<SearchFilters>,
    ) -> Result<Vec<(CodeEntity, f32)>> {
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
                if let Ok(entity) = Self::payload_to_entity(&point.payload) {
                    results.push((entity, point.score));
                }
            }
        }

        Ok(results)
    }

    async fn get_entity(&self, entity_id: &str) -> Result<Option<CodeEntity>> {
        // Search by entity_id in the payload, not by point ID
        let filter = Filter {
            must: vec![qdrant_client::qdrant::Condition {
                condition_one_of: Some(qdrant_client::qdrant::condition::ConditionOneOf::Field(
                    qdrant_client::qdrant::FieldCondition {
                        key: "entity_id".to_string(),
                        r#match: Some(qdrant_client::qdrant::Match {
                            match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Keyword(
                                entity_id.to_string(),
                            )),
                        }),
                        ..Default::default()
                    },
                )),
            }],
            ..Default::default()
        };

        // Use scroll to find the entity
        let scroll_response = self
            .qdrant_client
            .scroll(qdrant_client::qdrant::ScrollPoints {
                collection_name: self.collection_name.clone(),
                filter: Some(filter),
                limit: Some(1),
                with_payload: Some(true.into()),
                ..Default::default()
            })
            .await
            .map_err(|e| Error::storage(e.to_string()))?;

        if let Some(point) = scroll_response.result.first() {
            if !point.payload.is_empty() {
                return Ok(Some(Self::payload_to_entity(&point.payload)?));
            }
        }

        Ok(None)
    }
}
