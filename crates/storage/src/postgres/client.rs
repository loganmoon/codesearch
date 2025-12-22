use async_trait::async_trait;
use codesearch_core::entities::{CodeEntity, EntityType};
use codesearch_core::error::{Error, Result};
use serde::Serialize;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};
use std::str::FromStr;
use uuid::Uuid;

// Import Neo4j relationship builders (internal to crate)
use crate::neo4j::relationship_builder::{
    build_calls_relationship_json, build_contains_relationship_json,
    build_imports_relationship_json, build_inherits_from_relationship_json,
    build_trait_relationship_json, build_uses_relationship_json,
};

/// Neo4j node properties for outbox payload
#[derive(Debug, Clone, Serialize)]
struct Neo4jNodeProperties {
    id: String,
    repository_id: String,
    qualified_name: String,
    name: String,
    language: String,
    visibility: String,
    is_async: bool,
    is_generic: bool,
    is_static: bool,
    is_abstract: bool,
    is_const: bool,
}

/// Complete Neo4j outbox payload
#[derive(Debug, Serialize)]
struct Neo4jOutboxPayload<'a> {
    entity: &'a CodeEntity,
    node: Neo4jNodeProperties,
    labels: Vec<&'static str>,
    relationships: Vec<serde_json::Value>,
}

/// Maximum sparse embedding size to prevent memory exhaustion attacks
const MAX_SPARSE_EMBEDDING_SIZE: usize = 100_000;

/// Convert sparse embedding to separate indices and values arrays for PostgreSQL storage
/// PostgreSQL BIGINT[] can safely store all u32 values (0 to 4,294,967,295)
/// Returns an error if the sparse embedding exceeds MAX_SPARSE_EMBEDDING_SIZE
fn sparse_embedding_to_arrays(sparse: &[(u32, f32)]) -> Result<(Vec<i64>, Vec<f32>)> {
    if sparse.len() > MAX_SPARSE_EMBEDDING_SIZE {
        return Err(Error::storage(format!(
            "Sparse embedding size {} exceeds maximum allowed size {MAX_SPARSE_EMBEDDING_SIZE}",
            sparse.len()
        )));
    }

    let (indices, values): (Vec<u32>, Vec<f32>) = sparse.iter().copied().unzip();
    let indices_i64: Vec<i64> = indices.into_iter().map(|idx| idx as i64).collect();
    Ok((indices_i64, values))
}

/// Convert separate indices and values arrays from PostgreSQL back to sparse embedding
/// Returns an error if the arrays have mismatched lengths or exceed MAX_SPARSE_EMBEDDING_SIZE
fn arrays_to_sparse_embedding(indices: Vec<i64>, values: Vec<f32>) -> Result<Vec<(u32, f32)>> {
    if indices.len() != values.len() {
        return Err(Error::storage(format!(
            "Sparse embedding indices length {} does not match values length {}",
            indices.len(),
            values.len()
        )));
    }

    if indices.len() > MAX_SPARSE_EMBEDDING_SIZE {
        return Err(Error::storage(format!(
            "Sparse embedding size {} exceeds maximum allowed size {MAX_SPARSE_EMBEDDING_SIZE}",
            indices.len()
        )));
    }

    Ok(indices
        .into_iter()
        .zip(values)
        .map(|(idx, val)| (idx as u32, val))
        .collect())
}

/// Type alias for embedding cache entry: (content_hash, dense_embedding, sparse_embedding)
pub type EmbeddingCacheEntry = (String, Vec<f32>, Option<Vec<(u32, f32)>>);

/// Type alias for sparse embedding database row: (dense, sparse_indices, sparse_values)
type SparseEmbeddingRow = (Vec<f32>, Option<Vec<i64>>, Option<Vec<f32>>);

/// Type alias for validated sparse embedding arrays: (sparse_indices, sparse_values)
type ValidatedSparseArrays = (Option<Vec<i64>>, Option<Vec<f32>>);

/// Type alias for batch embedding row: (id, dense_embedding, sparse_indices, sparse_values)
type BatchEmbeddingRow = (i64, Vec<f32>, Option<Vec<i64>>, Option<Vec<f32>>);

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
/// * `collection_name` - Target collection name in the external store (e.g., Qdrant collection)
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
    pub embedding_id: Option<i64>,
}

