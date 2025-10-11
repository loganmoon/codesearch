use async_trait::async_trait;
use codesearch_core::entities::CodeEntity;
use codesearch_core::error::{Error, Result};
use sqlx::{PgPool, Postgres, QueryBuilder, Row};
use std::str::FromStr;
use uuid::Uuid;

/// Operation type for outbox pattern
///
/// Represents the type of operation to be performed on the target data store.
/// Used in the transactional outbox pattern to ensure eventual consistency
/// between PostgreSQL metadata and external stores like Qdrant.
#[derive(Debug, Clone, Copy)]
pub enum OutboxOperation {
    /// Insert a new entity into the target store
    Insert,
    /// Update an existing entity in the target store
    Update,
    /// Delete an entity from the target store
    Delete,
}

impl std::fmt::Display for OutboxOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Insert => write!(f, "INSERT"),
            Self::Update => write!(f, "UPDATE"),
            Self::Delete => write!(f, "DELETE"),
        }
    }
}

impl FromStr for OutboxOperation {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "INSERT" => Ok(Self::Insert),
            "UPDATE" => Ok(Self::Update),
            "DELETE" => Ok(Self::Delete),
            _ => Err(Error::storage(format!("Invalid operation: {s}"))),
        }
    }
}

/// Target data store for outbox pattern
///
/// Identifies which external data store should process the outbox entry.
/// Each target store has its own processing queue to enable parallel processing
/// and independent scaling of different storage backends.
#[derive(Debug, Clone, Copy)]
pub enum TargetStore {
    /// Qdrant vector database for semantic search
    Qdrant,
    /// Neo4j graph database for relationship queries
    Neo4j,
}

impl std::fmt::Display for TargetStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Qdrant => write!(f, "qdrant"),
            Self::Neo4j => write!(f, "neo4j"),
        }
    }
}

impl FromStr for TargetStore {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "qdrant" => Ok(Self::Qdrant),
            "neo4j" => Ok(Self::Neo4j),
            _ => Err(Error::storage(format!("Invalid target store: {s}"))),
        }
    }
}

/// Outbox entry for reliable event publishing
///
/// Represents a pending operation that needs to be applied to an external data store.
/// The outbox pattern ensures that database changes and external store updates happen
/// atomically by writing both to PostgreSQL in a transaction, then processing outbox
/// entries asynchronously to update external stores.
///
/// # Fields
///
/// * `outbox_id` - Unique identifier for this outbox entry
/// * `repository_id` - Repository this operation applies to
/// * `entity_id` - Identifier of the entity to be modified
/// * `operation` - Operation type (INSERT, UPDATE, DELETE)
/// * `target_store` - Which external store should process this (qdrant, neo4j)
/// * `payload` - JSON payload containing the data needed to perform the operation
/// * `created_at` - When this entry was created
/// * `processed_at` - When this entry was successfully processed (None if pending)
/// * `retry_count` - Number of times processing has been attempted
/// * `last_error` - Error message from the most recent failed processing attempt
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct OutboxEntry {
    pub outbox_id: Uuid,
    pub repository_id: Uuid,
    pub entity_id: String,
    pub operation: String,
    pub target_store: String,
    pub payload: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub processed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub retry_count: i32,
    pub last_error: Option<String>,
    pub collection_name: String,
}

/// Type alias for a single entity batch entry with outbox data
pub type EntityOutboxBatchEntry<'a> = (
    &'a CodeEntity,
    &'a [f32],
    OutboxOperation,
    Uuid,
    TargetStore,
    Option<String>,
);

pub struct PostgresClient {
    pool: PgPool,
    max_entity_batch_size: usize,
}

impl PostgresClient {
    pub fn new(pool: PgPool, max_entity_batch_size: usize) -> Self {
        Self {
            pool,
            max_entity_batch_size,
        }
    }

    /// Get direct access to the connection pool for custom queries
    ///
    /// This is used by the outbox processor for bulk operations that
    /// don't fit the standard trait methods.
    pub fn get_pool(&self) -> &PgPool {
        &self.pool
    }

