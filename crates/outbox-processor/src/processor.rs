use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
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

const DELIM: &str = " ";

/// Extract embeddable content from a CodeEntity
fn extract_embedding_content(entity: &CodeEntity) -> String {
    // Calculate accurate capacity
    let estimated_size = entity.name.len()
        + entity.qualified_name.len()
        + entity.documentation_summary.as_ref().map_or(0, |s| s.len())
        + entity.content.as_ref().map_or(0, |s| s.len())
        + 100; // Extra padding for delimiters and formatting

    let mut content = String::with_capacity(estimated_size);

    // Add entity type and name
    content.push_str(&format!("{} {}", entity.entity_type, entity.name));
    chain_delim(&mut content, &entity.qualified_name);

    // Add documentation summary if available
    if let Some(doc) = &entity.documentation_summary {
        chain_delim(&mut content, doc);
    }

    // Add signature information for functions/methods
    if let Some(sig) = &entity.signature {
        for (name, type_opt) in &sig.parameters {
            content.push_str(DELIM);
            content.push_str(name);
            if let Some(param_type) = type_opt {
                content.push_str(": ");
                content.push_str(param_type);
            }
        }

        if let Some(ret_type) = &sig.return_type {
            chain_delim(&mut content, &format!("-> {ret_type}"));
        }
    }

    // Add the full entity content (most important for semantic search)
    if let Some(entity_content) = &entity.content {
        chain_delim(&mut content, entity_content);
    }

    content
}

fn chain_delim(out_str: &mut String, text: &str) {
    out_str.push_str(DELIM);
    out_str.push_str(text);
}

/// Context information for Qdrant write failure handling
struct FailureContext<'a> {
    operation_type: &'a str,
    collection_name: &'a str,
    entry_count: usize,
    first_entity_id: Option<&'a str>,
}

pub struct OutboxProcessor {
    postgres_client: Arc<dyn PostgresClientTrait>,
    qdrant_config: QdrantConfig,
    poll_interval: Duration,
    batch_size: i64,
    max_retries: i32,
    max_embedding_dim: usize,
    client_cache: Arc<DashMap<String, Arc<dyn StorageClient>>>,
}

impl OutboxProcessor {
    /// Default maximum embedding dimensions to prevent memory exhaustion attacks
    pub const DEFAULT_MAX_EMBEDDING_DIM: usize = 100_000;