/// Type alias for a single entity batch entry with outbox data
pub type EntityOutboxBatchEntry<'a> = (
    &'a CodeEntity,
    i64, // embedding_id (now includes both dense and sparse in entity_embeddings table)
    OutboxOperation,
    Uuid, // qdrant_point_id
    TargetStore,
    Option<String>, // git_commit_hash
    usize,          // bm25_token_count
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
    ///
    /// Computes a deterministic repository_id from the repository_path using
    /// `StorageConfig::generate_repository_id()`.
    pub async fn ensure_repository(
        &self,
        repository_path: &std::path::Path,
        collection_name: &str,
        repository_name: Option<&str>,
    ) -> Result<Uuid> {
        // Generate deterministic repository ID from path
        let repository_id =
            codesearch_core::config::StorageConfig::generate_repository_id(repository_path)?;
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

        if let Some((existing_id,)) = existing {
            tracing::debug!(
                repository_id = %existing_id,
                collection_name = %collection_name,
                "Found existing repository"
            );
            return Ok(existing_id);
        }

        // Create new repository with the provided deterministic UUID
        let repo_name = repository_name
            .or_else(|| repository_path.file_name()?.to_str())
            .unwrap_or("unknown");

        sqlx::query(
            "INSERT INTO repositories (repository_id, repository_path, repository_name, collection_name, bm25_avgdl, bm25_total_tokens, bm25_entity_count, created_at, updated_at)
             VALUES ($1, $2, $3, $4, 50.0, 0, 0, NOW(), NOW())",
        )
        .bind(repository_id)
        .bind(repo_path_str)
        .bind(repo_name)
        .bind(collection_name)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to insert repository: {e}")))?;

        tracing::debug!(
            repository_id = %repository_id,
            collection_name = %collection_name,
            repository_path = %repository_path.display(),
            "Created new repository with deterministic UUID"
        );

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

    /// Get repository information by collection name
    pub async fn get_repository_by_collection(
        &self,
        collection_name: &str,
    ) -> Result<Option<(Uuid, std::path::PathBuf, String)>> {
        let record: Option<(Uuid, String, String)> = sqlx::query_as(
            "SELECT repository_id, repository_path, repository_name FROM repositories WHERE collection_name = $1"
        )
        .bind(collection_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to query repository by collection: {e}")))?;

        Ok(record.map(|(id, path, name)| (id, std::path::PathBuf::from(path), name)))
    }

    /// Get repository information by filesystem path
    pub async fn get_repository_by_path(
        &self,
        repository_path: &std::path::Path,
    ) -> Result<Option<(Uuid, String)>> {
        let repo_path_str = repository_path
            .to_str()
            .ok_or_else(|| Error::storage("Invalid repository path"))?;

        let record: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT repository_id, collection_name FROM repositories WHERE repository_path = $1",
        )
        .bind(repo_path_str)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to query repository by path: {e}")))?;

        Ok(record)
    }

    /// List all repositories in the database
    pub async fn list_all_repositories(&self) -> Result<Vec<(Uuid, String, std::path::PathBuf)>> {
        let rows = sqlx::query_as::<_, (Uuid, String, String)>(
            "SELECT repository_id, collection_name, repository_path FROM repositories ORDER BY created_at DESC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to list repositories: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|(id, name, path)| (id, name, std::path::PathBuf::from(path)))
            .collect())
    }

    /// Delete a single repository and all its associated data
    ///
    /// Relies on ON DELETE CASCADE constraints to automatically remove:
    /// - entity_metadata
    /// - file_entity_snapshots
    /// - entity_outbox
    /// - entity_embeddings
    pub async fn drop_repository(&self, repository_id: Uuid) -> Result<()> {
        let result = sqlx::query("DELETE FROM repositories WHERE repository_id = $1")
            .bind(repository_id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to delete repository: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(Error::storage(format!(
                "Repository {repository_id} not found"
            )));
        }

        tracing::info!("Deleted repository {repository_id} and all associated data");

        Ok(())
    }

    /// Get BM25 statistics for a repository
    pub async fn get_bm25_statistics(&self, repository_id: Uuid) -> Result<super::BM25Statistics> {
        let row = sqlx::query_as::<_, (Option<f32>, Option<i64>, Option<i64>)>(
            "SELECT bm25_avgdl, bm25_total_tokens, bm25_entity_count
             FROM repositories WHERE repository_id = $1",
        )
        .bind(repository_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get BM25 statistics: {e}")))?;

        let avgdl = row.0.ok_or_else(|| {
            Error::storage(format!(
                "BM25 statistics not initialized for repository {repository_id}"
            ))
        })?;
        let total_tokens = row.1.ok_or_else(|| {
            Error::storage(format!(
                "BM25 total_tokens not initialized for repository {repository_id}"
            ))
        })?;
        let entity_count = row.2.ok_or_else(|| {
            Error::storage(format!(
                "BM25 entity_count not initialized for repository {repository_id}"
            ))
        })?;

        Ok(super::BM25Statistics {
            avgdl,
            total_tokens,
            entity_count,
        })
    }

    /// Get BM25 statistics for a repository within a transaction (with row lock)
    pub async fn get_bm25_statistics_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        repository_id: Uuid,
    ) -> Result<super::BM25Statistics> {
        let row = sqlx::query_as::<_, (Option<f32>, Option<i64>, Option<i64>)>(
            "SELECT bm25_avgdl, bm25_total_tokens, bm25_entity_count
             FROM repositories WHERE repository_id = $1
             FOR UPDATE",
        )
        .bind(repository_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to get BM25 statistics in tx: {e}")))?;

        let avgdl = row.0.ok_or_else(|| {
            Error::storage(format!(
                "BM25 statistics not initialized for repository {repository_id}"
            ))
        })?;
        let total_tokens = row.1.ok_or_else(|| {
            Error::storage(format!(
                "BM25 total_tokens not initialized for repository {repository_id}"
            ))
        })?;
        let entity_count = row.2.ok_or_else(|| {
            Error::storage(format!(
                "BM25 entity_count not initialized for repository {repository_id}"
            ))
        })?;

        Ok(super::BM25Statistics {
            avgdl,
            total_tokens,
            entity_count,
        })
    }

    /// Get BM25 statistics for multiple repositories in a single query
    ///
    /// Optimized batch version for fetching statistics for many repositories at once.
    /// Uses PostgreSQL's ANY operator for efficient multi-row retrieval.
    pub async fn get_bm25_statistics_batch(
        &self,
        repository_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, super::BM25Statistics>> {
        if repository_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let rows = sqlx::query_as::<_, (Uuid, Option<f32>, Option<i64>, Option<i64>)>(
            "SELECT repository_id, bm25_avgdl, bm25_total_tokens, bm25_entity_count
             FROM repositories
             WHERE repository_id = ANY($1)",
        )
        .bind(repository_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to fetch batch BM25 statistics: {e}")))?;

        let mut result = std::collections::HashMap::new();
        for (repo_id, avgdl_opt, total_tokens_opt, entity_count_opt) in rows {
            // Filter incomplete statistics instead of failing the entire batch
            match (avgdl_opt, total_tokens_opt, entity_count_opt) {
                (Some(avgdl), Some(total_tokens), Some(entity_count)) => {
                    result.insert(
                        repo_id,
                        super::BM25Statistics {
                            avgdl,
                            total_tokens,
                            entity_count,
                        },
                    );
                }
                _ => {
                    tracing::warn!(
                        "Skipping repository {repo_id} with incomplete BM25 statistics (not yet initialized)"
                    );
                }
            }
        }

        Ok(result)
    }

    /// Update BM25 statistics incrementally after adding new entities (within transaction)
    pub async fn update_bm25_statistics_incremental_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        repository_id: Uuid,
        new_token_counts: &[usize],
    ) -> Result<f32> {
        let new_total_tokens: i64 = new_token_counts.iter().try_fold(0i64, |acc, &count| {
            let count_i64 = i64::try_from(count)
                .map_err(|_| Error::storage("Token count too large for i64"))?;
            acc.checked_add(count_i64)
                .ok_or_else(|| Error::storage("Token count overflow during aggregation"))
        })?;
        let new_entity_count: i64 = i64::try_from(new_token_counts.len())
            .map_err(|_| Error::storage("Entity count too large for i64"))?;

        // Perform atomic update with calculation in SQL to avoid race conditions
        let row = sqlx::query_scalar::<_, f32>(
            "UPDATE repositories
             SET bm25_total_tokens = bm25_total_tokens + $1,
                 bm25_entity_count = bm25_entity_count + $2,
                 bm25_avgdl = CASE
                     WHEN (bm25_entity_count + $2) > 0
                     THEN (bm25_total_tokens + $1)::float / (bm25_entity_count + $2)
                     ELSE CASE
                         WHEN bm25_avgdl > 0.0 THEN bm25_avgdl
                         ELSE 50.0
                     END
                 END,
                 updated_at = NOW()
             WHERE repository_id = $3
             RETURNING bm25_avgdl",
        )
        .bind(new_total_tokens)
        .bind(new_entity_count)
        .bind(repository_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to update BM25 statistics: {e}")))?;

        Ok(row)
    }

    /// Update BM25 statistics incrementally after adding new entities
    pub async fn update_bm25_statistics_incremental(
        &self,
        repository_id: Uuid,
        new_token_counts: &[usize],
    ) -> Result<f32> {
        let new_total_tokens: i64 = new_token_counts.iter().try_fold(0i64, |acc, &count| {
            let count_i64 = i64::try_from(count)
                .map_err(|_| Error::storage("Token count too large for i64"))?;
            acc.checked_add(count_i64)
                .ok_or_else(|| Error::storage("Token count overflow during aggregation"))
        })?;
        let new_entity_count: i64 = i64::try_from(new_token_counts.len())
            .map_err(|_| Error::storage("Entity count too large for i64"))?;

        // Perform atomic update with calculation in SQL to avoid race conditions
        let row = sqlx::query_scalar::<_, f32>(
            "UPDATE repositories
             SET bm25_total_tokens = bm25_total_tokens + $1,
                 bm25_entity_count = bm25_entity_count + $2,
                 bm25_avgdl = CASE
                     WHEN (bm25_entity_count + $2) > 0
                     THEN (bm25_total_tokens + $1)::float / (bm25_entity_count + $2)
                     ELSE CASE
                         WHEN bm25_avgdl > 0.0 THEN bm25_avgdl
                         ELSE 50.0
                     END
                 END,
                 updated_at = NOW()
             WHERE repository_id = $3
             RETURNING bm25_avgdl",
        )
        .bind(new_total_tokens)
        .bind(new_entity_count)
        .bind(repository_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to update BM25 statistics: {e}")))?;

        Ok(row)
    }

    /// Update BM25 statistics after deleting entities
    pub async fn update_bm25_statistics_after_deletion(
        &self,
        repository_id: Uuid,
        deleted_token_counts: &[usize],
    ) -> Result<f32> {
        let removed_total: i64 = deleted_token_counts.iter().try_fold(0i64, |acc, &count| {
            let count_i64 = i64::try_from(count)
                .map_err(|_| Error::storage("Token count too large for i64"))?;
            acc.checked_add(count_i64)
                .ok_or_else(|| Error::storage("Token count overflow during aggregation"))
        })?;
        let removed_count: i64 = i64::try_from(deleted_token_counts.len())
            .map_err(|_| Error::storage("Entity count too large for i64"))?;

        // Perform atomic update with calculation in SQL to avoid race conditions
        let row = sqlx::query_scalar::<_, f32>(
            "UPDATE repositories
             SET bm25_total_tokens = GREATEST(bm25_total_tokens - $1, 0),
                 bm25_entity_count = GREATEST(bm25_entity_count - $2, 0),
                 bm25_avgdl = CASE
                     WHEN GREATEST(bm25_entity_count - $2, 0) > 0
                     THEN GREATEST(bm25_total_tokens - $1, 0)::float / GREATEST(bm25_entity_count - $2, 0)
                     ELSE CASE
                         WHEN bm25_avgdl > 0.0 THEN bm25_avgdl
                         ELSE 50.0
                     END
                 END,
                 updated_at = NOW()
             WHERE repository_id = $3
             RETURNING bm25_avgdl",
        )
        .bind(removed_total)
        .bind(removed_count)
        .bind(repository_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            Error::storage(format!(
                "Failed to update BM25 statistics after deletion: {e}"
            ))
        })?;

        Ok(row)
    }

    /// Get token counts for entities (needed before deletion/update)
    pub async fn get_entity_token_counts(
        &self,
        entity_refs: &[(Uuid, String)],
    ) -> Result<Vec<usize>> {
        if entity_refs.is_empty() {
            return Ok(vec![]);
        }

        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT bm25_token_count FROM entity_metadata
             WHERE deleted_at IS NULL AND (repository_id, entity_id) IN ",
        );

        query_builder.push_tuples(entity_refs, |mut b, (repo_id, entity_id)| {
            b.push_bind(repo_id).push_bind(entity_id);
        });

        let rows = query_builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to get entity token counts: {e}")))?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                row.try_get::<Option<i32>, _>("bm25_token_count")
                    .ok()
                    .flatten()
            })
            .map(|count| count as usize)
            .collect())
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

    /// Get all entities of a specific type in a repository
    pub async fn get_entities_by_type(
        &self,
        repository_id: Uuid,
        entity_type: EntityType,
    ) -> Result<Vec<CodeEntity>> {
        let rows: Vec<(serde_json::Value, Option<String>)> = sqlx::query_as(
            "SELECT entity_data, content
             FROM entity_metadata
             WHERE repository_id = $1
               AND entity_type = $2
               AND deleted_at IS NULL",
        )
        .bind(repository_id)
        .bind(entity_type.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            Error::storage(format!("Failed to get entities by type {entity_type}: {e}"))
        })?;

        let entities = rows
            .into_iter()
            .filter_map(|(json, content)| {
                serde_json::from_value::<CodeEntity>(json)
                    .ok()
                    .map(|mut entity| {
                        // Use content from column, overriding any value in JSON
                        entity.content = content;
                        entity
                    })
            })
            .collect();

        Ok(entities)
    }

    /// Get all type entities (structs, enums, classes, interfaces, type aliases) in a repository
    pub async fn get_all_type_entities(&self, repository_id: Uuid) -> Result<Vec<CodeEntity>> {
        let rows: Vec<(serde_json::Value, Option<String>)> = sqlx::query_as(
            "SELECT entity_data, content
             FROM entity_metadata
             WHERE repository_id = $1
               AND entity_type IN ('struct', 'enum', 'class', 'interface', 'type_alias')
               AND deleted_at IS NULL",
        )
        .bind(repository_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get type entities: {e}")))?;

        let entities = rows
            .into_iter()
            .filter_map(|(json, content)| {
                serde_json::from_value::<CodeEntity>(json)
                    .ok()
                    .map(|mut entity| {
                        // Use content from column, overriding any value in JSON
                        entity.content = content;
                        entity
                    })
            })
            .collect();

        Ok(entities)
    }

    /// Get all entities in a repository
    pub async fn get_all_entities(&self, repository_id: Uuid) -> Result<Vec<CodeEntity>> {
        let rows: Vec<(serde_json::Value, Option<String>)> = sqlx::query_as(
            "SELECT entity_data, content
             FROM entity_metadata
             WHERE repository_id = $1
               AND deleted_at IS NULL",
        )
        .bind(repository_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get all entities: {e}")))?;

        let entities = rows
            .into_iter()
            .filter_map(|(json, content)| {
                serde_json::from_value::<CodeEntity>(json)
                    .ok()
                    .map(|mut entity| {
                        entity.content = content;
                        entity
                    })
            })
            .collect();

        Ok(entities)
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

    /// Batch fetch file snapshots for multiple files
    pub async fn get_file_snapshots_batch(
        &self,
        file_refs: &[(Uuid, String)],
    ) -> Result<std::collections::HashMap<(Uuid, String), Vec<String>>> {
        if file_refs.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        // Validate batch size
        if file_refs.len() > self.max_entity_batch_size {
            return Err(Error::storage(format!(
                "Batch size {} exceeds maximum allowed size of {}",
                file_refs.len(),
                self.max_entity_batch_size
            )));
        }

        // Build query using QueryBuilder
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT repository_id, file_path, entity_ids FROM file_entity_snapshots WHERE (repository_id, file_path) IN "
        );

        query_builder.push_tuples(file_refs, |mut b, (repo_id, file_path)| {
            b.push_bind(repo_id).push_bind(file_path);
        });

        let rows = query_builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to fetch file snapshots batch: {e}")))?;

        let mut result = std::collections::HashMap::new();
        for row in rows {
            let repository_id: Uuid = row
                .try_get("repository_id")
                .map_err(|e| Error::storage(format!("Failed to extract repository_id: {e}")))?;
            let file_path: String = row
                .try_get("file_path")
                .map_err(|e| Error::storage(format!("Failed to extract file_path: {e}")))?;
            let entity_ids: Vec<String> = row
                .try_get("entity_ids")
                .map_err(|e| Error::storage(format!("Failed to extract entity_ids: {e}")))?;

            result.insert((repository_id, file_path), entity_ids);
        }

        Ok(result)
    }

    /// Batch update file snapshots in a single transaction
    pub async fn update_file_snapshots_batch(
        &self,
        repository_id: Uuid,
        updates: &[(String, Vec<String>, Option<String>)],
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        // Validate batch size
        if updates.len() > self.max_entity_batch_size {
            return Err(Error::storage(format!(
                "Batch size {} exceeds maximum allowed size of {}",
                updates.len(),
                self.max_entity_batch_size
            )));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        // Build bulk upsert
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO file_entity_snapshots (repository_id, file_path, entity_ids, git_commit_hash, indexed_at) "
        );

        query_builder.push_values(updates, |mut b, (file_path, entity_ids, git_commit)| {
            b.push_bind(repository_id)
                .push_bind(file_path)
                .push_bind(entity_ids)
                .push_bind(git_commit)
                .push("NOW()");
        });

        query_builder.push(
            " ON CONFLICT (repository_id, file_path)
            DO UPDATE SET
                entity_ids = EXCLUDED.entity_ids,
                git_commit_hash = EXCLUDED.git_commit_hash,
                indexed_at = NOW()",
        );

        query_builder
            .build()
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to batch update file snapshots: {e}")))?;

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
            "SELECT entity_data, content FROM entity_metadata WHERE deleted_at IS NULL AND (repository_id, entity_id) IN "
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
            let content: Option<String> = row
                .try_get("content")
                .map_err(|e| Error::storage(format!("Failed to extract content: {e}")))?;
            let mut entity: CodeEntity = serde_json::from_value(entity_json)
                .map_err(|e| Error::storage(format!("Failed to deserialize entity: {e}")))?;
            // Use content from column, overriding any value in JSON
            entity.content = content;
            entities.push(entity);
        }

        Ok(entities)
    }

    /// Mark entities as deleted and create outbox entries in a single transaction
    ///
    /// Token counts are stored in the outbox payload for later BM25 statistics update
    /// by the outbox processor.
    pub async fn mark_entities_deleted_with_outbox(
        &self,
        repository_id: Uuid,
        collection_name: &str,
        entity_ids: &[String],
        token_counts: &[usize],
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

        // Validate token_counts length matches entity_ids
        if token_counts.len() != entity_ids.len() {
            return Err(Error::storage(format!(
                "Token counts length {} does not match entity_ids length {}",
                token_counts.len(),
                entity_ids.len()
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

        // 2. Create outbox entries only for entities that were actually deleted
        // If no entities were updated (all non-existent), skip outbox creation
        if update_result.rows_affected() > 0 {
            // Get the list of entities that were actually updated (exist in DB)
            let mut check_query: QueryBuilder<Postgres> =
                QueryBuilder::new("SELECT entity_id FROM entity_metadata WHERE repository_id = ");
            check_query.push_bind(repository_id);
            check_query.push(" AND entity_id IN (");

            let mut separated = check_query.separated(", ");
            for entity_id in entity_ids {
                separated.push_bind(entity_id);
            }
            separated.push_unseparated(") AND deleted_at IS NOT NULL");

            let existing_entity_ids: Vec<String> = check_query
                .build_query_as()
                .fetch_all(&mut *tx)
                .await
                .map_err(|e| Error::storage(format!("Failed to query deleted entities: {e}")))?
                .into_iter()
                .map(|(id,): (String,)| id)
                .collect();

            if !existing_entity_ids.is_empty() {
                // Build entity_id to token_count map for looking up token counts
                let token_count_map: std::collections::HashMap<&String, usize> = entity_ids
                    .iter()
                    .zip(token_counts.iter())
                    .map(|(id, &count)| (id, count))
                    .collect();

                let mut outbox_query: QueryBuilder<Postgres> = QueryBuilder::new(
                    "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store, payload, collection_name, created_at) "
                );

                outbox_query.push_values(&existing_entity_ids, |mut b, entity_id| {
                    // Look up token count for this entity_id
                    let token_count = token_count_map.get(entity_id).copied().unwrap_or(0);
                    let payload = serde_json::json!({
                        "entity_ids": [entity_id],
                        "token_counts": [token_count],
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

                // Create Neo4j DELETE outbox entries
                let mut neo4j_outbox_query: QueryBuilder<Postgres> = QueryBuilder::new(
                    "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store, payload, collection_name, created_at) "
                );

                neo4j_outbox_query.push_values(&existing_entity_ids, |mut b, entity_id| {
                    let payload = serde_json::json!({
                        "entity_id": entity_id,
                    });
                    b.push_bind(repository_id)
                        .push_bind(entity_id)
                        .push_bind(OutboxOperation::Delete.to_string())
                        .push_bind(TargetStore::Neo4j.to_string())
                        .push_bind(payload)
                        .push_bind(collection_name)
                        .push("NOW()");
                });

                neo4j_outbox_query
                    .build()
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| {
                        Error::storage(format!("Failed to write Neo4j outbox entries: {e}"))
                    })?;
            }
        }

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        tracing::info!(
            "Marked {} entities as deleted with outbox entries (token counts stored in payload)",
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
            .map(
                |(entity, embedding, op, point_id, target, git_commit, token_count)| {
                    // Create entity without content for JSON storage
                    let mut entity_without_content = (*entity).clone();

                    // Extract content (will be stored in separate column)
                    // Use take() to move instead of cloning again
                    let content = entity_without_content.content.take();

                    let entity_json = serde_json::to_value(&entity_without_content)
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
                        token_count,
                        entity_json,
                        file_path_str,
                        content,
                    ))
                },
            )
            .collect();

        let validated_entities = validated_entities?;

        // Build bulk insert for entity_metadata
        let mut entity_query: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO entity_metadata (
                entity_id, repository_id, qualified_name, name, parent_scope,
                entity_type, language, file_path, visibility,
                entity_data, git_commit_hash, qdrant_point_id, embedding_id, bm25_token_count, content
            ) ",
        );

        entity_query.push_values(
            &validated_entities,
            |mut b,
             (
                entity,
                embedding_id,
                _op,
                point_id,
                _target,
                git_commit,
                token_count,
                entity_json,
                file_path_str,
                content,
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
                    .push_bind(point_id)
                    .push_bind(embedding_id)
                    .push_bind(**token_count as i32)
                    .push_bind(content);
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
                embedding_id = EXCLUDED.embedding_id,
                bm25_token_count = EXCLUDED.bm25_token_count,
                content = EXCLUDED.content,
                updated_at = NOW(),
                deleted_at = NULL",
        );

        entity_query.build().execute(&mut *tx).await.map_err(|e| {
            Error::storage(format!(
                "Failed to bulk insert entity metadata (batch size {}): {e}",
                validated_entities.len()
            ))
        })?;

        // Build bulk insert for entity_outbox
        let mut outbox_query: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO entity_outbox (
                repository_id, entity_id, operation, target_store, payload, collection_name, embedding_id
            ) ",
        );

        outbox_query.push_values(
            &validated_entities,
            |mut b,
             (
                entity,
                embedding_id,
                op,
                point_id,
                target,
                _git_commit,
                _token_count,
                entity_json,
                _file_path_str,
                _content,
            )| {
                let payload = serde_json::json!({
                    "entity": entity_json,
                    "qdrant_point_id": point_id.to_string(),
                });

                b.push_bind(repository_id)
                    .push_bind(&entity.entity_id)
                    .push_bind(op.to_string())
                    .push_bind(target.to_string())
                    .push_bind(payload)
                    .push_bind(collection_name)
                    .push_bind(embedding_id);
            },
        );

        outbox_query.push(" RETURNING outbox_id");

        let outbox_ids: Vec<Uuid> = outbox_query
            .build_query_scalar()
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to bulk insert outbox entries: {e}")))?;

        // Build bulk insert for Neo4j outbox entries
        // Extract just the entities for relationship resolution
        let entities_in_batch: Vec<CodeEntity> = validated_entities
            .iter()
            .map(|(entity, ..)| (**entity).clone())
            .collect();

        // Build name -> entity_id map for O(1) relationship resolution
        // Include both qualified_name and simple name as keys to handle parent_scope lookups
        // (parent_scope may be just the name, not the full qualified_name)
        let mut name_to_id: std::collections::HashMap<&str, &str> =
            std::collections::HashMap::with_capacity(entities_in_batch.len() * 2);
        for entity in &entities_in_batch {
            // Add qualified_name as primary key
            name_to_id.insert(entity.qualified_name.as_str(), entity.entity_id.as_str());
            // Also add simple name (only if not already present to avoid collisions)
            name_to_id
                .entry(entity.name.as_str())
                .or_insert(entity.entity_id.as_str());
        }

        let mut neo4j_outbox_query: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO entity_outbox (
                repository_id, entity_id, operation, target_store, payload, collection_name
            ) ",
        );

        neo4j_outbox_query.push_values(
            &validated_entities,
            |mut b,
             (
                entity,
                _embedding_id,
                op,
                _point_id,
                _target,
                _git_commit,
                _token_count,
                _entity_json,
                _file_path_str,
                _content,
            )| {
                let neo4j_payload = self
                    .build_neo4j_payload(entity, &name_to_id)
                    .unwrap_or_else(|_| {
                        serde_json::json!({
                            "node": {},
                            "labels": [],
                            "relationships": []
                        })
                    });

                b.push_bind(repository_id)
                    .push_bind(&entity.entity_id)
                    .push_bind(op.to_string())
                    .push_bind(TargetStore::Neo4j.to_string())
                    .push_bind(neo4j_payload)
                    .push_bind(collection_name);
            },
        );

        neo4j_outbox_query
            .build()
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                Error::storage(format!("Failed to bulk insert Neo4j outbox entries: {e}"))
            })?;

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
                    created_at, processed_at, retry_count, last_error, collection_name, embedding_id
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

    /// Count pending (unprocessed) outbox entries across all target stores
    ///
    /// Returns the total number of outbox entries that have not yet been processed.
    /// This is used to determine when the outbox has been fully drained.
    pub async fn count_pending_outbox_entries(&self) -> Result<i64> {
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM entity_outbox WHERE processed_at IS NULL")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| {
                    Error::storage(format!("Failed to count pending outbox entries: {e}"))
                })?;

        Ok(count.0)
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

        sqlx::query("TRUNCATE TABLE entity_embeddings CASCADE")
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to truncate entity_embeddings: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        tracing::info!("Dropped all data from PostgreSQL tables");

        Ok(())
    }

    /// Get embeddings by content hashes, returning (embedding_id, dense_embedding, sparse_embedding) tuples
    pub async fn get_embeddings_by_content_hash(
        &self,
        repository_id: Uuid,
        content_hashes: &[String],
        model_version: &str,
    ) -> Result<std::collections::HashMap<String, (i64, Vec<f32>, Option<Vec<(u32, f32)>>)>> {
        if content_hashes.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        // Validate batch size
        if content_hashes.len() > self.max_entity_batch_size {
            return Err(Error::storage(format!(
                "Cache lookup batch size {} exceeds maximum {}",
                content_hashes.len(),
                self.max_entity_batch_size
            )));
        }

        // Build query with IN clause
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT id, content_hash, embedding, sparse_indices, sparse_values FROM entity_embeddings WHERE repository_id = ",
        );
        query_builder.push_bind(repository_id);
        query_builder.push(" AND model_version = ");
        query_builder.push_bind(model_version);
        query_builder.push(" AND content_hash IN (");

        let mut separated = query_builder.separated(", ");
        for hash in content_hashes {
            separated.push_bind(hash);
        }
        separated.push_unseparated(")");

        let rows = query_builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                Error::storage(format!("Failed to fetch embeddings by content hash: {e}"))
            })?;

        let mut result = std::collections::HashMap::new();
        for row in rows {
            let id: i64 = row
                .try_get("id")
                .map_err(|e| Error::storage(format!("Failed to extract id: {e}")))?;
            let content_hash: String = row
                .try_get("content_hash")
                .map_err(|e| Error::storage(format!("Failed to extract content_hash: {e}")))?;
            let embedding: Vec<f32> = row
                .try_get("embedding")
                .map_err(|e| Error::storage(format!("Failed to extract embedding: {e}")))?;
            let sparse_indices: Option<Vec<i64>> = row
                .try_get("sparse_indices")
                .map_err(|e| Error::storage(format!("Failed to extract sparse_indices: {e}")))?;
            let sparse_values: Option<Vec<f32>> = row
                .try_get("sparse_values")
                .map_err(|e| Error::storage(format!("Failed to extract sparse_values: {e}")))?;

            let sparse_embedding = match (sparse_indices, sparse_values) {
                (Some(indices), Some(values)) => Some(arrays_to_sparse_embedding(indices, values)?),
                _ => None,
            };

            result.insert(content_hash, (id, embedding, sparse_embedding));
        }

        Ok(result)
    }

    /// Store embeddings in entity_embeddings table, returning their IDs
    pub async fn store_embeddings(
        &self,
        repository_id: Uuid,
        cache_entries: &[EmbeddingCacheEntry],
        model_version: &str,
        dimension: usize,
    ) -> Result<Vec<i64>> {
        if cache_entries.is_empty() {
            return Ok(Vec::new());
        }

        // Validate batch size
        if cache_entries.len() > self.max_entity_batch_size {
            return Err(Error::storage(format!(
                "Embedding store batch size {} exceeds maximum {}",
                cache_entries.len(),
                self.max_entity_batch_size
            )));
        }

        // Validate and convert all sparse embeddings upfront
        let validated_sparse: Result<Vec<ValidatedSparseArrays>> = cache_entries
            .iter()
            .map(|(_, _, sparse_embedding)| {
                sparse_embedding
                    .as_ref()
                    .map(|s| sparse_embedding_to_arrays(s).map(|(i, v)| (Some(i), Some(v))))
                    .transpose()
                    .map(|opt| opt.unwrap_or((None, None)))
            })
            .collect();
        let validated_sparse = validated_sparse?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        // Build bulk INSERT with ON CONFLICT DO NOTHING
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO entity_embeddings (repository_id, content_hash, embedding, sparse_indices, sparse_values, model_version, dimension, created_at) "
        );

        query_builder.push_values(
            cache_entries.iter().zip(validated_sparse.iter()),
            |mut b, ((content_hash, embedding, _), (sparse_indices, sparse_values))| {
                b.push_bind(repository_id)
                    .push_bind(content_hash)
                    .push_bind(embedding)
                    .push_bind(sparse_indices)
                    .push_bind(sparse_values)
                    .push_bind(model_version)
                    .push_bind(dimension as i32)
                    .push("NOW()");
            },
        );

        query_builder.push(" ON CONFLICT (repository_id, content_hash) DO NOTHING RETURNING id");

        // Execute and get IDs for newly inserted rows
        let inserted_ids: Vec<i64> = query_builder
            .build_query_scalar()
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| Error::storage(format!("Failed to insert embeddings: {e}")))?;

        // Build result in the correct order matching input cache_entries
        let all_ids = if inserted_ids.len() == cache_entries.len() {
            // All entries were new, IDs are in correct order
            inserted_ids
        } else {
            // Some entries already existed, need to fetch and order correctly
            let mut fetch_query: QueryBuilder<Postgres> = QueryBuilder::new(
                "SELECT content_hash, id FROM entity_embeddings WHERE repository_id = ",
            );
            fetch_query.push_bind(repository_id);
            fetch_query.push(" AND model_version = ");
            fetch_query.push_bind(model_version);
            fetch_query.push(" AND content_hash IN (");

            let mut separated = fetch_query.separated(", ");
            for (hash, _, _) in cache_entries {
                separated.push_bind(hash);
            }
            separated.push_unseparated(")");

            let rows = fetch_query.build().fetch_all(&mut *tx).await.map_err(|e| {
                Error::storage(format!("Failed to fetch existing embedding IDs: {e}"))
            })?;

            // Build HashMap for O(1) lookup
            let mut hash_to_id = std::collections::HashMap::new();
            for row in rows {
                let content_hash: String = row
                    .try_get("content_hash")
                    .map_err(|e| Error::storage(format!("Failed to extract content_hash: {e}")))?;
                let id: i64 = row
                    .try_get("id")
                    .map_err(|e| Error::storage(format!("Failed to extract id: {e}")))?;
                hash_to_id.insert(content_hash, id);
            }

            // Return IDs in the same order as input cache_entries
            let mut ordered_ids = Vec::with_capacity(cache_entries.len());
            for (hash, _, _) in cache_entries {
                let id = hash_to_id.get(hash).ok_or_else(|| {
                    Error::storage(format!(
                        "Embedding ID not found for content_hash: {hash} (this should not happen)"
                    ))
                })?;
                ordered_ids.push(*id);
            }

            ordered_ids
        };

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit embedding transaction: {e}")))?;

        Ok(all_ids)
    }

    /// Get an embedding by its ID (used by outbox processor)
    pub async fn get_embedding_by_id(&self, embedding_id: i64) -> Result<Option<Vec<f32>>> {
        let record: Option<(Vec<f32>,)> =
            sqlx::query_as("SELECT embedding FROM entity_embeddings WHERE id = $1")
                .bind(embedding_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to get embedding by ID: {e}")))?;

        Ok(record.map(|(embedding,)| embedding))
    }

    /// Fetch both dense and sparse embeddings by ID from entity_embeddings table
    pub async fn get_embedding_with_sparse_by_id(
        &self,
        embedding_id: i64,
    ) -> Result<Option<(Vec<f32>, Option<Vec<(u32, f32)>>)>> {
        let record: Option<SparseEmbeddingRow> = sqlx::query_as(
            "SELECT embedding, sparse_indices, sparse_values FROM entity_embeddings WHERE id = $1",
        )
        .bind(embedding_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get embeddings by ID: {e}")))?;

        record
            .map(|(dense, sparse_indices, sparse_values)| {
                let sparse = match (sparse_indices, sparse_values) {
                    (Some(indices), Some(values)) => {
                        Some(arrays_to_sparse_embedding(indices, values)?)
                    }
                    _ => None,
                };
                Ok((dense, sparse))
            })
            .transpose()
    }

    /// Batch fetch embeddings by IDs from entity_embeddings table
    ///
    /// Optimized batch version for fetching embeddings for many entities at once.
    /// Uses a single query with ANY() instead of N individual queries.
    pub async fn get_embeddings_with_sparse_by_ids(
        &self,
        embedding_ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, (Vec<f32>, Option<Vec<(u32, f32)>>)>> {
        if embedding_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let rows: Vec<BatchEmbeddingRow> = sqlx::query_as(
            "SELECT id, embedding, sparse_indices, sparse_values FROM entity_embeddings WHERE id = ANY($1)",
        )
        .bind(embedding_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to batch fetch embeddings by IDs: {e}")))?;

        let mut result = std::collections::HashMap::with_capacity(rows.len());
        for (id, dense, sparse_indices, sparse_values) in rows {
            let sparse = match (sparse_indices, sparse_values) {
                (Some(indices), Some(values)) => Some(arrays_to_sparse_embedding(indices, values)?),
                _ => None,
            };
            result.insert(id, (dense, sparse));
        }

        Ok(result)
    }

    /// Fetch cached dense embeddings for entities by qualified names within a single repository
    pub async fn get_embeddings_by_qualified_names(
        &self,
        repository_id: Uuid,
        qualified_names: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<f32>>> {
        if qualified_names.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let rows: Vec<(String, Vec<f32>)> = sqlx::query_as(
            "SELECT
                e.entity_data->>'qualified_name' as qualified_name,
                ee.embedding
            FROM entity_metadata e
            JOIN entity_embeddings ee ON e.embedding_id = ee.id
            WHERE e.repository_id = $1
                AND e.deleted_at IS NULL
                AND e.entity_data->>'qualified_name' = ANY($2)",
        )
        .bind(repository_id)
        .bind(qualified_names)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            Error::storage(format!(
                "Failed to fetch embeddings by qualified names: {e}"
            ))
        })?;

        let found_count = rows.len();
        let requested_count = qualified_names.len();
        if found_count < requested_count {
            tracing::debug!(
                "Embedding cache partial hit: found {found_count}/{requested_count} embeddings"
            );
        }

        Ok(rows.into_iter().collect())
    }

    /// Get full entities by their qualified names
    pub async fn get_entities_by_qualified_names(
        &self,
        repository_id: Uuid,
        qualified_names: &[String],
    ) -> Result<std::collections::HashMap<String, CodeEntity>> {
        if qualified_names.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let rows: Vec<(String, sqlx::types::JsonValue)> = sqlx::query_as(
            "SELECT
                e.entity_data->>'qualified_name' as qualified_name,
                e.entity_data
            FROM entity_metadata e
            WHERE e.repository_id = $1
                AND e.deleted_at IS NULL
                AND e.entity_data->>'qualified_name' = ANY($2)",
        )
        .bind(repository_id)
        .bind(qualified_names)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to fetch entities by qualified names: {e}")))?;

        let mut result = std::collections::HashMap::new();
        for (qname, entity_json) in rows {
            let entity: CodeEntity = serde_json::from_value(entity_json)
                .map_err(|e| Error::storage(format!("Failed to deserialize entity: {e}")))?;
            result.insert(qname, entity);
        }

        Ok(result)
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> Result<crate::CacheStats> {
        let row = sqlx::query(
            "SELECT
                COUNT(*) as total_entries,
                SUM(array_length(embedding, 1) * 4) as total_size_bytes,
                MIN(created_at) as oldest_entry,
                MAX(created_at) as newest_entry
            FROM entity_embeddings",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get cache stats: {e}")))?;

        let total_entries: i64 = row
            .try_get("total_entries")
            .map_err(|e| Error::storage(format!("Failed to extract total_entries: {e}")))?;
        let total_size_bytes: Option<i64> = row.try_get("total_size_bytes").ok();
        let oldest_entry = row.try_get("oldest_entry").ok();
        let newest_entry = row.try_get("newest_entry").ok();

        // Get counts by model version
        let model_rows = sqlx::query(
            "SELECT model_version, COUNT(*) as count FROM entity_embeddings GROUP BY model_version",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get model version stats: {e}")))?;

        let mut entries_by_model = std::collections::HashMap::new();
        for row in model_rows {
            let model: String = row
                .try_get("model_version")
                .map_err(|e| Error::storage(format!("Failed to extract model_version: {e}")))?;
            let count: i64 = row
                .try_get("count")
                .map_err(|e| Error::storage(format!("Failed to extract count: {e}")))?;
            entries_by_model.insert(model, count);
        }

        Ok(crate::CacheStats {
            total_entries,
            total_size_bytes: total_size_bytes.unwrap_or(0),
            entries_by_model,
            oldest_entry,
            newest_entry,
        })
    }

    /// Clear cache entries (optionally filter by model version)
    pub async fn clear_cache(&self, model_version: Option<&str>) -> Result<u64> {
        let result = if let Some(version) = model_version {
            sqlx::query("DELETE FROM entity_embeddings WHERE model_version = $1")
                .bind(version)
                .execute(&self.pool)
                .await
        } else {
            sqlx::query("DELETE FROM entity_embeddings")
                .execute(&self.pool)
                .await
        };

        let rows_affected = result
            .map_err(|e| Error::storage(format!("Failed to clear cache: {e}")))?
            .rows_affected();

        tracing::info!("Cleared {} cache entries", rows_affected);
        Ok(rows_affected)
    }

    /// Build Neo4j payload from entity for outbox entry
    fn build_neo4j_payload(
        &self,
        entity: &CodeEntity,
        name_to_id: &std::collections::HashMap<&str, &str>,
    ) -> Result<serde_json::Value> {
        // Extract core properties using proper struct
        let properties = Neo4jNodeProperties {
            id: entity.entity_id.clone(),
            repository_id: entity.repository_id.to_string(),
            qualified_name: entity.qualified_name.clone(),
            name: entity.name.clone(),
            language: entity.language.to_string(),
            visibility: entity.visibility.to_string(),
            is_async: entity.metadata.is_async,
            is_generic: entity.metadata.is_generic,
            is_static: entity.metadata.is_static,
            is_abstract: entity.metadata.is_abstract,
            is_const: entity.metadata.is_const,
        };

        // Determine labels from entity type
        let labels = match entity.entity_type {
            EntityType::Function => vec!["Function"],
            EntityType::Method => vec!["Method"],
            EntityType::Class => vec!["Class"],
            EntityType::Struct => vec!["Struct", "Class"],
            EntityType::Interface => vec!["Interface"],
            EntityType::Trait => vec!["Trait", "Interface"],
            EntityType::Enum => vec!["Enum"],
            EntityType::Module => vec!["Module"],
            EntityType::Package => vec!["Package"],
            EntityType::Constant => vec!["Constant"],
            EntityType::Variable => vec!["Variable"],
            EntityType::TypeAlias => vec!["TypeAlias"],
            EntityType::Macro => vec!["Macro"],
            EntityType::Impl => vec!["ImplBlock"],
        };

        // Extract CONTAINS relationships using O(1) HashMap lookup
        let mut relationships = build_contains_relationship_json(entity, name_to_id);

        // Extract IMPLEMENTS and EXTENDS_INTERFACE relationships
        relationships.extend(build_trait_relationship_json(entity));

        // Extract INHERITS_FROM relationships
        relationships.extend(build_inherits_from_relationship_json(entity));

        // Extract USES relationships (field type dependencies)
        relationships.extend(build_uses_relationship_json(entity));

        // Extract CALLS relationships (function calls)
        relationships.extend(build_calls_relationship_json(entity));

        // Extract IMPORTS relationships (module imports)
        relationships.extend(build_imports_relationship_json(entity));

        // Build payload using proper struct
        let payload = Neo4jOutboxPayload {
            entity,
            node: properties,
            labels,
            relationships,
        };

        // Serialize to JSON
        serde_json::to_value(&payload)
            .map_err(|e| Error::storage(format!("Failed to serialize Neo4j payload: {e}")))
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

    async fn get_neo4j_database_name(&self, repository_id: Uuid) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT neo4j_database_name FROM repositories WHERE repository_id = $1")
                .bind(repository_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to get neo4j database name: {e}")))?;

        Ok(row.and_then(|(name,)| name))
    }

    async fn set_neo4j_database_name(&self, repository_id: Uuid, db_name: &str) -> Result<()> {
        sqlx::query("UPDATE repositories SET neo4j_database_name = $1 WHERE repository_id = $2")
            .bind(db_name)
            .bind(repository_id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to set neo4j database name: {e}")))?;

        Ok(())
    }

    async fn set_graph_ready(&self, repository_id: Uuid, ready: bool) -> Result<()> {
        sqlx::query("UPDATE repositories SET graph_ready = $1 WHERE repository_id = $2")
            .bind(ready)
            .bind(repository_id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to set graph_ready: {e}")))?;

        Ok(())
    }

    async fn is_graph_ready(&self, repository_id: Uuid) -> Result<bool> {
        let row: (bool,) = sqlx::query_as(
            "SELECT COALESCE(graph_ready, FALSE) FROM repositories WHERE repository_id = $1",
        )
        .bind(repository_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get graph_ready status: {e}")))?;

        Ok(row.0)
    }

    async fn set_pending_relationship_resolution(
        &self,
        repository_id: Uuid,
        pending: bool,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE repositories SET pending_relationship_resolution = $1 WHERE repository_id = $2",
        )
        .bind(pending)
        .bind(repository_id)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            Error::storage(format!(
                "Failed to set pending_relationship_resolution: {e}"
            ))
        })?;

        Ok(())
    }

    async fn has_pending_relationship_resolution(&self, repository_id: Uuid) -> Result<bool> {
        let row: (bool,) = sqlx::query_as(
            "SELECT COALESCE(pending_relationship_resolution, FALSE)
             FROM repositories
             WHERE repository_id = $1",
        )
        .bind(repository_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            Error::storage(format!(
                "Failed to get pending_relationship_resolution status: {e}"
            ))
        })?;

        Ok(row.0)
    }

    async fn get_repositories_with_pending_resolution(&self) -> Result<Vec<Uuid>> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT repository_id
             FROM repositories
             WHERE pending_relationship_resolution = TRUE
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            Error::storage(format!(
                "Failed to get repositories with pending resolution: {e}"
            ))
        })?;

        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_repository_by_collection(
        &self,
        collection_name: &str,
    ) -> Result<Option<(Uuid, std::path::PathBuf, String)>> {
        self.get_repository_by_collection(collection_name).await
    }

    async fn get_repository_by_path(
        &self,
        repository_path: &std::path::Path,
    ) -> Result<Option<(Uuid, String)>> {
        self.get_repository_by_path(repository_path).await
    }

    async fn list_all_repositories(&self) -> Result<Vec<(Uuid, String, std::path::PathBuf)>> {
        self.list_all_repositories().await
    }

    async fn drop_repository(&self, repository_id: Uuid) -> Result<()> {
        self.drop_repository(repository_id).await
    }

    async fn get_bm25_statistics(&self, repository_id: Uuid) -> Result<super::BM25Statistics> {
        self.get_bm25_statistics(repository_id).await
    }

    async fn get_bm25_statistics_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        repository_id: Uuid,
    ) -> Result<super::BM25Statistics> {
        self.get_bm25_statistics_in_tx(tx, repository_id).await
    }

    async fn get_bm25_statistics_batch(
        &self,
        repository_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, super::BM25Statistics>> {
        self.get_bm25_statistics_batch(repository_ids).await
    }

    async fn update_bm25_statistics_incremental_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        repository_id: Uuid,
        new_token_counts: &[usize],
    ) -> Result<f32> {
        self.update_bm25_statistics_incremental_in_tx(tx, repository_id, new_token_counts)
            .await
    }

    async fn update_bm25_statistics_incremental(
        &self,
        repository_id: Uuid,
        new_token_counts: &[usize],
    ) -> Result<f32> {
        self.update_bm25_statistics_incremental(repository_id, new_token_counts)
            .await
    }

    async fn update_bm25_statistics_after_deletion(
        &self,
        repository_id: Uuid,
        deleted_token_counts: &[usize],
    ) -> Result<f32> {
        self.update_bm25_statistics_after_deletion(repository_id, deleted_token_counts)
            .await
    }

    async fn get_entity_token_counts(&self, entity_refs: &[(Uuid, String)]) -> Result<Vec<usize>> {
        self.get_entity_token_counts(entity_refs).await
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

    async fn get_file_snapshots_batch(
        &self,
        file_refs: &[(Uuid, String)],
    ) -> Result<std::collections::HashMap<(Uuid, String), Vec<String>>> {
        self.get_file_snapshots_batch(file_refs).await
    }

    async fn update_file_snapshots_batch(
        &self,
        repository_id: Uuid,
        updates: &[(String, Vec<String>, Option<String>)],
    ) -> Result<()> {
        self.update_file_snapshots_batch(repository_id, updates)
            .await
    }

    async fn get_entities_by_ids(&self, entity_refs: &[(Uuid, String)]) -> Result<Vec<CodeEntity>> {
        self.get_entities_by_ids(entity_refs).await
    }

    async fn get_entities_by_type(
        &self,
        repository_id: Uuid,
        entity_type: EntityType,
    ) -> Result<Vec<CodeEntity>> {
        self.get_entities_by_type(repository_id, entity_type).await
    }

    async fn get_all_type_entities(&self, repository_id: Uuid) -> Result<Vec<CodeEntity>> {
        self.get_all_type_entities(repository_id).await
    }

    async fn get_all_entities(&self, repository_id: Uuid) -> Result<Vec<CodeEntity>> {
        self.get_all_entities(repository_id).await
    }

    async fn mark_entities_deleted_with_outbox(
        &self,
        repository_id: Uuid,
        collection_name: &str,
        entity_ids: &[String],
        token_counts: &[usize],
    ) -> Result<()> {
        self.mark_entities_deleted_with_outbox(
            repository_id,
            collection_name,
            entity_ids,
            token_counts,
        )
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

    async fn count_pending_outbox_entries(&self) -> Result<i64> {
        self.count_pending_outbox_entries().await
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

    async fn get_embeddings_by_content_hash(
        &self,
        repository_id: Uuid,
        content_hashes: &[String],
        model_version: &str,
    ) -> Result<std::collections::HashMap<String, (i64, Vec<f32>, Option<Vec<(u32, f32)>>)>> {
        self.get_embeddings_by_content_hash(repository_id, content_hashes, model_version)
            .await
    }

    async fn store_embeddings(
        &self,
        repository_id: Uuid,
        cache_entries: &[(String, Vec<f32>, Option<Vec<(u32, f32)>>)],
        model_version: &str,
        dimension: usize,
    ) -> Result<Vec<i64>> {
        self.store_embeddings(repository_id, cache_entries, model_version, dimension)
            .await
    }

    async fn get_embedding_by_id(&self, embedding_id: i64) -> Result<Option<Vec<f32>>> {
        self.get_embedding_by_id(embedding_id).await
    }

    async fn get_embedding_with_sparse_by_id(
        &self,
        embedding_id: i64,
    ) -> Result<Option<(Vec<f32>, Option<Vec<(u32, f32)>>)>> {
        self.get_embedding_with_sparse_by_id(embedding_id).await
    }

    async fn get_embeddings_with_sparse_by_ids(
        &self,
        embedding_ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, (Vec<f32>, Option<Vec<(u32, f32)>>)>> {
        self.get_embeddings_with_sparse_by_ids(embedding_ids).await
    }

    async fn get_cache_stats(&self) -> Result<crate::CacheStats> {
        self.get_cache_stats().await
    }

    async fn clear_cache(&self, model_version: Option<&str>) -> Result<u64> {
        self.clear_cache(model_version).await
    }

    async fn get_embeddings_by_qualified_names(
        &self,
        repository_id: Uuid,
        qualified_names: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<f32>>> {
        self.get_embeddings_by_qualified_names(repository_id, qualified_names)
            .await
    }

    async fn get_entities_by_qualified_names(
        &self,
        repository_id: Uuid,
        qualified_names: &[String],
    ) -> Result<std::collections::HashMap<String, CodeEntity>> {
        self.get_entities_by_qualified_names(repository_id, qualified_names)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparse_embedding_to_arrays_empty() {
        let sparse: Vec<(u32, f32)> = vec![];
        let result = sparse_embedding_to_arrays(&sparse).unwrap();
        assert_eq!(result.0.len(), 0);
        assert_eq!(result.1.len(), 0);
    }

    #[test]
    fn test_sparse_embedding_to_arrays_simple() {
        let sparse = vec![(0, 1.0), (5, 2.5), (100, 0.5)];
        let (indices, values) = sparse_embedding_to_arrays(&sparse).unwrap();
        assert_eq!(indices, vec![0, 5, 100]);
        assert_eq!(values, vec![1.0, 2.5, 0.5]);
    }

    #[test]
    fn test_sparse_embedding_to_arrays_large_indices() {
        let sparse = vec![(u32::MAX, 1.0), (u32::MAX - 1, 2.0)];
        let (indices, values) = sparse_embedding_to_arrays(&sparse).unwrap();
        assert_eq!(indices, vec![u32::MAX as i64, (u32::MAX - 1) as i64]);
        assert_eq!(values, vec![1.0, 2.0]);
    }

    #[test]
    fn test_sparse_embedding_to_arrays_exceeds_max_size() {
        let sparse: Vec<(u32, f32)> = (0..MAX_SPARSE_EMBEDDING_SIZE + 1)
            .map(|i| (i as u32, 1.0))
            .collect();
        let result = sparse_embedding_to_arrays(&sparse);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("exceeds maximum allowed size"));
    }

    #[test]
    fn test_arrays_to_sparse_embedding_empty() {
        let indices: Vec<i64> = vec![];
        let values: Vec<f32> = vec![];
        let result = arrays_to_sparse_embedding(indices, values).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_arrays_to_sparse_embedding_simple() {
        let indices = vec![0, 5, 100];
        let values = vec![1.0, 2.5, 0.5];
        let result = arrays_to_sparse_embedding(indices, values).unwrap();
        assert_eq!(result, vec![(0, 1.0), (5, 2.5), (100, 0.5)]);
    }

    #[test]
    fn test_arrays_to_sparse_embedding_large_indices() {
        let indices = vec![u32::MAX as i64, (u32::MAX - 1) as i64];
        let values = vec![1.0, 2.0];
        let result = arrays_to_sparse_embedding(indices, values).unwrap();
        assert_eq!(result, vec![(u32::MAX, 1.0), (u32::MAX - 1, 2.0)]);
    }

    #[test]
    fn test_arrays_to_sparse_embedding_mismatched_lengths() {
        let indices = vec![0, 1, 2];
        let values = vec![1.0, 2.0]; // One less value
        let result = arrays_to_sparse_embedding(indices, values);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("does not match"));
    }

    #[test]
    fn test_arrays_to_sparse_embedding_exceeds_max_size() {
        let indices: Vec<i64> = (0..MAX_SPARSE_EMBEDDING_SIZE + 1)
            .map(|i| i as i64)
            .collect();
        let values: Vec<f32> = (0..MAX_SPARSE_EMBEDDING_SIZE + 1).map(|_| 1.0).collect();
        let result = arrays_to_sparse_embedding(indices, values);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("exceeds maximum allowed size"));
    }

    #[test]
    fn test_round_trip_conversion() {
        let original = vec![(0, 1.0), (42, 2.5), (1000, 0.1), (u32::MAX, 3.0)];
        let (indices, values) = sparse_embedding_to_arrays(&original).unwrap();
        let recovered = arrays_to_sparse_embedding(indices, values).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_round_trip_conversion_empty() {
        let original: Vec<(u32, f32)> = vec![];
        let (indices, values) = sparse_embedding_to_arrays(&original).unwrap();
        let recovered = arrays_to_sparse_embedding(indices, values).unwrap();
        assert_eq!(original, recovered);
    }
}