    /// Run database migrations
    pub async fn run_migrations(&self) -> Result<()> {
        sqlx::migrate!("../../migrations")
            .run(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to run migrations: {e}")))?;
        Ok(())
    }

    /// Ensure repository exists, return repository_id
    pub async fn ensure_repository(
        &self,
        repository_path: &std::path::Path,
        collection_name: &str,
        repository_name: Option<&str>,
    ) -> Result<Uuid> {
        let repo_path_str = repository_path
            .to_str()
            .ok_or_else(|| Error::storage("Invalid repository path"))?;

        // Try to find existing repository
        let existing: Option<(Uuid,)> =
            sqlx::query_as("SELECT repository_id FROM repositories WHERE collection_name = $1")
                .bind(collection_name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to query repository: {e}")))?;

        if let Some((repository_id,)) = existing {
            return Ok(repository_id);
        }

        // Create new repository
        let repo_name = repository_name
            .or_else(|| repository_path.file_name()?.to_str())
            .unwrap_or("unknown");

        let (repository_id,): (Uuid,) = sqlx::query_as(
            "INSERT INTO repositories (repository_path, repository_name, collection_name, created_at, updated_at)
             VALUES ($1, $2, $3, NOW(), NOW())
             RETURNING repository_id",
        )
        .bind(repo_path_str)
        .bind(repo_name)
        .bind(collection_name)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to insert repository: {e}")))?;

        Ok(repository_id)
    }

    /// Get repository by collection name
    pub async fn get_repository_id(&self, collection_name: &str) -> Result<Option<Uuid>> {
        let record: Option<(Uuid,)> =
            sqlx::query_as("SELECT repository_id FROM repositories WHERE collection_name = $1")
                .bind(collection_name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to query repository: {e}")))?;

        Ok(record.map(|(id,)| id))
    }

    /// Get collection name by repository ID
    pub async fn get_collection_name(&self, repository_id: Uuid) -> Result<Option<String>> {
        let record: Option<(String,)> =
            sqlx::query_as("SELECT collection_name FROM repositories WHERE repository_id = $1")
                .bind(repository_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to query collection name: {e}")))?;

        Ok(record.map(|(name,)| name))
    }

    /// Get entity metadata (qdrant_point_id and deleted_at) by entity_id
    pub async fn get_entity_metadata(
        &self,
        repository_id: Uuid,
        entity_id: &str,
    ) -> Result<Option<(Uuid, Option<chrono::DateTime<chrono::Utc>>)>> {
        let record: Option<(Uuid, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
            "SELECT qdrant_point_id, deleted_at FROM entity_metadata
             WHERE repository_id = $1 AND entity_id = $2",
        )
        .bind(repository_id)
        .bind(entity_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get entity metadata: {e}")))?;

        Ok(record)
    }

    /// Batch fetch entity metadata for multiple entities
    pub async fn get_entities_metadata_batch(
        &self,
        repository_id: Uuid,
        entity_ids: &[String],
    ) -> Result<std::collections::HashMap<String, (Uuid, Option<chrono::DateTime<chrono::Utc>>)>>
    {
        if entity_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        // Validate batch size to prevent resource exhaustion
        if entity_ids.len() > self.max_entity_batch_size {
            return Err(Error::storage(format!(
                "Batch size {} exceeds maximum allowed size of {}",
                entity_ids.len(),
                self.max_entity_batch_size
            )));
        }

        // Build query using QueryBuilder for type safety
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT entity_id, qdrant_point_id, deleted_at FROM entity_metadata WHERE repository_id = "
        );

        query_builder.push_bind(repository_id);
        query_builder.push(" AND entity_id IN (");

        let mut separated = query_builder.separated(", ");
        for entity_id in entity_ids {
            separated.push_bind(entity_id);
        }
        separated.push_unseparated(")");

        let rows = query_builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to fetch entity metadata batch: {e}")))?;

        let mut result = std::collections::HashMap::new();
        for row in rows {
            let entity_id: String = row
                .try_get("entity_id")
                .map_err(|e| Error::storage(format!("Failed to extract entity_id: {e}")))?;
            let point_id: Uuid = row
                .try_get("qdrant_point_id")
                .map_err(|e| Error::storage(format!("Failed to extract qdrant_point_id: {e}")))?;
            let deleted_at: Option<chrono::DateTime<chrono::Utc>> = row
                .try_get("deleted_at")
                .map_err(|e| Error::storage(format!("Failed to extract deleted_at: {e}")))?;

            result.insert(entity_id, (point_id, deleted_at));
        }

        Ok(result)
    }

    /// Get file snapshot (list of entity IDs in file)
    pub async fn get_file_snapshot(
        &self,
        repository_id: Uuid,
        file_path: &str,
    ) -> Result<Option<Vec<String>>> {
        let record: Option<(Vec<String>,)> = sqlx::query_as(
            "SELECT entity_ids FROM file_entity_snapshots
             WHERE repository_id = $1 AND file_path = $2",
        )
        .bind(repository_id)
        .bind(file_path)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get file snapshot: {e}")))?;

        Ok(record.map(|(ids,)| ids))
    }

    /// Update file snapshot with current entity IDs (transactional)
    pub async fn update_file_snapshot(
        &self,
        repository_id: Uuid,
        file_path: &str,
        entity_ids: Vec<String>,
        git_commit_hash: Option<String>,
    ) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        sqlx::query(
            "INSERT INTO file_entity_snapshots (repository_id, file_path, entity_ids, git_commit_hash, indexed_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (repository_id, file_path)
             DO UPDATE SET
                entity_ids = EXCLUDED.entity_ids,
                git_commit_hash = EXCLUDED.git_commit_hash,
                indexed_at = NOW()",
        )
        .bind(repository_id)
        .bind(file_path)
        .bind(&entity_ids)
        .bind(git_commit_hash)
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to update file snapshot: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        Ok(())
    }

    /// Batch fetch entities by (repository_id, entity_id) pairs
    pub async fn get_entities_by_ids(
        &self,
        entity_refs: &[(Uuid, String)],
    ) -> Result<Vec<CodeEntity>> {
        if entity_refs.is_empty() {
            return Ok(Vec::new());
        }

        // Validate batch size to prevent resource exhaustion
        if entity_refs.len() > self.max_entity_batch_size {
            return Err(Error::storage(format!(
                "Batch size {} exceeds maximum allowed size of {}",
                entity_refs.len(),
                self.max_entity_batch_size
            )));
        }

        // Build query using QueryBuilder for type safety
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT entity_data FROM entity_metadata WHERE deleted_at IS NULL AND (repository_id, entity_id) IN "
        );

        query_builder.push_tuples(entity_refs, |mut b, (repo_id, entity_id)| {
            b.push_bind(repo_id).push_bind(entity_id);
        });

        let rows = query_builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to fetch entities: {e}")))?;

        let mut entities = Vec::new();
        for row in rows {
            let entity_json: serde_json::Value = row
                .try_get("entity_data")
                .map_err(|e| Error::storage(format!("Failed to extract entity_data: {e}")))?;
            let entity: CodeEntity = serde_json::from_value(entity_json)
                .map_err(|e| Error::storage(format!("Failed to deserialize entity: {e}")))?;
            entities.push(entity);
        }

        Ok(entities)
    }

    /// Mark entities as deleted (soft delete) (transactional)
    pub async fn mark_entities_deleted(
        &self,
        repository_id: Uuid,
        entity_ids: &[String],
    ) -> Result<()> {
        if entity_ids.is_empty() {
            return Ok(());
        }

        // Validate batch size to prevent resource exhaustion
        if entity_ids.len() > self.max_entity_batch_size {
            return Err(Error::storage(format!(
                "Batch size {} exceeds maximum allowed size of {}",
                entity_ids.len(),
                self.max_entity_batch_size
            )));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        // Build query using QueryBuilder for type safety
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "UPDATE entity_metadata SET deleted_at = NOW(), updated_at = NOW() WHERE repository_id = "
        );

        query_builder.push_bind(repository_id);
        query_builder.push(" AND entity_id IN (");

        let mut separated = query_builder.separated(", ");
        for entity_id in entity_ids {
            separated.push_bind(entity_id);
        }
        separated.push_unseparated(")");

        let result = query_builder
            .build()
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to mark entities as deleted: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        tracing::info!("Marked {} entities as deleted", result.rows_affected());

        Ok(())
    }

    /// Mark entities as deleted and create outbox entries in a single transaction
    pub async fn mark_entities_deleted_with_outbox(
        &self,
        repository_id: Uuid,
        collection_name: &str,
        entity_ids: &[String],
    ) -> Result<()> {
        if entity_ids.is_empty() {
            return Ok(());
        }

        // Validate batch size to prevent resource exhaustion
        if entity_ids.len() > self.max_entity_batch_size {
            return Err(Error::storage(format!(
                "Batch size {} exceeds maximum allowed size of {}",
                entity_ids.len(),
                self.max_entity_batch_size
            )));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        // 1. Mark entities as deleted
        let mut update_query: QueryBuilder<Postgres> = QueryBuilder::new(
            "UPDATE entity_metadata SET deleted_at = NOW(), updated_at = NOW() WHERE repository_id = "
        );

        update_query.push_bind(repository_id);
        update_query.push(" AND entity_id IN (");

        let mut separated = update_query.separated(", ");
        for entity_id in entity_ids {
            separated.push_bind(entity_id);
        }
        separated.push_unseparated(")");

        let update_result = update_query
            .build()
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to mark entities as deleted: {e}")))?;

        // 2. Create outbox entries for all deletes
        let mut outbox_query: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store, payload, collection_name, created_at) "
        );

        outbox_query.push_values(entity_ids, |mut b, entity_id| {
            let payload = serde_json::json!({
                "entity_ids": [entity_id],
                "reason": "file_change"
            });
            b.push_bind(repository_id)
                .push_bind(entity_id)
                .push_bind(OutboxOperation::Delete.to_string())
                .push_bind(TargetStore::Qdrant.to_string())
                .push_bind(payload)
                .push_bind(collection_name)
                .push("NOW()");
        });

        outbox_query
            .build()
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to write outbox entries: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        tracing::info!(
            "Marked {} entities as deleted with outbox entries",
            update_result.rows_affected()
        );

        Ok(())
    }

    /// Store entities with outbox entries in a single transaction (batch operation)
    pub async fn store_entities_with_outbox_batch(
        &self,
        repository_id: Uuid,
        collection_name: &str,
        entities: &[EntityOutboxBatchEntry<'_>],
    ) -> Result<Vec<Uuid>> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }

        // Validate batch size
        if entities.len() > self.max_entity_batch_size {
            return Err(Error::storage(format!(
                "Batch size {} exceeds maximum allowed size of {}",
                entities.len(),
                self.max_entity_batch_size
            )));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        // Pre-validate and convert entities to avoid unwrap in closure
        let validated_entities: Result<Vec<_>> = entities
            .iter()
            .map(|(entity, embedding, op, point_id, target, git_commit)| {
                let entity_json = serde_json::to_value(entity)
                    .map_err(|e| Error::storage(format!("Failed to serialize entity: {e}")))?;

                let file_path_str = entity
                    .file_path
                    .to_str()
                    .ok_or_else(|| Error::storage("Invalid file path"))?;

                Ok((
                    entity,
                    embedding,
                    op,
                    point_id,
                    target,
                    git_commit,
                    entity_json,
                    file_path_str,
                ))
            })
            .collect();

        let validated_entities = validated_entities?;

        // Build bulk insert for entity_metadata
        let mut entity_query: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO entity_metadata (
                entity_id, repository_id, qualified_name, name, parent_scope,
                entity_type, language, file_path, visibility,
                entity_data, git_commit_hash, qdrant_point_id
            ) ",
        );

        entity_query.push_values(
            &validated_entities,
            |mut b,
             (
                entity,
                _embedding,
                _op,
                point_id,
                _target,
                git_commit,
                entity_json,
                file_path_str,
            )| {
                b.push_bind(&entity.entity_id)
                    .push_bind(repository_id)
                    .push_bind(&entity.qualified_name)
                    .push_bind(&entity.name)
                    .push_bind(&entity.parent_scope)
                    .push_bind(entity.entity_type.to_string())
                    .push_bind(entity.language.to_string())
                    .push_bind(*file_path_str)
                    .push_bind(entity.visibility.to_string())
                    .push_bind(entity_json)
                    .push_bind(git_commit)
                    .push_bind(point_id);
            },
        );

        entity_query.push(
            " ON CONFLICT (repository_id, entity_id)
            DO UPDATE SET
                qualified_name = EXCLUDED.qualified_name,
                name = EXCLUDED.name,
                parent_scope = EXCLUDED.parent_scope,
                entity_type = EXCLUDED.entity_type,
                language = EXCLUDED.language,
                file_path = EXCLUDED.file_path,
                visibility = EXCLUDED.visibility,
                entity_data = EXCLUDED.entity_data,
                git_commit_hash = EXCLUDED.git_commit_hash,
                qdrant_point_id = EXCLUDED.qdrant_point_id,
                updated_at = NOW(),
                deleted_at = NULL",
        );

        entity_query
            .build()
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                // Extract entity IDs for debugging duplicate key errors
                let entity_ids: Vec<String> = validated_entities
                    .iter()
                    .map(|(entity, _, _, _, _, _, _, _)| entity.entity_id.clone())
                    .collect();
                let unique_ids: std::collections::HashSet<_> = entity_ids.iter().collect();

                if entity_ids.len() != unique_ids.len() {
                    Error::storage(format!(
                        "Failed to bulk insert entity metadata (detected {} duplicate entity_ids in batch of {}): {e}",
                        entity_ids.len() - unique_ids.len(),
                        entity_ids.len()
                    ))
                } else {
                    Error::storage(format!(
                        "Failed to bulk insert entity metadata (batch size {}): {e}",
                        entity_ids.len()
                    ))
                }
            })?;

        // Build bulk insert for entity_outbox
        let mut outbox_query: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO entity_outbox (
                repository_id, entity_id, operation, target_store, payload, collection_name
            ) ",
        );

        outbox_query.push_values(
            &validated_entities,
            |mut b,
             (
                entity,
                embedding,
                op,
                point_id,
                target,
                _git_commit,
                entity_json,
                _file_path_str,
            )| {
                let payload = serde_json::json!({
                    "entity": entity_json,
                    "embedding": embedding,
                    "qdrant_point_id": point_id.to_string()
                });

                b.push_bind(repository_id)
                    .push_bind(&entity.entity_id)
                    .push_bind(op.to_string())
                    .push_bind(target.to_string())
                    .push_bind(payload)
                    .push_bind(collection_name);
            },
        );

        outbox_query.push(" RETURNING outbox_id");

        let outbox_ids: Vec<Uuid> = outbox_query
            .build_query_scalar()
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to bulk insert outbox entries: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        Ok(outbox_ids)
    }

    /// Get unprocessed outbox entries for a target store
    pub async fn get_unprocessed_outbox_entries(
        &self,
        target_store: TargetStore,
        limit: i64,
    ) -> Result<Vec<OutboxEntry>> {
        let entries = sqlx::query_as::<_, OutboxEntry>(
            "SELECT outbox_id, repository_id, entity_id, operation, target_store, payload,
                    created_at, processed_at, retry_count, last_error, collection_name
             FROM entity_outbox
             WHERE target_store = $1 AND processed_at IS NULL
             ORDER BY created_at ASC
             LIMIT $2",
        )
        .bind(target_store.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get outbox entries: {e}")))?;

        Ok(entries)
    }

    /// Mark outbox entry as processed (transactional)
    pub async fn mark_outbox_processed(&self, outbox_id: Uuid) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        sqlx::query("UPDATE entity_outbox SET processed_at = NOW() WHERE outbox_id = $1")
            .bind(outbox_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to mark outbox processed: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        Ok(())
    }

    /// Increment retry count and record error (transactional)
    pub async fn record_outbox_failure(&self, outbox_id: Uuid, error: &str) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        sqlx::query(
            "UPDATE entity_outbox
             SET retry_count = retry_count + 1, last_error = $2
             WHERE outbox_id = $1",
        )
        .bind(outbox_id)
        .bind(error)
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to record outbox failure: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        Ok(())
    }

    /// Get the last indexed commit for a repository
    ///
    /// Retrieves the commit hash of the most recently indexed commit for the specified repository.
    /// This is used for incremental indexing to determine which commits need to be processed.
    ///
    /// # Parameters
    ///
    /// * `repository_id` - The UUID of the repository to query
    ///
    /// # Returns
    ///
    /// * `Ok(Some(String))` - The commit hash if a commit has been indexed
    /// * `Ok(None)` - If no commits have been indexed yet
    /// * `Err(_)` - If a database error occurred
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// * The database connection fails
    /// * The repository_id is invalid or not found
    /// * A database query error occurs
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use uuid::Uuid;
    /// # use codesearch_storage::PostgresClientTrait;
    /// # async fn example(client: &dyn PostgresClientTrait, repo_id: Uuid) -> codesearch_core::error::Result<()> {
    /// let last_commit = client.get_last_indexed_commit(repo_id).await?;
    /// if let Some(commit_hash) = last_commit {
    ///     println!("Last indexed commit: {commit_hash}");
    /// } else {
    ///     println!("Repository has not been indexed yet");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_last_indexed_commit(&self, repository_id: Uuid) -> Result<Option<String>> {
        let record: Option<(Option<String>,)> =
            sqlx::query_as("SELECT last_indexed_commit FROM repositories WHERE repository_id = $1")
                .bind(repository_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| {
                    Error::storage(format!(
                        "Failed to get last indexed commit for repository {repository_id}: {e}"
                    ))
                })?;

        Ok(record.and_then(|(commit,)| commit))
    }

    /// Set the last indexed commit for a repository
    pub async fn set_last_indexed_commit(
        &self,
        repository_id: Uuid,
        commit_hash: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE repositories SET last_indexed_commit = $2, updated_at = NOW() WHERE repository_id = $1",
        )
        .bind(repository_id)
        .bind(commit_hash)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            Error::storage(format!(
                "Failed to set last indexed commit for repository {repository_id}: {e}"
            ))
        })?;

        Ok(())
    }

    /// Drop all data from all tables
    pub async fn drop_all_data(&self) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        // Truncate all tables with CASCADE to handle foreign key constraints
        // Order matters - truncate child tables first
        sqlx::query("TRUNCATE TABLE entity_outbox CASCADE")
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to truncate entity_outbox: {e}")))?;

        sqlx::query("TRUNCATE TABLE file_entity_snapshots CASCADE")
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                Error::storage(format!("Failed to truncate file_entity_snapshots: {e}"))
            })?;

        sqlx::query("TRUNCATE TABLE entity_metadata CASCADE")
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to truncate entity_metadata: {e}")))?;

        sqlx::query("TRUNCATE TABLE repositories CASCADE")
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to truncate repositories: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        tracing::info!("Dropped all data from PostgreSQL tables");

        Ok(())
    }
}

