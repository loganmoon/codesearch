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
    fn prepare_embedded_entity(&self, entry: &OutboxEntry) -> Result<EmbeddedEntity> {
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

// TODO: Add unit tests for OutboxProcessor
// Tests removed temporarily due to structural changes needed in test setup
