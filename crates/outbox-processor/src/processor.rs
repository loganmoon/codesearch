use codesearch_core::error::{Error, Result};
use codesearch_core::StorageConfig;
use codesearch_storage::{
    create_storage_client_from_config, EmbeddedEntity, Neo4jClientTrait, QdrantConfig,
    StorageClient, ALLOWED_RELATIONSHIP_TYPES,
};
use codesearch_storage::{OutboxEntry, PostgresClientTrait};
use moka::future::Cache;
use sqlx::{Postgres, QueryBuilder};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

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
    storage_config: StorageConfig,
    poll_interval: Duration,
    batch_size: i64,
    max_retries: i32,
    max_embedding_dim: usize,
    client_cache: Cache<String, Arc<dyn StorageClient>>,
    neo4j_client: Arc<Mutex<Option<Arc<dyn Neo4jClientTrait>>>>,
}

impl OutboxProcessor {
    /// Default maximum embedding dimensions to prevent memory exhaustion attacks
    pub const DEFAULT_MAX_EMBEDDING_DIM: usize = 100_000;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        postgres_client: Arc<dyn PostgresClientTrait>,
        qdrant_config: QdrantConfig,
        storage_config: StorageConfig,
        poll_interval: Duration,
        batch_size: i64,
        max_retries: i32,
        max_embedding_dim: usize,
        max_cached_collections: u64,
    ) -> Self {
        Self {
            postgres_client,
            qdrant_config,
            storage_config,
            poll_interval,
            batch_size,
            max_retries,
            max_embedding_dim,
            client_cache: Cache::builder()
                .max_capacity(max_cached_collections)
                .build(),
            neo4j_client: Arc::new(Mutex::new(None)),
        }
    }

    /// Get or create a StorageClient for a specific collection (with caching)
    ///
    /// Clients are cached per collection to avoid recreating them on every poll cycle.
    /// The cache is bounded (default 200 collections) using LRU eviction.
    async fn get_or_create_client_for_collection(
        &self,
        collection_name: &str,
    ) -> Result<Arc<dyn StorageClient>> {
        // Try to get from cache (non-blocking)
        if let Some(client) = self.client_cache.get(collection_name).await {
            return Ok(client);
        }

        // Create new client
        let client =
            create_storage_client_from_config(&self.qdrant_config, collection_name).await?;

        // Insert into cache (LRU will evict oldest entry if at capacity)
        self.client_cache
            .insert(collection_name.to_string(), Arc::clone(&client))
            .await;

        Ok(client)
    }

    /// Get or create Neo4j client (with lazy initialization)
    async fn get_neo4j_client(&self) -> Result<Arc<dyn Neo4jClientTrait>> {
        let mut client_guard = self.neo4j_client.lock().await;

        if let Some(client) = client_guard.as_ref() {
            return Ok(Arc::clone(client));
        }

        // Create new client
        let client = codesearch_storage::create_neo4j_client(&self.storage_config).await?;
        *client_guard = Some(Arc::clone(&client));

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
    /// 6. BM25 statistics are updated atomically with Qdrant writes:
    ///    - INSERT/UPDATE: Stats incremented after successful Qdrant write
    ///    - DELETE: Stats decremented after successful Qdrant deletion
    ///
    /// # Error Handling
    /// - Qdrant failures: Rollback transaction, record failures separately
    /// - Entry preparation failures: Fail entire batch (all-or-nothing)
    /// - Max retry entries: Mark processed without Qdrant write
    pub async fn process_batch(&self) -> Result<()> {
        // Step 1: Begin single transaction for entire batch
        let mut tx = self
            .postgres_client
            .get_pool()
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        // Step 2: Single query across ALL target stores with CTE
        // Uses CTE to select batch with global ordering, then sorts by target_store and collection
        let entries: Vec<OutboxEntry> = sqlx::query_as(
            "WITH batch AS (
                 SELECT outbox_id, repository_id, entity_id, operation, target_store,
                        payload, created_at, processed_at, retry_count, last_error,
                        collection_name, embedding_id
                 FROM entity_outbox
                 WHERE processed_at IS NULL
                 ORDER BY created_at ASC
                 LIMIT $1
                 FOR UPDATE SKIP LOCKED
             )
             SELECT * FROM batch
             ORDER BY target_store, collection_name, created_at",
        )
        .bind(self.batch_size)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to fetch outbox entries: {e}")))?;

        // Step 3: Early return if no work (drop transaction without commit to avoid overhead)
        if entries.is_empty() {
            drop(tx);
            return Ok(());
        }

        // Step 4: Split entries by target_store
        let qdrant_entries: Vec<&OutboxEntry> = entries
            .iter()
            .filter(|e| e.target_store == "qdrant")
            .collect();
        let neo4j_entries: Vec<&OutboxEntry> = entries
            .iter()
            .filter(|e| e.target_store == "neo4j")
            .collect();

        // Process Neo4j entries first
        if !neo4j_entries.is_empty() {
            debug!("Processing {} Neo4j outbox entries", neo4j_entries.len());
            self.process_neo4j_batch(&mut tx, &neo4j_entries).await?;
        }

        // Process Qdrant entries grouped by collection
        if qdrant_entries.is_empty() {
            tx.commit()
                .await
                .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;
            return Ok(());
        }

        let total_entries = qdrant_entries.len();
        let mut start_idx = 0;
        let mut collection_count = 0;

        while start_idx < qdrant_entries.len() {
            collection_count += 1;
            let collection_name = &qdrant_entries[start_idx].collection_name;

            // Find the end index for this collection
            let mut end_idx = start_idx + 1;
            while end_idx < qdrant_entries.len()
                && qdrant_entries[end_idx].collection_name == *collection_name
            {
                end_idx += 1;
            }

            let collection_slice = &qdrant_entries[start_idx..end_idx];

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
                let (repo_token_counts, prep_failed_entries) = match self
                    .write_to_qdrant_insert_update(&storage_client, &insert_update_entries)
                    .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        // Only Qdrant write failures reach here (not preparation failures)
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
                };

                // Record preparation failures (per-entry retry)
                if !prep_failed_entries.is_empty() {
                    warn!(
                        collection = %collection_name,
                        failed_count = prep_failed_entries.len(),
                        "Recording preparation failures for retry"
                    );
                    for (outbox_id, error_message) in prep_failed_entries {
                        self.bulk_record_failures_in_tx(&mut tx, &[outbox_id], &error_message)
                            .await?;
                    }
                }

                // Update BM25 statistics within transaction
                use std::collections::HashMap;
                let mut repo_counts: HashMap<Uuid, Vec<usize>> = HashMap::new();
                for (repo_id, token_count) in repo_token_counts {
                    repo_counts.entry(repo_id).or_default().push(token_count);
                }

                for (repo_id, token_counts) in repo_counts {
                    self.postgres_client
                        .update_bm25_statistics_incremental_in_tx(&mut tx, repo_id, &token_counts)
                        .await?;
                }

                // Mark as processed
                let ids: Vec<Uuid> = insert_update_entries.iter().map(|e| e.outbox_id).collect();
                self.bulk_mark_processed_in_tx(&mut tx, &ids).await?;
            }

            // Process DELETE operations
            if !delete_entries.is_empty() {
                // Extract token counts from DELETE payloads for BM25 stats update
                let mut repo_token_counts: std::collections::HashMap<Uuid, Vec<usize>> =
                    std::collections::HashMap::new();
                for entry in &delete_entries {
                    if let Some(token_counts_json) = entry.payload.get("token_counts") {
                        if let Ok(token_counts) =
                            serde_json::from_value::<Vec<usize>>(token_counts_json.clone())
                        {
                            for token_count in token_counts {
                                repo_token_counts
                                    .entry(entry.repository_id)
                                    .or_default()
                                    .push(token_count);
                            }
                        }
                    }
                }

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

                // Update BM25 statistics after successful deletion (subtract token counts)
                for (repo_id, token_counts) in repo_token_counts {
                    // Convert usize to i64 for statistics calculation
                    let removed_total: i64 =
                        token_counts.iter().try_fold(0i64, |acc, &count| {
                            let count_i64 = i64::try_from(count)
                                .map_err(|_| Error::storage("Token count too large for i64"))?;
                            acc.checked_add(count_i64).ok_or_else(|| {
                                Error::storage("Token count overflow during aggregation")
                            })
                        })?;
                    let removed_count: i64 = i64::try_from(token_counts.len())
                        .map_err(|_| Error::storage("Entity count too large for i64"))?;

                    // Get current stats with lock
                    let stats = self
                        .postgres_client
                        .get_bm25_statistics_in_tx(&mut tx, repo_id)
                        .await?;

                    let updated_total = (stats.total_tokens - removed_total).max(0);
                    let updated_count = (stats.entity_count - removed_count).max(0);

                    let updated_avgdl = if updated_count > 0 {
                        updated_total as f32 / updated_count as f32
                    } else {
                        // Preserve last known avgdl when count becomes 0
                        if stats.avgdl > 0.0 {
                            stats.avgdl
                        } else {
                            50.0
                        }
                    };

                    // Update repository statistics
                    sqlx::query(
                        "UPDATE repositories
                         SET bm25_avgdl = $1, bm25_total_tokens = $2, bm25_entity_count = $3, updated_at = NOW()
                         WHERE repository_id = $4",
                    )
                    .bind(updated_avgdl)
                    .bind(updated_total)
                    .bind(updated_count)
                    .bind(repo_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| Error::storage(format!("Failed to update BM25 statistics after deletion: {e}")))?;
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

        // Step 6: Resolve pending relationships after successful batch processing
        // This runs outside the transaction to avoid holding locks during resolution
        if let Err(e) = self.resolve_pending_relationships().await {
            warn!("Failed to resolve pending relationships: {}", e);
            // Don't fail the batch processing if resolution fails - it will be retried next cycle
        }

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

        // Fetch both dense and sparse embeddings by ID from entity_embeddings table
        let embedding_id = entry
            .embedding_id
            .ok_or_else(|| Error::storage("Missing embedding_id in outbox entry"))?;

        let (dense_embedding, sparse_embedding) = self
            .postgres_client
            .get_embedding_with_sparse_by_id(embedding_id)
            .await?
            .ok_or_else(|| {
                Error::storage(format!(
                    "Embedding ID {embedding_id} not found in entity_embeddings table"
                ))
            })?;

        let sparse_embedding = sparse_embedding.ok_or_else(|| {
            Error::storage(format!(
                "Sparse embedding not found for embedding ID {embedding_id}"
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

        let bm25_token_count = token_counts.first().copied().ok_or_else(|| {
            Error::storage(format!(
                "BM25 token count not found for entity {} in repository {}",
                entity.entity_id, entry.repository_id
            ))
        })?;

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
    /// Returns the embedded entities with their repository IDs and token counts for BM25 stats update.
    ///
    /// Note: Qdrant writes are idempotent (same point ID = same data), making
    /// retries safe if the DB commit fails after a successful Qdrant write.
    async fn write_to_qdrant_insert_update(
        &self,
        storage_client: &Arc<dyn StorageClient>,
        entries: &[&OutboxEntry],
    ) -> Result<(Vec<(Uuid, usize)>, Vec<(Uuid, String)>)> {
        let mut embedded_entities = Vec::with_capacity(entries.len());
        let mut repo_token_counts = Vec::with_capacity(entries.len());
        let mut failed_entries: Vec<(Uuid, String)> = Vec::new();

        // Prepare entities (fetches embeddings from database)
        // Per-entry failure handling: collect failures instead of failing entire batch
        for entry in entries {
            match self.prepare_embedded_entity(entry).await {
                Ok(embedded) => {
                    repo_token_counts.push((entry.repository_id, embedded.bm25_token_count));
                    embedded_entities.push(embedded);
                }
                Err(e) => {
                    error!(
                        outbox_id = %entry.outbox_id,
                        error = %e,
                        "Failed to prepare entry, will retry"
                    );
                    // Record failure for retry, continue processing other entries
                    failed_entries.push((entry.outbox_id, e.to_string()));
                }
            }
        }

        // Bulk load successful entries to Qdrant
        if !embedded_entities.is_empty() {
            storage_client.bulk_load_entities(embedded_entities).await?;
            debug!(
                "Successfully wrote {} entities to Qdrant ({} failed preparation)",
                repo_token_counts.len(),
                failed_entries.len()
            );
        }

        Ok((repo_token_counts, failed_entries))
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

    /// Process a batch of Neo4j outbox entries
    async fn process_neo4j_batch(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        entries: &[&OutboxEntry],
    ) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        // Get Neo4j client
        let neo4j_client = self.get_neo4j_client().await?;

        // Group entries by repository (each repo has its own Neo4j database)
        let mut repo_entries: HashMap<Uuid, Vec<&OutboxEntry>> = HashMap::new();
        for entry in entries {
            repo_entries
                .entry(entry.repository_id)
                .or_default()
                .push(entry);
        }

        for (repository_id, repo_entries) in repo_entries {
            // Get Neo4j database name and switch to it
            let db_name = match self
                .postgres_client
                .get_neo4j_database_name(repository_id)
                .await?
            {
                Some(name) => name,
                None => {
                    // Database doesn't exist yet, create it
                    let db_name = format!("codesearch_{}", repository_id.simple());
                    neo4j_client.create_database(&db_name).await?;
                    self.postgres_client
                        .set_neo4j_database_name(repository_id, &db_name)
                        .await?;
                    db_name
                }
            };

            neo4j_client.use_database(&db_name).await?;

            // Separate INSERT/UPDATE from DELETE operations
            let mut insert_update_entries = Vec::new();
            let mut delete_entries = Vec::new();

            for entry in &repo_entries {
                match entry.operation.as_str() {
                    "INSERT" | "UPDATE" => insert_update_entries.push(*entry),
                    "DELETE" => delete_entries.push(*entry),
                    _ => {}
                }
            }

            let mut processed_ids = Vec::new();
            let mut failed_ids = Vec::new();
            let mut resolved_relationships: Vec<(String, String, String)> = Vec::new();

            // Batch process INSERT/UPDATE operations
            if !insert_update_entries.is_empty() {
                // Parse all entities first, tracking which ones succeed
                let mut entities_to_create = Vec::new();
                let mut entity_entry_map = Vec::new(); // (entity_idx, entry, payload)

                for entry in &insert_update_entries {
                    let payload: serde_json::Value = entry.payload.clone();

                    // Parse entity from payload
                    match serde_json::from_value::<codesearch_core::CodeEntity>(
                        payload["entity"].clone(),
                    ) {
                        Ok(entity) => {
                            let idx = entities_to_create.len();
                            entities_to_create.push(entity);
                            entity_entry_map.push((idx, *entry, payload));
                        }
                        Err(e) => {
                            warn!(
                                "Failed to parse entity from payload for {}: {}",
                                entry.entity_id, e
                            );
                            failed_ids.push(entry.outbox_id);
                        }
                    }
                }

                // Batch create all nodes in Neo4j
                if !entities_to_create.is_empty() {
                    match neo4j_client.batch_create_nodes(&entities_to_create).await {
                        Ok(neo4j_node_ids) => {
                            // Update neo4j_node_id in PostgreSQL and process relationships
                            for (idx, entry, payload) in entity_entry_map {
                                if idx < neo4j_node_ids.len() {
                                    let neo4j_node_id = neo4j_node_ids[idx];

                                    // Store neo4j_node_id in PostgreSQL
                                    if let Err(e) = sqlx::query(
                                        "UPDATE entity_metadata
                                         SET neo4j_node_id = $1
                                         WHERE repository_id = $2 AND entity_id = $3",
                                    )
                                    .bind(neo4j_node_id)
                                    .bind(entry.repository_id)
                                    .bind(&entry.entity_id)
                                    .execute(&mut **tx)
                                    .await
                                    {
                                        warn!(
                                            "Failed to update neo4j_node_id for {}: {}",
                                            entry.entity_id, e
                                        );
                                    }

                                    // Process relationships
                                    let relationships: Vec<serde_json::Value> =
                                        match serde_json::from_value(
                                            payload["relationships"].clone(),
                                        ) {
                                            Ok(rels) => rels,
                                            Err(e) => {
                                                warn!(
                                                    "Failed to parse relationships for entity {}: {}",
                                                    entry.entity_id, e
                                                );
                                                Vec::new()
                                            }
                                        };

                                    for rel in relationships {
                                        // Extract required fields with proper error handling
                                        let rel_type = match rel["type"].as_str() {
                                            Some(t) => t,
                                            None => {
                                                warn!(
                                                "Missing relationship type for entity {}, skipping relationship",
                                                entry.entity_id
                                            );
                                                continue;
                                            }
                                        };
                                        let resolved = rel["resolved"].as_bool().unwrap_or(false);

                                        // Validate relationship type against allowlist (prevents Cypher injection)
                                        if !ALLOWED_RELATIONSHIP_TYPES.contains(&rel_type) {
                                            warn!(
                                            "Invalid relationship type '{}' for entity {}, skipping. Allowed types: {:?}",
                                            rel_type, entry.entity_id, ALLOWED_RELATIONSHIP_TYPES
                                        );
                                            continue;
                                        }

                                        if resolved {
                                            // Resolved relationship: collect for batch creation
                                            let from_id = match rel["from_id"].as_str() {
                                                Some(id) if !id.is_empty() => id,
                                                _ => {
                                                    warn!(
                                                    "Missing or empty from_id for {} relationship on entity {}, skipping",
                                                    rel_type, entry.entity_id
                                                );
                                                    continue;
                                                }
                                            };
                                            let to_id = match rel["to_id"].as_str() {
                                                Some(id) if !id.is_empty() => id,
                                                _ => {
                                                    warn!(
                                                    "Missing or empty to_id for {} relationship on entity {}, skipping",
                                                    rel_type, entry.entity_id
                                                );
                                                    continue;
                                                }
                                            };

                                            // Collect relationship for batch creation after all nodes are processed
                                            resolved_relationships.push((
                                                from_id.to_string(),
                                                to_id.to_string(),
                                                rel_type.to_string(),
                                            ));
                                        } else {
                                            // Unresolved relationship: store as node property for later resolution
                                            let to_id = match rel["to_id"].as_str() {
                                                Some(id) if !id.is_empty() => id,
                                                _ => {
                                                    warn!(
                                                    "Missing or empty to_id for unresolved {} relationship on entity {}, skipping",
                                                    rel_type, entry.entity_id
                                                );
                                                    continue;
                                                }
                                            };
                                            let from_qname = match rel["from_qualified_name"]
                                                .as_str()
                                            {
                                                Some(qname) if !qname.is_empty() => qname,
                                                _ => {
                                                    warn!(
                                                    "Missing or empty from_qualified_name for unresolved {} relationship on entity {}, skipping",
                                                    rel_type, entry.entity_id
                                                );
                                                    continue;
                                                }
                                            };

                                            // Use validated API method instead of graph() escape hatch
                                            if let Err(e) = neo4j_client
                                                .store_unresolved_relationship(
                                                    to_id, rel_type, from_qname,
                                                )
                                                .await
                                            {
                                                debug!(
                                                    "Failed to store unresolved {rel_type}: {e}"
                                                );
                                            }
                                        }
                                    }

                                    processed_ids.push(entry.outbox_id);
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to batch create Neo4j nodes: {}", e);
                            // Mark all entities in this batch as failed
                            for (_, entry, _) in entity_entry_map {
                                failed_ids.push(entry.outbox_id);
                            }
                        }
                    }
                }
            }

            // Process DELETE operations
            for entry in delete_entries {
                let payload: serde_json::Value = entry.payload.clone();
                let entity_id = payload["entity_id"]
                    .as_str()
                    .ok_or_else(|| Error::storage("Missing entity_id in delete payload"))?;

                match neo4j_client.delete_entity_node(entity_id).await {
                    Ok(_) => {
                        processed_ids.push(entry.outbox_id);
                    }
                    Err(e) => {
                        warn!("Failed to delete Neo4j node for {}: {}", entity_id, e);
                        failed_ids.push(entry.outbox_id);
                    }
                }
            }

            // Batch create all resolved relationships (reduces network round-trips)
            if !resolved_relationships.is_empty() {
                if let Err(e) = neo4j_client
                    .batch_create_relationships(&resolved_relationships)
                    .await
                {
                    warn!(
                        "Failed to batch create {} relationships: {}",
                        resolved_relationships.len(),
                        e
                    );
                }
            }

            // Set pending_relationship_resolution flag if we processed any nodes
            // (they may have unresolved relationships that need resolution)
            if !insert_update_entries.is_empty() && !processed_ids.is_empty() {
                if let Err(e) = self
                    .postgres_client
                    .set_pending_relationship_resolution(repository_id, true)
                    .await
                {
                    warn!(
                        "Failed to set pending_relationship_resolution for repository {}: {}",
                        repository_id, e
                    );
                }
            }

            // Mark processed entries
            if !processed_ids.is_empty() {
                self.bulk_mark_processed_in_tx(tx, &processed_ids).await?;
            }

            // Mark failed entries (increment retry count)
            if !failed_ids.is_empty() {
                for id in failed_ids {
                    sqlx::query(
                        "UPDATE entity_outbox
                         SET retry_count = retry_count + 1,
                             last_error = $1
                         WHERE outbox_id = $2",
                    )
                    .bind("Failed to process Neo4j entry")
                    .bind(id)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| Error::storage(format!("Failed to update retry count: {e}")))?;
                }
            }
        }

        Ok(())
    }

    /// Resolve pending relationships for repositories that need it
    ///
    /// This method checks for repositories with the pending_relationship_resolution flag set
    /// and runs all relationship resolvers to create relationship edges in Neo4j.
    ///
    /// Called after processing batches of entity outbox entries to ensure relationships
    /// are resolved as entities become available.
    async fn resolve_pending_relationships(&self) -> Result<()> {
        use crate::neo4j_relationship_resolver::{
            resolve_contains_relationships, resolve_relationships_generic, CallGraphResolver,
            ImportsResolver, InheritanceResolver, TraitImplResolver, TypeUsageResolver,
        };

        // Get repositories that need resolution
        let repo_ids = self
            .postgres_client
            .get_repositories_with_pending_resolution()
            .await?;

        if repo_ids.is_empty() {
            return Ok(());
        }

        debug!(
            "Found {} repositories with pending relationship resolution",
            repo_ids.len()
        );

        // Get Neo4j client
        let neo4j_client = self.get_neo4j_client().await?;

        // Resolve relationships for each repository
        for repository_id in repo_ids {
            info!("Resolving relationships for repository {}", repository_id);

            // Ensure Neo4j database exists and is selected
            let db_name = match self
                .postgres_client
                .get_neo4j_database_name(repository_id)
                .await?
            {
                Some(name) => name,
                None => {
                    warn!(
                        "No Neo4j database found for repository {}, skipping resolution",
                        repository_id
                    );
                    // Clear the flag since we can't resolve without a database
                    let _ = self
                        .postgres_client
                        .set_pending_relationship_resolution(repository_id, false)
                        .await;
                    continue;
                }
            };

            neo4j_client.use_database(&db_name).await?;

            // Run all resolvers
            let resolvers: Vec<Box<dyn crate::neo4j_relationship_resolver::RelationshipResolver>> = vec![
                Box::new(TraitImplResolver),
                Box::new(InheritanceResolver),
                Box::new(TypeUsageResolver),
                Box::new(CallGraphResolver),
                Box::new(ImportsResolver),
            ];

            for resolver in resolvers {
                if let Err(e) = resolve_relationships_generic(
                    &self.postgres_client,
                    neo4j_client.as_ref(),
                    repository_id,
                    resolver.as_ref(),
                )
                .await
                {
                    warn!(
                        "Failed to resolve {} for repository {}: {}",
                        resolver.name(),
                        repository_id,
                        e
                    );
                    // Continue with other resolvers even if one fails
                }
            }

            // Resolve CONTAINS relationships (special case with batch optimization)
            if let Err(e) = resolve_contains_relationships(
                &self.postgres_client,
                neo4j_client.as_ref(),
                repository_id,
            )
            .await
            {
                warn!(
                    "Failed to resolve CONTAINS relationships for repository {}: {}",
                    repository_id, e
                );
            }

            // Clear the pending flag and set graph_ready
            if let Err(e) = self
                .postgres_client
                .set_pending_relationship_resolution(repository_id, false)
                .await
            {
                warn!(
                    "Failed to clear pending_relationship_resolution flag for repository {}: {}",
                    repository_id, e
                );
            }

            if let Err(e) = self
                .postgres_client
                .set_graph_ready(repository_id, true)
                .await
            {
                warn!(
                    "Failed to set graph_ready flag for repository {}: {}",
                    repository_id, e
                );
            }

            info!(
                "Completed relationship resolution for repository {}",
                repository_id
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