// Trait implementation delegates to inherent methods for testability and flexibility
#[async_trait]
impl super::PostgresClientTrait for PostgresClient {
    fn max_entity_batch_size(&self) -> usize {
        self.max_entity_batch_size
    }

    fn get_pool(&self) -> &PgPool {
        self.get_pool()
    }

    async fn run_migrations(&self) -> Result<()> {
        self.run_migrations().await
    }

    async fn ensure_repository(
        &self,
        repository_path: &std::path::Path,
        collection_name: &str,
        repository_name: Option<&str>,
    ) -> Result<Uuid> {
        self.ensure_repository(repository_path, collection_name, repository_name)
            .await
    }

    async fn get_repository_id(&self, collection_name: &str) -> Result<Option<Uuid>> {
        self.get_repository_id(collection_name).await
    }

    async fn get_collection_name(&self, repository_id: Uuid) -> Result<Option<String>> {
        self.get_collection_name(repository_id).await
    }

    async fn get_entity_metadata(
        &self,
        repository_id: Uuid,
        entity_id: &str,
    ) -> Result<Option<(Uuid, Option<chrono::DateTime<chrono::Utc>>)>> {
        self.get_entity_metadata(repository_id, entity_id).await
    }

    async fn get_entities_metadata_batch(
        &self,
        repository_id: Uuid,
        entity_ids: &[String],
    ) -> Result<std::collections::HashMap<String, (Uuid, Option<chrono::DateTime<chrono::Utc>>)>>
    {
        self.get_entities_metadata_batch(repository_id, entity_ids)
            .await
    }

