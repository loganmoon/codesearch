use super::client::{OutboxEntry, PostgresClient, TargetStore};
use crate::{EmbeddedEntity, StorageClient};
use codesearch_core::error::{Error, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

pub struct OutboxProcessor {
    postgres_client: Arc<PostgresClient>,
    qdrant_client: Arc<dyn StorageClient>,
    poll_interval: Duration,
    batch_size: i64,
    max_retries: i32,
}

impl OutboxProcessor {
    pub fn new(
        postgres_client: Arc<PostgresClient>,
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

            for entry in qdrant_entries {
                if entry.retry_count >= self.max_retries {
                    warn!(
                        "Outbox entry {} exceeded max retries ({}), skipping",
                        entry.outbox_id, self.max_retries
                    );
                    continue;
                }

                match self.process_qdrant_entry(&entry).await {
                    Ok(_) => {
                        self.postgres_client
                            .mark_outbox_processed(entry.outbox_id)
                            .await?;
                        debug!("Processed outbox entry {}", entry.outbox_id);
                    }
                    Err(e) => {
                        error!("Failed to process outbox entry {}: {e}", entry.outbox_id);
                        self.postgres_client
                            .record_outbox_failure(entry.outbox_id, &e.to_string())
                            .await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn process_qdrant_entry(&self, entry: &OutboxEntry) -> Result<()> {
        match entry.operation.as_str() {
            "INSERT" | "UPDATE" => {
                let entity: codesearch_core::entities::CodeEntity =
                    serde_json::from_value(entry.payload.clone()).map_err(|e| {
                        Error::storage(format!("Failed to deserialize entity: {e}"))
                    })?;

                // Use placeholder embedding (all zeros) for Phase 4
                // A proper implementation would need the embedding manager
                let embedding = vec![0.0f32; 384];

                let embedded = EmbeddedEntity { entity, embedding };
                self.qdrant_client
                    .bulk_load_entities(vec![embedded])
                    .await?;

                Ok(())
            }
            "DELETE" => {
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
            op => Err(Error::storage(format!("Unknown operation: {op}"))),
        }
    }
}
