use codesearch_core::error::{Error, Result};
use codesearch_core::StorageConfig;
use codesearch_storage::{
    create_storage_client_from_config, EmbeddedEntity, Neo4jClientTrait, QdrantConfig,
    StorageClient,
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
            match self.process_batch().await {
                Ok(had_work) => {
                    // Only sleep when queue was empty; process immediately when there's a backlog
                    if !had_work {
                        sleep(self.poll_interval).await;
                    }
                }
                Err(e) => {
                    error!("Outbox processing error: {e}");
                    // Sleep on error to avoid tight error loops
                    sleep(self.poll_interval).await;
                }
            }
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
    /// # Returns
    /// `Ok(true)` if entries were processed (potentially more waiting)
    /// `Ok(false)` if queue was empty (caller should sleep)
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
    pub async fn process_batch(&self) -> Result<bool> {
        let batch_start = std::time::Instant::now();

        // Step 1: Begin single transaction for entire batch
        let tx_begin_start = std::time::Instant::now();
        let mut tx = self
            .postgres_client
            .get_pool()
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;
        debug!(
            elapsed_ms = tx_begin_start.elapsed().as_millis() as u64,
            "Transaction BEGIN completed"
        );

        // Step 2: Single query across ALL target stores with CTE
        // Uses CTE to select batch with global ordering, then sorts by target_store and collection
        let fetch_start = std::time::Instant::now();
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
        debug!(
            elapsed_ms = fetch_start.elapsed().as_millis() as u64,
            count = entries.len(),
            batch_size = self.batch_size,
            "Outbox entry fetch completed"
        );

        // Step 3: Early return if no work (drop transaction without commit to avoid overhead)
        if entries.is_empty() {
            drop(tx);
            return Ok(false); // No work, caller should sleep
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
            let neo4j_start = std::time::Instant::now();
            debug!("Processing {} Neo4j outbox entries", neo4j_entries.len());
            self.process_neo4j_batch(&mut tx, &neo4j_entries).await?;
            info!(
                elapsed_ms = neo4j_start.elapsed().as_millis() as u64,
                count = neo4j_entries.len(),
                "Neo4j batch processing completed"
            );
        }

        // Process Qdrant entries grouped by collection
        if qdrant_entries.is_empty() {
            tx.commit()
                .await
                .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;
            return Ok(true); // Had Neo4j work, check for more
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
                        self.handle_qdrant_write_failure(tx, ids, e, context)
                            .await?;
                        unreachable!("handle_qdrant_write_failure always returns Err");
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
                    self.handle_qdrant_write_failure(tx, ids, e, context)
                        .await?;
                    unreachable!("handle_qdrant_write_failure always returns Err");
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
        let commit_start = std::time::Instant::now();
        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;
        debug!(
            elapsed_ms = commit_start.elapsed().as_millis() as u64,
            "Transaction COMMIT completed"
        );

        info!(
            total_entries = total_entries,
            qdrant_entries = qdrant_entries.len(),
            neo4j_entries = neo4j_entries.len(),
            collections = collection_count,
            total_elapsed_ms = batch_start.elapsed().as_millis() as u64,
            "Batch processing completed"
        );

        Ok(true) // Work was done, check for more immediately
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

        // Collect all embedding IDs and entity refs for batch queries
        let mut embedding_ids: Vec<i64> = Vec::with_capacity(entries.len());
        let mut entity_refs: Vec<(Uuid, String)> = Vec::with_capacity(entries.len());
        // Store parsed entities to avoid double parsing (parse once in first pass, reuse in second)
        let mut valid_entries: Vec<(&OutboxEntry, codesearch_core::entities::CodeEntity)> =
            Vec::with_capacity(entries.len());

        // First pass: collect IDs and validate entries
        for entry in entries {
            // Extract embedding_id
            let embedding_id = match entry.embedding_id {
                Some(id) => id,
                None => {
                    failed_entries.push((
                        entry.outbox_id,
                        "Missing embedding_id in outbox entry".to_string(),
                    ));
                    continue;
                }
            };

            // Extract entity from payload to get entity_id for token count lookup
            let entity_json = match entry.payload.get("entity") {
                Some(e) => e,
                None => {
                    failed_entries.push((entry.outbox_id, "Missing entity in payload".to_string()));
                    continue;
                }
            };

            let entity: codesearch_core::entities::CodeEntity =
                match serde_json::from_value(entity_json.clone()) {
                    Ok(e) => e,
                    Err(e) => {
                        failed_entries.push((
                            entry.outbox_id,
                            format!("Failed to deserialize entity: {e}"),
                        ));
                        continue;
                    }
                };

            embedding_ids.push(embedding_id);
            entity_refs.push((entry.repository_id, entity.entity_id.clone()));
            valid_entries.push((entry, entity));
        }

        // Batch fetch all embeddings in one query (instead of N queries)
        let embeddings_start = std::time::Instant::now();
        let embeddings_map = self
            .postgres_client
            .get_embeddings_with_sparse_by_ids(&embedding_ids)
            .await?;
        debug!(
            elapsed_ms = embeddings_start.elapsed().as_millis() as u64,
            count = embedding_ids.len(),
            "Batch embeddings fetch completed"
        );

        // Batch fetch all token counts in one query (instead of N queries)
        let token_counts_start = std::time::Instant::now();
        let token_counts_vec = self
            .postgres_client
            .get_entity_token_counts(&entity_refs)
            .await?;
        debug!(
            elapsed_ms = token_counts_start.elapsed().as_millis() as u64,
            count = entity_refs.len(),
            "Batch token counts fetch completed"
        );

        // Build token counts lookup map
        let mut token_counts_map: HashMap<(Uuid, String), usize> =
            HashMap::with_capacity(entity_refs.len());
        for (i, token_count) in token_counts_vec.into_iter().enumerate() {
            if i < entity_refs.len() {
                token_counts_map.insert(entity_refs[i].clone(), token_count);
            }
        }

        // Second pass: build EmbeddedEntity from cached data
        for (i, (entry, entity)) in valid_entries.into_iter().enumerate() {
            let embedding_id = embedding_ids[i];
            let entity_ref = &entity_refs[i];

            // Get embedding from batch result
            let (dense_embedding, sparse_embedding) = match embeddings_map.get(&embedding_id) {
                Some((dense, sparse)) => (dense.clone(), sparse.clone()),
                None => {
                    failed_entries.push((
                        entry.outbox_id,
                        format!("Embedding ID {embedding_id} not found in entity_embeddings table"),
                    ));
                    continue;
                }
            };

            // Validate sparse embedding exists
            let sparse_embedding = match sparse_embedding {
                Some(sparse) => sparse,
                None => {
                    failed_entries.push((
                        entry.outbox_id,
                        format!("Sparse embedding not found for embedding ID {embedding_id}"),
                    ));
                    continue;
                }
            };

            // Get token count from batch result
            let bm25_token_count = match token_counts_map.get(entity_ref) {
                Some(&count) => count,
                None => {
                    failed_entries.push((
                        entry.outbox_id,
                        format!(
                            "BM25 token count not found for entity {} in repository {}",
                            entity_ref.1, entity_ref.0
                        ),
                    ));
                    continue;
                }
            };

            // Validate embedding dimensions
            if dense_embedding.len() > self.max_embedding_dim {
                failed_entries.push((
                    entry.outbox_id,
                    format!(
                        "Embedding dimensions {} exceeds maximum allowed size of {}",
                        dense_embedding.len(),
                        self.max_embedding_dim
                    ),
                ));
                continue;
            }

            // Extract qdrant_point_id from payload
            let qdrant_point_id: String = match entry
                .payload
                .get("qdrant_point_id")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
            {
                Some(id) => id,
                None => {
                    failed_entries.push((
                        entry.outbox_id,
                        "Missing or invalid qdrant_point_id in payload".to_string(),
                    ));
                    continue;
                }
            };

            let qdrant_point_id = match codesearch_storage::Uuid::parse_str(&qdrant_point_id) {
                Ok(id) => id,
                Err(e) => {
                    failed_entries.push((entry.outbox_id, format!("Invalid qdrant_point_id: {e}")));
                    continue;
                }
            };

            // Use entity from first pass (already parsed and validated)
            repo_token_counts.push((entry.repository_id, bm25_token_count));
            embedded_entities.push(EmbeddedEntity {
                entity,
                dense_embedding,
                sparse_embedding,
                bm25_token_count,
                qdrant_point_id,
            });
        }

        // Bulk load successful entries to Qdrant
        if !embedded_entities.is_empty() {
            let qdrant_write_start = std::time::Instant::now();
            let entity_count = embedded_entities.len();
            storage_client.bulk_load_entities(embedded_entities).await?;
            info!(
                elapsed_ms = qdrant_write_start.elapsed().as_millis() as u64,
                count = entity_count,
                failed = failed_entries.len(),
                "Qdrant bulk_load_entities completed"
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

            // Batch process INSERT/UPDATE operations
            if !insert_update_entries.is_empty() {
                // Parse all entities first, tracking which ones succeed
                let mut entities_to_create = Vec::new();
                let mut entity_entry_map: Vec<(&OutboxEntry, usize)> = Vec::new();

                for entry in &insert_update_entries {
                    let payload: serde_json::Value = entry.payload.clone();

                    // Parse entity from payload
                    match serde_json::from_value::<codesearch_core::CodeEntity>(
                        payload["entity"].clone(),
                    ) {
                        Ok(entity) => {
                            let idx = entities_to_create.len();
                            entities_to_create.push(entity);
                            entity_entry_map.push((entry, idx));
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
                    let neo4j_create_start = std::time::Instant::now();
                    let entity_count = entities_to_create.len();
                    match neo4j_client.batch_create_nodes(&entities_to_create).await {
                        Ok(neo4j_node_ids) => {
                            // Collect all neo4j_node_id updates for bulk UPDATE
                            let mut node_id_updates: Vec<(Uuid, String, i64)> =
                                Vec::with_capacity(entity_entry_map.len());

                            // Collect node ID updates and mark entries as processed
                            for (entry, idx) in &entity_entry_map {
                                if *idx < neo4j_node_ids.len() {
                                    let neo4j_node_id = neo4j_node_ids[*idx];

                                    // Collect for bulk update
                                    node_id_updates.push((
                                        entry.repository_id,
                                        entry.entity_id.clone(),
                                        neo4j_node_id,
                                    ));

                                    processed_ids.push(entry.outbox_id);
                                }
                            }

                            // Bulk update neo4j_node_id in PostgreSQL (single query instead of N queries)
                            if !node_id_updates.is_empty() {
                                let repo_ids: Vec<Uuid> =
                                    node_id_updates.iter().map(|(r, _, _)| *r).collect();
                                let entity_ids: Vec<&str> =
                                    node_id_updates.iter().map(|(_, e, _)| e.as_str()).collect();
                                let node_ids: Vec<i64> =
                                    node_id_updates.iter().map(|(_, _, n)| *n).collect();

                                if let Err(e) = sqlx::query(
                                    "UPDATE entity_metadata AS em
                                     SET neo4j_node_id = data.node_id
                                     FROM unnest($1::uuid[], $2::text[], $3::bigint[]) AS data(repository_id, entity_id, node_id)
                                     WHERE em.repository_id = data.repository_id AND em.entity_id = data.entity_id",
                                )
                                .bind(&repo_ids)
                                .bind(&entity_ids)
                                .bind(&node_ids)
                                .execute(&mut **tx)
                                .await
                                {
                                    warn!("Failed to bulk update neo4j_node_ids: {}", e);
                                }
                            }
                            info!(
                                elapsed_ms = neo4j_create_start.elapsed().as_millis() as u64,
                                count = entity_count,
                                "Neo4j batch_create_nodes completed"
                            );
                        }
                        Err(e) => {
                            warn!("Failed to batch create Neo4j nodes: {}", e);
                            // Mark all entities in this batch as failed
                            for (entry, _) in entity_entry_map {
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
    /// Resolution uses GenericResolver with typed relationship data from EntityRelationshipData:
    /// - ContainsResolver: parent/child relationships via parent_scope
    /// - calls_resolver: CALLS for function/method calls
    /// - uses_resolver: USES for type references
    /// - implements_resolver: IMPLEMENTS for trait implementations
    /// - associates_resolver: ASSOCIATES for impl blocks
    /// - extends_resolver: EXTENDS_INTERFACE for extended types (Rust trait bounds, TS interface extends)
    /// - inherits_resolver: INHERITS_FROM for class inheritance
    /// - imports_resolver: IMPORTS for module imports
    /// - reexports_resolver: REEXPORTS for module re-exports (barrel exports)
    ///
    /// Called once when the outbox drains (index mode completes).
    pub async fn resolve_pending_relationships(&self) -> Result<()> {
        use crate::generic_resolver::{
            associates_resolver, calls_resolver, extends_resolver, implements_resolver,
            imports_resolver, inherits_resolver, reexports_resolver, uses_resolver,
        };
        use crate::neo4j_relationship_resolver::{
            resolve_relationships_generic, ContainsResolver, EntityCache,
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

        // Create generic resolvers using typed EntityRelationshipData
        let calls = calls_resolver();
        let uses = uses_resolver();
        let implements = implements_resolver();
        let associates = associates_resolver();
        let extends = extends_resolver();
        let inherits = inherits_resolver();
        let imports = imports_resolver();
        let reexports = reexports_resolver();

        // Define all resolvers to run (ContainsResolver uses parent_scope, not relationships field)
        let resolvers: Vec<&dyn crate::neo4j_relationship_resolver::RelationshipResolver> = vec![
            &ContainsResolver,
            &calls,
            &uses,
            &implements,
            &associates,
            &extends,
            &inherits,
            &imports,
            &reexports,
        ];

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
                    continue;
                }
            };

            neo4j_client.use_database(&db_name).await?;

            // Create entity cache once for all resolvers (eliminates duplicate DB queries)
            let cache = match EntityCache::new(&self.postgres_client, repository_id).await {
                Ok(c) => c,
                Err(e) => {
                    warn!(
                        "Failed to create entity cache for repository {}: {}",
                        repository_id, e
                    );
                    // Clear the pending flag to prevent infinite retry loops
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
                    continue;
                }
            };

            let entity_count = cache.all().len();
            info!(
                "Loaded {} entities into cache for relationship resolution",
                entity_count
            );

            // Run all resolvers using cached entity data, tracking failures
            let mut failed_resolvers: Vec<&str> = Vec::new();
            for resolver in &resolvers {
                if let Err(e) =
                    resolve_relationships_generic(&cache, neo4j_client.as_ref(), *resolver).await
                {
                    warn!(
                        "Failed to resolve {} relationships for repository {}: {}",
                        resolver.name(),
                        repository_id,
                        e
                    );
                    failed_resolvers.push(resolver.name());
                    // Continue with other resolvers even if one fails
                }
            }

            // Resolve external references (creates External stub nodes)
            let mut external_resolution_failed = false;
            if let Err(e) = crate::neo4j_relationship_resolver::resolve_external_references(
                &cache,
                neo4j_client.as_ref(),
                repository_id,
            )
            .await
            {
                warn!(
                    "Failed to resolve external references for repository {}: {}",
                    repository_id, e
                );
                external_resolution_failed = true;
                // Continue even if external resolution fails
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

            // Log completion with summary
            if failed_resolvers.is_empty() && !external_resolution_failed {
                info!(
                    "Completed relationship resolution for repository {} ({} entities)",
                    repository_id, entity_count
                );
            } else {
                let failure_summary = if !failed_resolvers.is_empty() {
                    format!(
                        "{} resolver(s) failed: {}",
                        failed_resolvers.len(),
                        failed_resolvers.join(", ")
                    )
                } else {
                    String::new()
                };
                let external_summary = if external_resolution_failed {
                    "external resolution failed"
                } else {
                    ""
                };
                let separator = if !failure_summary.is_empty() && external_resolution_failed {
                    "; "
                } else {
                    ""
                };
                warn!(
                    "Completed relationship resolution for repository {} with warnings: {}{}{}",
                    repository_id, failure_summary, separator, external_summary
                );
            }
        }

        Ok(())
    }
}

/*
 * Tests have been moved to tests/integration_tests.rs
 * They now use testcontainers for real PostgreSQL and Qdrant instances
 * to properly test SQL-based transactions (SELECT FOR UPDATE, etc.)
 */