    async fn get_file_snapshot(
        &self,
        repository_id: Uuid,
        file_path: &str,
    ) -> Result<Option<Vec<String>>> {
        self.get_file_snapshot(repository_id, file_path).await
    }

    async fn update_file_snapshot(
        &self,
        repository_id: Uuid,
        file_path: &str,
        entity_ids: Vec<String>,
        git_commit_hash: Option<String>,
    ) -> Result<()> {
        self.update_file_snapshot(repository_id, file_path, entity_ids, git_commit_hash)
            .await
    }

    async fn get_entities_by_ids(&self, entity_refs: &[(Uuid, String)]) -> Result<Vec<CodeEntity>> {
        self.get_entities_by_ids(entity_refs).await
    }

    async fn mark_entities_deleted(
        &self,
        repository_id: Uuid,
        entity_ids: &[String],
    ) -> Result<()> {
        self.mark_entities_deleted(repository_id, entity_ids).await
    }

    async fn mark_entities_deleted_with_outbox(
        &self,
        repository_id: Uuid,
        collection_name: &str,
        entity_ids: &[String],
    ) -> Result<()> {
        self.mark_entities_deleted_with_outbox(repository_id, collection_name, entity_ids)
            .await
    }

    async fn store_entities_with_outbox_batch(
        &self,
        repository_id: Uuid,
        collection_name: &str,
        entities: &[EntityOutboxBatchEntry<'_>],
    ) -> Result<Vec<Uuid>> {
        self.store_entities_with_outbox_batch(repository_id, collection_name, entities)
            .await
    }

    async fn get_unprocessed_outbox_entries(
        &self,
        target_store: TargetStore,
        limit: i64,
    ) -> Result<Vec<OutboxEntry>> {
        self.get_unprocessed_outbox_entries(target_store, limit)
            .await
    }

    async fn mark_outbox_processed(&self, outbox_id: Uuid) -> Result<()> {
        self.mark_outbox_processed(outbox_id).await
    }

    async fn record_outbox_failure(&self, outbox_id: Uuid, error: &str) -> Result<()> {
        self.record_outbox_failure(outbox_id, error).await
    }

    async fn get_last_indexed_commit(&self, repository_id: Uuid) -> Result<Option<String>> {
        self.get_last_indexed_commit(repository_id).await
    }

    async fn set_last_indexed_commit(&self, repository_id: Uuid, commit_hash: &str) -> Result<()> {
        self.set_last_indexed_commit(repository_id, commit_hash)
            .await
    }

    async fn drop_all_data(&self) -> Result<()> {
        self.drop_all_data().await
    }
}
