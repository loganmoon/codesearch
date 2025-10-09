use codesearch_core::error::{Error, Result};
use codesearch_storage::{EmbeddedEntity, StorageClient};
use codesearch_storage::{OutboxEntry, PostgresClientTrait, TargetStore};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

pub struct OutboxProcessor {
    postgres_client: Arc<dyn PostgresClientTrait>,
    qdrant_client: Arc<dyn StorageClient>,
    poll_interval: Duration,
    batch_size: i64,
    max_retries: i32,
}

impl OutboxProcessor {
    pub fn new(
        postgres_client: Arc<dyn PostgresClientTrait>,
        qdrant_client: Arc<dyn StorageClient>,
        poll_interval: Duration,
        batch_size: i64,
        max_retries: i32,
    ) -> Self {
        Self {
            postgres_client,
            qdrant_client,
            poll_interval,
            batch_size,
            max_retries,
        }
    }

    /// Start processing loop (runs indefinitely until process is killed)
    pub async fn start(&self) -> Result<()> {
        info!("Outbox processor started");

        loop {
            if let Err(e) = self.process_batch().await {
                error!("Outbox processing error: {e}");
            }

            sleep(self.poll_interval).await;
        }
    }

    async fn process_batch(&self) -> Result<()> {
        let qdrant_entries = self
            .postgres_client
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, self.batch_size)
            .await?;

