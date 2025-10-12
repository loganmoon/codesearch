use codesearch_core::error::{Error, Result};
use codesearch_storage::{
    create_storage_client_from_config, EmbeddedEntity, QdrantConfig, StorageClient,
};
use codesearch_storage::{OutboxEntry, PostgresClientTrait, TargetStore};
use dashmap::DashMap;
use sqlx::{Postgres, QueryBuilder};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub struct OutboxProcessor {
    postgres_client: Arc<dyn PostgresClientTrait>,
    qdrant_config: QdrantConfig,
    poll_interval: Duration,
    batch_size: i64,
    max_retries: i32,
    client_cache: Arc<DashMap<String, Arc<dyn StorageClient>>>,
}

impl OutboxProcessor {
    pub fn new(
        postgres_client: Arc<dyn PostgresClientTrait>,
        qdrant_config: QdrantConfig,
        poll_interval: Duration,
        batch_size: i64,
        max_retries: i32,
    ) -> Self {
        Self {
            postgres_client,
            qdrant_config,
            poll_interval,
            batch_size,
            max_retries,
            client_cache: Arc::new(DashMap::new()),
        }
    }

    /// Get or create a StorageClient for a specific collection (with caching)
    ///
    /// Clients are cached per collection to avoid recreating them on every poll cycle.
    async fn get_or_create_client_for_collection(
        &self,
        collection_name: &str,
    ) -> Result<Arc<dyn StorageClient>> {
        // Check cache first
        if let Some(client) = self.client_cache.get(collection_name) {
            return Ok(Arc::clone(client.value()));
        }

        // Create new client and cache it
        let client =
            create_storage_client_from_config(&self.qdrant_config, collection_name).await?;
        self.client_cache
            .insert(collection_name.to_string(), Arc::clone(&client));

        Ok(client)
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
        // Query 1: Get distinct collections with pending entries (1 query, not N)
        let collections_with_work: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT collection_name
             FROM entity_outbox
             WHERE target_store = $1 AND processed_at IS NULL
             LIMIT 100",
        )
        .bind(TargetStore::Qdrant.to_string())
        .fetch_all(self.postgres_client.get_pool())
        .await
        .map_err(|e| Error::storage(format!("Failed to query collections: {e}")))?;

        if collections_with_work.is_empty() {
            return Ok(());
        }

        debug!(
            "Found {} collections with pending outbox entries",
            collections_with_work.len()
        );

        // Process each collection's batch
        for collection_name in collections_with_work {
            if let Err(e) = self.process_collection_batch(&collection_name).await {
                error!(
                    collection = %collection_name,
                    error = %e,
                    "Failed to process collection batch"
                );
                // Continue to next collection instead of failing entire batch
            }
        }