    pub fn new(
        postgres_client: Arc<dyn PostgresClientTrait>,
        qdrant_config: QdrantConfig,
        poll_interval: Duration,
        batch_size: i64,
        max_retries: i32,
        max_embedding_dim: usize,
    ) -> Self {
        Self {
            postgres_client,
            qdrant_config,
            poll_interval,
            batch_size,
            max_retries,
            max_embedding_dim,
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

    /// Handle Qdrant write failure by rolling back transaction and recording failures
    ///
    /// This helper consolidates error handling for both INSERT/UPDATE and DELETE operations.
    /// It ensures atomic failure handling by:
    /// 1. Rolling back the main transaction
    /// 2. Recording failures in a separate transaction
    async fn handle_qdrant_write_failure(
        &self,
        tx: sqlx::Transaction<'_, Postgres>,
        entry_ids: Vec<Uuid>,
        error: Error,
        context: FailureContext<'_>,
    ) -> Result<()> {
        error!(
            operation = context.operation_type,
            collection = %context.collection_name,
            entry_count = context.entry_count,
            first_entity_id = ?context.first_entity_id,
            error = %error,
            "Failed to write to Qdrant, rolling back entire batch"
        );

        tx.rollback()
            .await
            .map_err(|e| Error::storage(format!("Failed to rollback transaction: {e}")))?;

        let mut failure_tx = self
            .postgres_client
            .get_pool()
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin failure transaction: {e}")))?;
        self.bulk_record_failures_in_tx(&mut failure_tx, &entry_ids, &error.to_string())
            .await?;
        failure_tx
            .commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit failure transaction: {e}")))?;

        Err(error)
    }

    /// Process a single batch of outbox entries using a single transaction.
    ///
    /// # Guarantees
    /// 1. All entries processed within a single PostgreSQL transaction
    /// 2. Qdrant writes occur BEFORE PostgreSQL commits (write-ahead pattern)
    /// 3. On Qdrant failure, entire batch rolls back
    /// 4. Entries exceeding retry limits are marked processed
    /// 5. Global ordering by created_at ensures fairness across collections
    ///
    /// # Error Handling
    /// - Qdrant failures: Rollback transaction, record failures separately
    /// - Entry preparation failures: Fail entire batch (all-or-nothing)
    /// - Max retry entries: Mark processed without Qdrant write
    async fn process_batch(&self) -> Result<()> {
        // Step 1: Begin single transaction for entire batch
        let mut tx = self
            .postgres_client
            .get_pool()
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        // Step 2: Single query across ALL collections with CTE
        // Uses CTE to select batch with global ordering, then sorts by collection for grouping
        let entries: Vec<OutboxEntry> = sqlx::query_as(
            "WITH batch AS (
                 SELECT outbox_id, repository_id, entity_id, operation, target_store,
                        payload, created_at, processed_at, retry_count, last_error,
                        collection_name, embedding_id
                 FROM entity_outbox
                 WHERE target_store = $1
                   AND processed_at IS NULL
                 ORDER BY created_at ASC
                 LIMIT $2
                 FOR UPDATE SKIP LOCKED
             )
             SELECT * FROM batch
             ORDER BY collection_name, created_at",
        )
        .bind(TargetStore::Qdrant.to_string())
        .bind(self.batch_size)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to fetch outbox entries: {e}")))?;

        // Step 3: Early return if no work (drop transaction without commit to avoid overhead)
        if entries.is_empty() {
            drop(tx);
            return Ok(());
        }

        // Step 4: Process entries grouped by collection using slices (zero-copy)
        // Entries are already sorted by collection_name, find slice boundaries
        let total_entries = entries.len();
        let mut start_idx = 0;
        let mut collection_count = 0;

        while start_idx < entries.len() {
            collection_count += 1;
            let collection_name = &entries[start_idx].collection_name;

            // Find the end index for this collection
            let mut end_idx = start_idx + 1;
            while end_idx < entries.len() && entries[end_idx].collection_name == *collection_name {
                end_idx += 1;
            }

            let collection_slice = &entries[start_idx..end_idx];

            debug!(
                collection = %collection_name,
                entries_in_collection = collection_slice.len(),
                "Processing collection"
            );

            // Get or create Qdrant client (cached)
            let storage_client = self
                .get_or_create_client_for_collection(collection_name)
                .await?;

            // Separate INSERT/UPDATE from DELETE, track failures
            let mut insert_update_entries: Vec<&OutboxEntry> = Vec::new();
            let mut delete_entries: Vec<&OutboxEntry> = Vec::new();
            let mut failed_entry_ids = Vec::new();

            for entry in collection_slice {
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

                // Categorize by operation
                match entry.operation.as_str() {
                    "INSERT" | "UPDATE" => insert_update_entries.push(entry),
                    "DELETE" => delete_entries.push(entry),
                    _ => {
                        error!(
                            collection = %collection_name,
                            operation = %entry.operation,
                            "Unknown operation type"
                        );
                        failed_entry_ids.push(entry.outbox_id);
                    }
                }
            }

            // Bulk mark entries that exceeded retry count
            if !failed_entry_ids.is_empty() {
                self.bulk_mark_processed_in_tx(&mut tx, &failed_entry_ids)
                    .await?;
            }

            // Process INSERT/UPDATE operations
            if !insert_update_entries.is_empty() {
                if let Err(e) = self
                    .write_to_qdrant_insert_update(&storage_client, &insert_update_entries)
                    .await
                {
                    let ids: Vec<Uuid> =
                        insert_update_entries.iter().map(|e| e.outbox_id).collect();
                    let context = FailureContext {
                        operation_type: "INSERT/UPDATE",
                        collection_name,
                        entry_count: insert_update_entries.len(),
                        first_entity_id: insert_update_entries
                            .first()
                            .map(|e| e.entity_id.as_str()),
                    };
                    return self.handle_qdrant_write_failure(tx, ids, e, context).await;
                }

                // Mark as processed
                let ids: Vec<Uuid> = insert_update_entries.iter().map(|e| e.outbox_id).collect();
                self.bulk_mark_processed_in_tx(&mut tx, &ids).await?;
            }

            // Process DELETE operations
            if !delete_entries.is_empty() {
                if let Err(e) = self
                    .write_to_qdrant_delete(&storage_client, &delete_entries)
                    .await
                {
                    let ids: Vec<Uuid> = delete_entries.iter().map(|e| e.outbox_id).collect();
                    let context = FailureContext {
                        operation_type: "DELETE",
                        collection_name,
                        entry_count: delete_entries.len(),
                        first_entity_id: delete_entries.first().map(|e| e.entity_id.as_str()),
                    };
                    return self.handle_qdrant_write_failure(tx, ids, e, context).await;
                }

                // Mark as processed
                let ids: Vec<Uuid> = delete_entries.iter().map(|e| e.outbox_id).collect();
                self.bulk_mark_processed_in_tx(&mut tx, &ids).await?;
            }

            debug!(
                collection = %collection_name,
                processed = insert_update_entries.len() + delete_entries.len(),
                failed = failed_entry_ids.len(),
                "Processed collection entries"
            );

            // Move to next collection
            start_idx = end_idx;
        }

        // Step 5: Commit entire batch
        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        debug!(
            total_entries = total_entries,
            collections = collection_count,
            "Successfully processed entire batch"
        );
        Ok(())
    }

    /// Prepare an embedded entity from an outbox entry (fetches embedding by ID)
    pub(crate) async fn prepare_embedded_entity(
        &self,
        entry: &OutboxEntry,
    ) -> Result<EmbeddedEntity> {
        // Extract entity from payload
        let entity: codesearch_core::entities::CodeEntity = serde_json::from_value(
            entry
                .payload
                .get("entity")
                .ok_or_else(|| Error::storage("Missing entity in payload"))?
                .clone(),
        )
        .map_err(|e| Error::storage(format!("Failed to deserialize entity: {e}")))?;

        // Fetch dense embedding by ID from entity_embeddings table
        let embedding_id = entry
            .embedding_id
            .ok_or_else(|| Error::storage("Missing embedding_id in outbox entry"))?;

        let dense_embedding = self
            .postgres_client
            .get_embedding_by_id(embedding_id)
            .await?
            .ok_or_else(|| {
                Error::storage(format!(
                    "Embedding ID {embedding_id} not found in entity_embeddings table"
                ))
            })?;

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
        if dense_embedding.len() > self.max_embedding_dim {
            return Err(Error::storage(format!(
                "Embedding dimensions {} exceeds maximum allowed size of {}",
                dense_embedding.len(),
                self.max_embedding_dim
            )));
        }

        // Fetch token count from entity_metadata
        let token_counts = self
            .postgres_client
            .get_entity_token_counts(&[(entry.repository_id, entity.entity_id.clone())])
            .await?;

        let bm25_token_count = token_counts.first().copied().unwrap_or(50); // Default fallback if not found

        // Get current avgdl for the repository
        let stats = self
            .postgres_client
            .get_bm25_statistics(entry.repository_id)
            .await?;

        // Generate sparse embedding using BM25 provider
        let sparse_manager = codesearch_embeddings::create_sparse_manager(stats.avgdl)
            .map_err(|e| Error::storage(format!("Failed to create sparse manager: {e}")))?;

        let content = extract_embedding_content(&entity);
        let sparse_embeddings = sparse_manager
            .embed_sparse(vec![content.as_str()])
            .await
            .map_err(|e| Error::storage(format!("Failed to generate sparse embedding: {e}")))?;

        let sparse_embedding = sparse_embeddings
            .into_iter()
            .next()
            .ok_or_else(|| Error::storage("No sparse embedding returned"))?
            .ok_or_else(|| Error::storage("Sparse embedding is None"))?;

        Ok(EmbeddedEntity {
            entity,
            dense_embedding,
            sparse_embedding,
            bm25_token_count,
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
    ///
    /// Note: Qdrant writes are idempotent (same point ID = same data), making
    /// retries safe if the DB commit fails after a successful Qdrant write.
    async fn write_to_qdrant_insert_update(
        &self,
        storage_client: &Arc<dyn StorageClient>,
        entries: &[&OutboxEntry],
    ) -> Result<()> {
        let mut embedded_entities = Vec::with_capacity(entries.len());

        // Prepare entities (fetches embeddings from database)
        for entry in entries {
            match self.prepare_embedded_entity(entry).await {
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
        entries: &[&OutboxEntry],
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