        if !qdrant_entries.is_empty() {
            debug!("Processing {} Qdrant outbox entries", qdrant_entries.len());

            // Separate INSERT/UPDATE from DELETE operations
            let mut insert_update_entries = Vec::new();
            let mut delete_entries = Vec::new();

            for entry in qdrant_entries {
                if entry.retry_count >= self.max_retries {
                    warn!(
                        "Outbox entry {} exceeded max retries ({}), marking as processed with failure",
                        entry.outbox_id, self.max_retries
                    );
                    // Mark as processed so it doesn't block the outbox forever
                    self.postgres_client
                        .mark_outbox_processed(entry.outbox_id)
                        .await?;
                    continue;
                }

                match entry.operation.as_str() {
                    "INSERT" | "UPDATE" => insert_update_entries.push(entry),
                    "DELETE" => delete_entries.push(entry),
                    _ => {
                        error!("Unknown operation: {}", entry.operation);
                        self.postgres_client
                            .record_outbox_failure(entry.outbox_id, "Unknown operation")
                            .await?;
                    }
                }
            }

            // Process INSERT/UPDATE entries in true batch
            if !insert_update_entries.is_empty() {
                let mut embedded_entities = Vec::new();
                let mut successful_outbox_ids = Vec::new();

                for entry in &insert_update_entries {
                    match self.prepare_embedded_entity(entry) {
                        Ok(embedded) => {
                            embedded_entities.push(embedded);
                            successful_outbox_ids.push(entry.outbox_id);
                        }
                        Err(e) => {
                            error!("Failed to prepare entry {}: {e}", entry.outbox_id);
                            self.postgres_client
                                .record_outbox_failure(entry.outbox_id, &e.to_string())
                                .await?;
                        }
                    }
                }

                // Single bulk operation for entire batch
                if !embedded_entities.is_empty() {
                    self.qdrant_client
                        .bulk_load_entities(embedded_entities)
                        .await?;

                    for outbox_id in successful_outbox_ids {
                        self.postgres_client
                            .mark_outbox_processed(outbox_id)
                            .await?;
                    }
                    debug!(
                        "Processed {} INSERT/UPDATE entries in batch",
                        insert_update_entries.len()
                    );
                }
            }

            // Process DELETE entries (already batched internally)
            for entry in delete_entries {
                match self.process_delete_entry(&entry).await {
                    Ok(_) => {
                        self.postgres_client
                            .mark_outbox_processed(entry.outbox_id)
                            .await?;
                    }
                    Err(e) => {
                        error!("Failed to process DELETE entry {}: {e}", entry.outbox_id);
                        self.postgres_client
                            .record_outbox_failure(entry.outbox_id, &e.to_string())
                            .await?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Prepare an embedded entity from an outbox entry (validation only, no I/O)
    pub(crate) fn prepare_embedded_entity(&self, entry: &OutboxEntry) -> Result<EmbeddedEntity> {
        // Extract both entity and embedding from payload
        let entity: codesearch_core::entities::CodeEntity = serde_json::from_value(
            entry
                .payload
                .get("entity")
                .ok_or_else(|| Error::storage("Missing entity in payload"))?
                .clone(),
        )
        .map_err(|e| Error::storage(format!("Failed to deserialize entity: {e}")))?;

        let embedding: Vec<f32> = serde_json::from_value(
            entry
                .payload
                .get("embedding")
                .ok_or_else(|| Error::storage("Missing embedding in payload"))?
                .clone(),
        )
        .map_err(|e| Error::storage(format!("Failed to deserialize embedding: {e}")))?;

        let qdrant_point_id: String = serde_json::from_value(
            entry
                .payload
                .get("qdrant_point_id")
                .ok_or_else(|| Error::storage("Missing qdrant_point_id in payload"))?
                .clone(),
        )
        .map_err(|e| Error::storage(format!("Failed to deserialize qdrant_point_id: {e}")))?;

        let qdrant_point_id = codesearch_storage::Uuid::parse_str(&qdrant_point_id)
            .map_err(|e| Error::storage(format!("Invalid qdrant_point_id: {e}")))?;

        // Validate embedding dimensions (prevent memory exhaustion)
        const MAX_EMBEDDING_DIM: usize = 100_000;
        if embedding.len() > MAX_EMBEDDING_DIM {
            return Err(Error::storage(format!(
                "Embedding dimensions {} exceeds maximum allowed size of {}",
                embedding.len(),
                MAX_EMBEDDING_DIM
            )));
        }

        // Removed hardcoded 1536 check - dimension validation handled by Qdrant

        Ok(EmbeddedEntity {
            entity,
            embedding,
            qdrant_point_id,
        })
    }

    /// Process a DELETE outbox entry
    async fn process_delete_entry(&self, entry: &OutboxEntry) -> Result<()> {
        // Extract entity IDs from payload
        let entity_ids: Vec<String> = if let Some(ids) = entry.payload.get("entity_ids") {
            serde_json::from_value(ids.clone())
                .map_err(|e| Error::storage(format!("Invalid DELETE payload: {e}")))?
        } else {
            vec![entry.entity_id.clone()]
        };

        // Delete from Qdrant by entity_id
        self.qdrant_client.delete_entities(&entity_ids).await?;

        tracing::info!("Deleted {} entities from Qdrant", entity_ids.len());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesearch_core::entities::{
        CodeEntityBuilder, EntityType, Language, SourceLocation, Visibility,
    };
    use codesearch_core::CodeEntity;
    use codesearch_storage::{OutboxOperation, Uuid};
    use serde_json::json;
    use std::sync::Mutex;

    // Type alias matching the storage module's internal type
    type EntityOutboxBatchEntry<'a> = (
        &'a CodeEntity,
        &'a [f32],
        OutboxOperation,
        Uuid,
        TargetStore,
        Option<String>,
    );

    // Mock PostgreSQL client
    struct MockPostgresClient {
        unprocessed_entries: Mutex<Vec<OutboxEntry>>,
        processed_ids: Mutex<Vec<Uuid>>,
        failed_ids: Mutex<Vec<(Uuid, String)>>,
    }

    impl MockPostgresClient {
        fn new(entries: Vec<OutboxEntry>) -> Self {
            Self {
                unprocessed_entries: Mutex::new(entries),
                processed_ids: Mutex::new(Vec::new()),
                failed_ids: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl PostgresClientTrait for MockPostgresClient {
        async fn run_migrations(&self) -> Result<()> {
            Ok(())
        }

        async fn ensure_repository(
            &self,
            _: &std::path::Path,
            _: &str,
            _: Option<&str>,
        ) -> Result<Uuid> {
            Ok(Uuid::new_v4())
        }

        async fn get_repository_id(&self, _: &str) -> Result<Option<Uuid>> {
            Ok(Some(Uuid::new_v4()))
        }

        async fn get_entity_metadata(
            &self,
            _: Uuid,
            _: &str,
        ) -> Result<Option<(Uuid, Option<chrono::DateTime<chrono::Utc>>)>> {
            Ok(None)
        }

        async fn get_entities_metadata_batch(
            &self,
            _: Uuid,
            _: &[String],
        ) -> Result<std::collections::HashMap<String, (Uuid, Option<chrono::DateTime<chrono::Utc>>)>>
        {
            Ok(std::collections::HashMap::new())
        }

        async fn get_file_snapshot(&self, _: Uuid, _: &str) -> Result<Option<Vec<String>>> {
            Ok(None)
        }

        async fn update_file_snapshot(
            &self,
            _: Uuid,
            _: &str,
            _: Vec<String>,
            _: Option<String>,
        ) -> Result<()> {
            Ok(())
        }

        async fn get_entities_by_ids(&self, _: &[(Uuid, String)]) -> Result<Vec<CodeEntity>> {
            Ok(vec![])
        }

        async fn mark_entities_deleted(&self, _: Uuid, _: &[String]) -> Result<()> {
            Ok(())
        }

        async fn mark_entities_deleted_with_outbox(&self, _: Uuid, _: &[String]) -> Result<()> {
            Ok(())
        }

        async fn store_entities_with_outbox_batch(
            &self,
            _: Uuid,
            _: &[EntityOutboxBatchEntry<'_>],
        ) -> Result<Vec<Uuid>> {
            Ok(vec![])
        }

        async fn get_unprocessed_outbox_entries(
            &self,
            _target: TargetStore,
            _limit: i64,
        ) -> Result<Vec<OutboxEntry>> {
            Ok(self.unprocessed_entries.lock().unwrap().clone())
        }

        async fn mark_outbox_processed(&self, outbox_id: Uuid) -> Result<()> {
            self.processed_ids.lock().unwrap().push(outbox_id);
            Ok(())
        }

        async fn record_outbox_failure(&self, outbox_id: Uuid, error_message: &str) -> Result<()> {
            self.failed_ids
                .lock()
                .unwrap()
                .push((outbox_id, error_message.to_string()));
            Ok(())
        }

        async fn get_last_indexed_commit(&self, _: Uuid) -> Result<Option<String>> {
            Ok(None)
        }

        async fn set_last_indexed_commit(&self, _: Uuid, _: &str) -> Result<()> {
            Ok(())
        }
    }

    // Mock Qdrant client
    struct MockQdrantClient {
        bulk_loaded: Mutex<Vec<EmbeddedEntity>>,
        deleted_ids: Mutex<Vec<String>>,
        should_fail: bool,
    }

    impl MockQdrantClient {
        fn new(should_fail: bool) -> Self {
            Self {
                bulk_loaded: Mutex::new(Vec::new()),
                deleted_ids: Mutex::new(Vec::new()),
                should_fail,
            }
        }
    }

    #[async_trait::async_trait]
    impl StorageClient for MockQdrantClient {
        async fn bulk_load_entities(
            &self,
            entities: Vec<EmbeddedEntity>,
        ) -> Result<Vec<(String, Uuid)>> {
            if self.should_fail {
                return Err(Error::storage("Mock failure"));
            }
            let result: Vec<(String, Uuid)> = entities
                .iter()
                .map(|e| (e.entity.entity_id.clone(), e.qdrant_point_id))
                .collect();
            self.bulk_loaded.lock().unwrap().extend(entities);
            Ok(result)
        }

        async fn search_similar(
            &self,
            _: Vec<f32>,
            _: usize,
            _: Option<codesearch_storage::SearchFilters>,
        ) -> Result<Vec<(String, String, f32)>> {
            Ok(vec![])
        }

        async fn get_entity(&self, _: &str) -> Result<Option<CodeEntity>> {
            Ok(None)
        }

        async fn delete_entities(&self, entity_ids: &[String]) -> Result<()> {
            if self.should_fail {
                return Err(Error::storage("Mock failure"));
            }
            self.deleted_ids
                .lock()
                .unwrap()
                .extend(entity_ids.iter().cloned());
            Ok(())
        }
    }

    fn create_test_entity() -> codesearch_core::entities::CodeEntity {
        CodeEntityBuilder::default()
            .entity_id("test_entity_id".to_string())
            .repository_id("test_repo".to_string())
            .name("test_function".to_string())
            .qualified_name("test_function".to_string())
            .entity_type(EntityType::Function)
            .location(SourceLocation {
                start_line: 1,
                end_line: 1,
                start_column: 0,
                end_column: 10,
            })
            .visibility(Visibility::Public)
            .language(Language::Rust)
            .file_path(std::path::PathBuf::from("/test/file.rs"))
            .build()
            .unwrap()
    }

    fn create_test_outbox_entry(operation: &str, retry_count: i32) -> OutboxEntry {
        let entity = create_test_entity();
        let embedding = vec![0.1; 1536];
        let outbox_id = Uuid::new_v4();
        let qdrant_point_id = Uuid::new_v4();

        OutboxEntry {
            outbox_id,
            repository_id: Uuid::new_v4(),
            entity_id: entity.entity_id.clone(),
            operation: operation.to_string(),
            target_store: "qdrant".to_string(),
            payload: json!({
                "entity": entity,
                "embedding": embedding,
                "qdrant_point_id": qdrant_point_id.to_string(),
            }),
            created_at: chrono::Utc::now(),
            processed_at: None,
            retry_count,
            last_error: None,
        }
    }

    #[tokio::test]
    async fn test_successful_insert_processing() {
        let entry = create_test_outbox_entry("INSERT", 0);
        let outbox_id = entry.outbox_id;

        let postgres = Arc::new(MockPostgresClient::new(vec![entry]));
        let qdrant = Arc::new(MockQdrantClient::new(false));

        let processor = OutboxProcessor::new(
            postgres.clone(),
            qdrant.clone(),
            Duration::from_secs(1),
            10,
            3,
        );

        processor.process_batch().await.unwrap();

        // Verify entity was loaded to Qdrant
        assert_eq!(qdrant.bulk_loaded.lock().unwrap().len(), 1);

        // Verify outbox entry was marked as processed
        assert_eq!(postgres.processed_ids.lock().unwrap().len(), 1);
        assert_eq!(postgres.processed_ids.lock().unwrap()[0], outbox_id);
    }

    #[tokio::test]
    async fn test_successful_delete_processing() {
        let mut entry = create_test_outbox_entry("DELETE", 0);
        let outbox_id = entry.outbox_id;
        let entity_id = entry.entity_id.clone();

        // Modify payload for DELETE operation
        entry.payload = json!({
            "entity_ids": vec![entity_id.clone()],
        });

        let postgres = Arc::new(MockPostgresClient::new(vec![entry]));
        let qdrant = Arc::new(MockQdrantClient::new(false));

        let processor = OutboxProcessor::new(
            postgres.clone(),
            qdrant.clone(),
            Duration::from_secs(1),
            10,
            3,
        );

        processor.process_batch().await.unwrap();

        // Verify entity was deleted from Qdrant
        assert_eq!(qdrant.deleted_ids.lock().unwrap().len(), 1);
        assert_eq!(qdrant.deleted_ids.lock().unwrap()[0], entity_id);

        // Verify outbox entry was marked as processed
        assert_eq!(postgres.processed_ids.lock().unwrap().len(), 1);
        assert_eq!(postgres.processed_ids.lock().unwrap()[0], outbox_id);
    }

    #[tokio::test]
    async fn test_max_retries_exceeded() {
        let entry = create_test_outbox_entry("INSERT", 5);
        let outbox_id = entry.outbox_id;

        let postgres = Arc::new(MockPostgresClient::new(vec![entry]));
        let qdrant = Arc::new(MockQdrantClient::new(false));

        let processor = OutboxProcessor::new(
            postgres.clone(),
            qdrant.clone(),
            Duration::from_secs(1),
            10,
            3, // max_retries = 3, entry has retry_count = 5
        );

        processor.process_batch().await.unwrap();

        // Verify entity was NOT loaded to Qdrant
        assert_eq!(qdrant.bulk_loaded.lock().unwrap().len(), 0);

        // Verify entry was marked as processed despite exceeding retries
        assert_eq!(postgres.processed_ids.lock().unwrap().len(), 1);
        assert_eq!(postgres.processed_ids.lock().unwrap()[0], outbox_id);
    }

    #[tokio::test]
    async fn test_invalid_payload_handling() {
        let mut entry = create_test_outbox_entry("INSERT", 0);
        let outbox_id = entry.outbox_id;

        // Create invalid payload (missing embedding)
        entry.payload = json!({
            "entity": create_test_entity(),
            "qdrant_point_id": Uuid::new_v4().to_string(),
        });

        let postgres = Arc::new(MockPostgresClient::new(vec![entry]));
        let qdrant = Arc::new(MockQdrantClient::new(false));

        let processor = OutboxProcessor::new(
            postgres.clone(),
            qdrant.clone(),
            Duration::from_secs(1),
            10,
            3,
        );

        processor.process_batch().await.unwrap();

        // Verify entity was NOT loaded to Qdrant
        assert_eq!(qdrant.bulk_loaded.lock().unwrap().len(), 0);

        // Verify failure was recorded
        assert_eq!(postgres.failed_ids.lock().unwrap().len(), 1);
        assert_eq!(postgres.failed_ids.lock().unwrap()[0].0, outbox_id);
        assert!(postgres.failed_ids.lock().unwrap()[0]
            .1
            .contains("Missing embedding"));
    }

    #[tokio::test]
    async fn test_unknown_operation() {
        let entry = create_test_outbox_entry("UNKNOWN", 0);
        let outbox_id = entry.outbox_id;

        let postgres = Arc::new(MockPostgresClient::new(vec![entry]));
        let qdrant = Arc::new(MockQdrantClient::new(false));

        let processor = OutboxProcessor::new(
            postgres.clone(),
            qdrant.clone(),
            Duration::from_secs(1),
            10,
            3,
        );

        processor.process_batch().await.unwrap();

        // Verify failure was recorded
        assert_eq!(postgres.failed_ids.lock().unwrap().len(), 1);
        assert_eq!(postgres.failed_ids.lock().unwrap()[0].0, outbox_id);
        assert!(postgres.failed_ids.lock().unwrap()[0]
            .1
            .contains("Unknown operation"));
    }

    #[tokio::test]
    async fn test_batch_processing() {
        let entries = vec![
            create_test_outbox_entry("INSERT", 0),
            create_test_outbox_entry("UPDATE", 0),
            create_test_outbox_entry("INSERT", 0),
        ];

        let postgres = Arc::new(MockPostgresClient::new(entries));
        let qdrant = Arc::new(MockQdrantClient::new(false));

        let processor = OutboxProcessor::new(
            postgres.clone(),
            qdrant.clone(),
            Duration::from_secs(1),
            10,
            3,
        );

        processor.process_batch().await.unwrap();

        // Verify all entities were loaded to Qdrant in batch
        assert_eq!(qdrant.bulk_loaded.lock().unwrap().len(), 3);

        // Verify all outbox entries were marked as processed
        assert_eq!(postgres.processed_ids.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_embedding_dimension_validation() {
        let mut entry = create_test_outbox_entry("INSERT", 0);
        let outbox_id = entry.outbox_id;

        // Create payload with oversized embedding
        let entity = create_test_entity();
        let huge_embedding = vec![0.1; 200_000]; // Exceeds MAX_EMBEDDING_DIM

        entry.payload = json!({
            "entity": entity,
            "embedding": huge_embedding,
            "qdrant_point_id": Uuid::new_v4().to_string(),
        });

        let postgres = Arc::new(MockPostgresClient::new(vec![entry]));
        let qdrant = Arc::new(MockQdrantClient::new(false));

        let processor = OutboxProcessor::new(
            postgres.clone(),
            qdrant.clone(),
            Duration::from_secs(1),
            10,
            3,
        );

        processor.process_batch().await.unwrap();

        // Verify entity was NOT loaded to Qdrant
        assert_eq!(qdrant.bulk_loaded.lock().unwrap().len(), 0);

        // Verify failure was recorded with dimension error
        assert_eq!(postgres.failed_ids.lock().unwrap().len(), 1);
        assert_eq!(postgres.failed_ids.lock().unwrap()[0].0, outbox_id);
        assert!(postgres.failed_ids.lock().unwrap()[0]
            .1
            .contains("exceeds maximum allowed size"));
    }
}