        Ok(())
    }

    /// Process all pending entries for a single collection in a transaction
    ///
    /// CRITICAL: Uses SELECT FOR UPDATE to lock entries, writes to Qdrant,
    /// then marks as processed - all in a single transaction. This ensures
    /// work is only marked complete after successful Qdrant write.
    async fn process_collection_batch(&self, collection_name: &str) -> Result<()> {
        // Start transaction
        let mut tx = self
            .postgres_client
            .get_pool()
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        // Query 2: Lock and fetch unprocessed entries for THIS collection
        // CRITICAL: SELECT FOR UPDATE locks the rows for the duration of the transaction
        let entries: Vec<OutboxEntry> = sqlx::query_as(
            "SELECT outbox_id, repository_id, entity_id, operation, target_store, payload,
                    created_at, processed_at, retry_count, last_error, collection_name
             FROM entity_outbox
             WHERE target_store = $1
               AND collection_name = $2
               AND processed_at IS NULL
             ORDER BY created_at ASC
             LIMIT $3
             FOR UPDATE SKIP LOCKED",
        )
        .bind(TargetStore::Qdrant.to_string())
        .bind(collection_name)
        .bind(self.batch_size)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to fetch outbox entries: {e}")))?;

        if entries.is_empty() {
            // No work to do, commit empty transaction
            tx.commit()
                .await
                .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;
            return Ok(());
        }

        debug!(
            collection = %collection_name,
            entry_count = entries.len(),
            "Processing outbox entries for collection"
        );

        // Get or create cached client for this collection
        let storage_client = self
            .get_or_create_client_for_collection(collection_name)
            .await?;

        // Separate INSERT/UPDATE from DELETE (same as before)
        let mut insert_update_entries = Vec::new();
        let mut delete_entries = Vec::new();
        let mut failed_entry_ids = Vec::new();

        for entry in entries {
            // Check retry limit
            if entry.retry_count >= self.max_retries {
                warn!(
                    outbox_id = %entry.outbox_id,
                    retry_count = entry.retry_count,
                    "Entry exceeded max retries, marking as processed"
                );
                failed_entry_ids.push(entry.outbox_id);
                continue;
            }

            match entry.operation.as_str() {
                "INSERT" | "UPDATE" => insert_update_entries.push(entry),
                "DELETE" => delete_entries.push(entry),
                _ => {
                    error!(operation = %entry.operation, "Unknown operation");
                    failed_entry_ids.push(entry.outbox_id);
                }
            }
        }

        // Bulk mark failed entries as processed (within transaction)
        if !failed_entry_ids.is_empty() {
            self.bulk_mark_processed_in_tx(&mut tx, &failed_entry_ids)
                .await?;
        }

        // Process INSERT/UPDATE entries
        if !insert_update_entries.is_empty() {
            // Write to Qdrant first (outside transaction)
            if let Err(e) = self
                .write_to_qdrant_insert_update(&storage_client, &insert_update_entries)
                .await
            {
                error!(error = %e, "Failed to write to Qdrant, rolling back transaction");
                tx.rollback()
                    .await
                    .map_err(|e| Error::storage(format!("Failed to rollback: {e}")))?;
                // Record failures for these entries (in new transaction)
                let ids: Vec<Uuid> = insert_update_entries.iter().map(|e| e.outbox_id).collect();
                let mut failure_tx =
                    self.postgres_client.get_pool().begin().await.map_err(|e| {
                        Error::storage(format!("Failed to begin failure transaction: {e}"))
                    })?;
                self.bulk_record_failures_in_tx(&mut failure_tx, &ids, &e.to_string())
                    .await?;
                failure_tx.commit().await.map_err(|e| {
                    Error::storage(format!("Failed to commit failure transaction: {e}"))
                })?;
                return Err(e);
            }

            // Qdrant write succeeded, mark as processed (within transaction)
            let ids: Vec<Uuid> = insert_update_entries.iter().map(|e| e.outbox_id).collect();
            self.bulk_mark_processed_in_tx(&mut tx, &ids).await?;
        }

        // Process DELETE entries
        if !delete_entries.is_empty() {
            // Delete from Qdrant first (outside transaction)
            if let Err(e) = self
                .write_to_qdrant_delete(&storage_client, &delete_entries)
                .await
            {
                error!(error = %e, "Failed to delete from Qdrant, rolling back transaction");
                tx.rollback()
                    .await
                    .map_err(|e| Error::storage(format!("Failed to rollback: {e}")))?;
                // Record failures (in new transaction)
                let ids: Vec<Uuid> = delete_entries.iter().map(|e| e.outbox_id).collect();
                let mut failure_tx =
                    self.postgres_client.get_pool().begin().await.map_err(|e| {
                        Error::storage(format!("Failed to begin failure transaction: {e}"))
                    })?;
                self.bulk_record_failures_in_tx(&mut failure_tx, &ids, &e.to_string())
                    .await?;
                failure_tx.commit().await.map_err(|e| {
                    Error::storage(format!("Failed to commit failure transaction: {e}"))
                })?;
                return Err(e);
            }

            // Qdrant delete succeeded, mark as processed (within transaction)
            let ids: Vec<Uuid> = delete_entries.iter().map(|e| e.outbox_id).collect();
            self.bulk_mark_processed_in_tx(&mut tx, &ids).await?;
        }

        // Commit transaction - all entries successfully processed
        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        info!(
            collection = %collection_name,
            entries_processed = insert_update_entries.len() + delete_entries.len() + failed_entry_ids.len(),
            "Successfully processed batch"
        );

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

    /// Mark multiple outbox entries as processed within an existing transaction
    ///
    /// CRITICAL: This operates within the provided transaction, allowing
    /// atomicity with Qdrant writes.
    async fn bulk_mark_processed_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        outbox_ids: &[Uuid],
    ) -> Result<()> {
        if outbox_ids.is_empty() {
            return Ok(());
        }

        let mut query_builder: QueryBuilder<Postgres> =
            QueryBuilder::new("UPDATE entity_outbox SET processed_at = NOW() WHERE outbox_id IN (");

        let mut separated = query_builder.separated(", ");
        for id in outbox_ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");

        let result = query_builder
            .build()
            .execute(&mut **tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to bulk mark processed: {e}")))?;

        debug!(
            "Marked {} entries as processed (in transaction)",
            result.rows_affected()
        );
        Ok(())
    }

    /// Record failures for multiple outbox entries within a transaction
    ///
    /// CRITICAL: This operates within the provided transaction, ensuring
    /// atomic failure recording.
    async fn bulk_record_failures_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        outbox_ids: &[Uuid],
        error_message: &str,
    ) -> Result<()> {
        if outbox_ids.is_empty() {
            return Ok(());
        }

        // Truncate error message to prevent payload bloat
        let error_message = if error_message.len() > 1000 {
            &error_message[..1000]
        } else {
            error_message
        };

        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "UPDATE entity_outbox
             SET retry_count = retry_count + 1,
                 last_error = ",
        );
        query_builder.push_bind(error_message);
        query_builder.push(" WHERE outbox_id IN (");

        let mut separated = query_builder.separated(", ");
        for id in outbox_ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");

        let result = query_builder
            .build()
            .execute(&mut **tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to bulk record failures: {e}")))?;

        debug!(
            "Recorded failures for {} entries (in transaction)",
            result.rows_affected()
        );
        Ok(())
    }

    /// Write INSERT/UPDATE entries to Qdrant (without DB marking)
    ///
    /// CRITICAL: This ONLY writes to Qdrant. The caller is responsible for
    /// marking entries as processed in the transaction AFTER this succeeds.
    async fn write_to_qdrant_insert_update(
        &self,
        storage_client: &Arc<dyn StorageClient>,
        entries: &[OutboxEntry],
    ) -> Result<()> {
        let mut embedded_entities = Vec::with_capacity(entries.len());

        // Prepare entities without cloning embeddings
        for entry in entries {
            match self.prepare_embedded_entity(entry) {
                Ok(embedded) => {
                    embedded_entities.push(embedded);
                }
                Err(e) => {
                    error!(
                        outbox_id = %entry.outbox_id,
                        error = %e,
                        "Failed to prepare entry"
                    );
                    // If ANY entry fails preparation, fail the entire batch
                    // This ensures transactional all-or-nothing behavior
                    return Err(Error::storage(format!(
                        "Failed to prepare entry {}: {e}",
                        entry.outbox_id
                    )));
                }
            }
        }

        // Bulk load to Qdrant
        if !embedded_entities.is_empty() {
            storage_client.bulk_load_entities(embedded_entities).await?;
            debug!("Successfully wrote {} entities to Qdrant", entries.len());
        }

        Ok(())
    }

    /// Delete entries from Qdrant (without DB marking)
    ///
    /// CRITICAL: This ONLY deletes from Qdrant. The caller is responsible for
    /// marking entries as processed in the transaction AFTER this succeeds.
    async fn write_to_qdrant_delete(
        &self,
        storage_client: &Arc<dyn StorageClient>,
        entries: &[OutboxEntry],
    ) -> Result<()> {
        // Collect all entity_ids from all DELETE entries
        let mut all_entity_ids = Vec::new();

        for entry in entries {
            if let Some(ids) = entry.payload.get("entity_ids") {
                if let Ok(ids_vec) = serde_json::from_value::<Vec<String>>(ids.clone()) {
                    all_entity_ids.extend(ids_vec);
                } else {
                    error!(outbox_id = %entry.outbox_id, "Invalid DELETE payload");
                    return Err(Error::storage(format!(
                        "Invalid DELETE payload for entry {}",
                        entry.outbox_id
                    )));
                }
            } else {
                // Fallback: single entity_id
                all_entity_ids.push(entry.entity_id.clone());
            }
        }

        // Bulk delete from Qdrant
        if !all_entity_ids.is_empty() {
            storage_client.delete_entities(&all_entity_ids).await?;
            info!(
                "Successfully deleted {} entities from Qdrant",
                all_entity_ids.len()
            );
        }

        Ok(())
    }
}

/*
 * Tests have been moved to tests/integration_tests.rs
 * They now use testcontainers for real PostgreSQL and Qdrant instances
 * to properly test SQL-based transactions (SELECT FOR UPDATE, etc.)
 */
