mod client;
pub mod mock;

use async_trait::async_trait;
use codesearch_core::entities::CodeEntity;
use codesearch_core::error::Result;
use uuid::Uuid;

// Re-export client types
pub use client::{
    EmbeddingCacheEntry, EntityOutboxBatchEntry, OutboxEntry, OutboxOperation, PostgresClient,
    TargetStore,
};

/// BM25 statistics for a repository
#[derive(Debug, Clone)]
pub struct BM25Statistics {
    pub avgdl: f32,
    pub total_tokens: i64,
    pub entity_count: i64,
}

/// Trait for PostgreSQL metadata operations
#[async_trait]
pub trait PostgresClientTrait: Send + Sync {
    /// Get the maximum entity batch size for batch operations
    fn max_entity_batch_size(&self) -> usize;

    /// Get direct access to the connection pool for custom queries
    ///
    /// This is used by the outbox processor for bulk operations that
    /// don't fit the standard trait methods.
    fn get_pool(&self) -> &sqlx::PgPool;

    /// Run database migrations
    async fn run_migrations(&self) -> Result<()>;

    /// Ensure repository exists in the database, creating it if necessary
    ///
    /// This method inserts a new repository record or returns the existing repository_id
    /// if a repository with the given path and collection_name already exists.
    ///
    /// The repository_id is computed deterministically from the repository_path using
    /// `StorageConfig::generate_repository_id()`, ensuring stable IDs across re-indexing.
    ///
    /// # Parameters
    ///
    /// * `repository_path` - Absolute filesystem path to the repository
    /// * `collection_name` - Unique Qdrant collection name for this repository
    /// * `repository_name` - Optional human-readable name (defaults to last path component)
    ///
    /// # Returns
    ///
    /// The UUID of the repository (newly created or existing)
    async fn ensure_repository(
        &self,
        repository_path: &std::path::Path,
        collection_name: &str,
        repository_name: Option<&str>,
    ) -> Result<Uuid>;

    /// Get repository by collection name
    async fn get_repository_id(&self, collection_name: &str) -> Result<Option<Uuid>>;

    /// Get collection name by repository ID
    async fn get_collection_name(&self, repository_id: Uuid) -> Result<Option<String>>;

    /// Get Neo4j database name by repository ID
    async fn get_neo4j_database_name(&self, repository_id: Uuid) -> Result<Option<String>>;

    /// Set Neo4j database name for a repository
    async fn set_neo4j_database_name(&self, repository_id: Uuid, db_name: &str) -> Result<()>;

    /// Set graph_ready flag for repository
    async fn set_graph_ready(&self, repository_id: Uuid, ready: bool) -> Result<()>;

    /// Check if graph is ready for repository
    async fn is_graph_ready(&self, repository_id: Uuid) -> Result<bool>;

    /// Get repository information by collection name
    ///
    /// Looks up a repository by its Qdrant collection name and returns full metadata.
    ///
    /// # Parameters
    ///
    /// * `collection_name` - The Qdrant collection name to search for
    ///
    /// # Returns
    ///
    /// * `Some((repository_id, repository_path, repository_name))` if found
    /// * `None` if no repository with this collection name exists
    async fn get_repository_by_collection(
        &self,
        collection_name: &str,
    ) -> Result<Option<(Uuid, std::path::PathBuf, String)>>;

    /// Get repository information by filesystem path
    ///
    /// Looks up a repository by its absolute filesystem path.
    /// Path comparison is exact (no canonicalization performed).
    ///
    /// # Parameters
    ///
    /// * `repository_path` - The absolute filesystem path to search for
    ///
    /// # Returns
    ///
    /// * `Some((repository_id, collection_name))` if found
    /// * `None` if no repository with this path exists
    async fn get_repository_by_path(
        &self,
        repository_path: &std::path::Path,
    ) -> Result<Option<(Uuid, String)>>;

    /// List all repositories in the database
    ///
    /// Returns metadata for all indexed repositories, sorted by creation time (oldest first).
    /// This is used by the multi-repository serve command to load all available repositories.
    ///
    /// # Returns
    ///
    /// A vector of `(repository_id, collection_name, repository_path)` tuples
    async fn list_all_repositories(&self) -> Result<Vec<(Uuid, String, std::path::PathBuf)>>;

    /// Delete a single repository and all its associated data
    ///
    /// Uses cascading deletes to automatically remove:
    /// - entity_metadata (via FK to repositories)
    /// - file_entity_snapshots (via FK to repositories)
    /// - entity_outbox (via FK to entity_metadata)
    /// - entity_embeddings (via FK to repositories)
    ///
    /// # Parameters
    ///
    /// * `repository_id` - The UUID of the repository to delete
    ///
    /// # Returns
    ///
    /// Returns Ok(()) on success, Error if repository doesn't exist or deletion fails
    async fn drop_repository(&self, repository_id: Uuid) -> Result<()>;

    /// Get BM25 statistics for a repository
    ///
    /// Returns the current average document length (avgdl) and related statistics
    /// used for BM25 sparse embedding generation.
    ///
    /// # Parameters
    ///
    /// * `repository_id` - The repository UUID
    ///
    /// # Returns
    ///
    /// BM25Statistics containing avgdl, total_tokens, and entity_count.
    /// If statistics are not yet calculated, returns default values (avgdl=50.0).
    async fn get_bm25_statistics(&self, repository_id: Uuid) -> Result<BM25Statistics>;

    /// Get BM25 statistics for a repository within a transaction
    ///
    /// Similar to get_bm25_statistics but operates within an existing transaction
    /// and uses FOR UPDATE to lock the row. This is used by the outbox processor
    /// to ensure atomic reads and updates without additional round trips.
    ///
    /// # Parameters
    ///
    /// * `tx` - The active transaction
    /// * `repository_id` - The repository UUID
    ///
    /// # Returns
    ///
    /// BM25Statistics containing avgdl, total_tokens, and entity_count
    async fn get_bm25_statistics_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        repository_id: Uuid,
    ) -> Result<BM25Statistics>;

    /// Get BM25 statistics for multiple repositories in a single query
    ///
    /// Optimized batch version for fetching statistics for many repositories at once.
    /// This reduces database round trips when loading statistics for multiple repositories,
    /// such as when the MCP server initializes all repositories at startup.
    ///
    /// # Parameters
    ///
    /// * `repository_ids` - Slice of repository UUIDs to fetch statistics for
    ///
    /// # Returns
    ///
    /// HashMap mapping repository_id to BM25Statistics
    async fn get_bm25_statistics_batch(
        &self,
        repository_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, BM25Statistics>>;

    /// Update BM25 statistics incrementally after adding new entities (within transaction)
    ///
    /// Updates the running average document length by incorporating token counts
    /// from newly added entities within an existing transaction. This is used by
    /// the outbox processor to maintain atomicity.
    ///
    /// # Parameters
    ///
    /// * `tx` - The active transaction
    /// * `repository_id` - The repository UUID
    /// * `new_token_counts` - Token counts for newly added entities
    ///
    /// # Returns
    ///
    /// The updated average document length (avgdl)
    async fn update_bm25_statistics_incremental_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        repository_id: Uuid,
        new_token_counts: &[usize],
    ) -> Result<f32>;

    /// Update BM25 statistics incrementally after adding new entities
    ///
    /// Updates the running average document length by incorporating token counts
    /// from newly added entities. This avoids full repository rescans.
    ///
    /// # Parameters
    ///
    /// * `repository_id` - The repository UUID
    /// * `new_token_counts` - Token counts for newly added entities
    ///
    /// # Returns
    ///
    /// The updated average document length (avgdl)
    async fn update_bm25_statistics_incremental(
        &self,
        repository_id: Uuid,
        new_token_counts: &[usize],
    ) -> Result<f32>;

    /// Update BM25 statistics after deleting entities
    ///
    /// Updates the running average by subtracting token counts from deleted entities.
    /// Call this after fetching token counts via `get_entity_token_counts`.
    ///
    /// # Parameters
    ///
    /// * `repository_id` - The repository UUID
    /// * `deleted_token_counts` - Token counts for deleted entities
    ///
    /// # Returns
    ///
    /// The updated average document length (avgdl)
    async fn update_bm25_statistics_after_deletion(
        &self,
        repository_id: Uuid,
        deleted_token_counts: &[usize],
    ) -> Result<f32>;

    /// Get token counts for entities (needed before deletion/update)
    ///
    /// Fetches the stored token counts for specified entities.
    /// This is used to accurately update avgdl when deleting or modifying entities.
    ///
    /// # Parameters
    ///
    /// * `entity_refs` - Slice of (repository_id, entity_id) pairs
    ///
    /// # Returns
    ///
    /// Vector of token counts in the same order as entity_refs.
    /// Entities not found or with NULL token counts are omitted.
    async fn get_entity_token_counts(&self, entity_refs: &[(Uuid, String)]) -> Result<Vec<usize>>;

    /// Batch fetch entity metadata (qdrant_point_id and deleted_at) for multiple entities
    ///
    /// Returns a HashMap mapping entity_id to (qdrant_point_id, deleted_at).
    /// Entities not found in the database will not be present in the map.
    ///
    /// # Parameters
    ///
    /// * `repository_id` - The repository UUID
    /// * `entity_ids` - Slice of entity IDs to fetch (max 1000)
    ///
    /// # Performance
    ///
    /// This method fetches all metadata in a single database query, avoiding the N+1 query problem.
    ///
    /// # Errors
    ///
    /// Returns an error if `entity_ids.len()` exceeds the maximum batch size of 1000 entities.
    async fn get_entities_metadata_batch(
        &self,
        repository_id: Uuid,
        entity_ids: &[String],
    ) -> Result<std::collections::HashMap<String, (Uuid, Option<chrono::DateTime<chrono::Utc>>)>>;

    /// Get file snapshot (list of entity IDs in file)
    async fn get_file_snapshot(
        &self,
        repository_id: Uuid,
        file_path: &str,
    ) -> Result<Option<Vec<String>>>;

    /// Update file snapshot with current entity IDs
    async fn update_file_snapshot(
        &self,
        repository_id: Uuid,
        file_path: &str,
        entity_ids: Vec<String>,
        git_commit_hash: Option<String>,
    ) -> Result<()>;

    /// Batch fetch file snapshots for multiple files
    ///
    /// Returns a HashMap mapping (repository_id, file_path) to entity_ids.
    /// Files not found in the database will not be present in the map.
    async fn get_file_snapshots_batch(
        &self,
        file_refs: &[(Uuid, String)],
    ) -> Result<std::collections::HashMap<(Uuid, String), Vec<String>>>;

    /// Batch update file snapshots in a single transaction
    ///
    /// Updates snapshots for multiple files atomically.
    /// Maximum batch size is 1000 files.
    async fn update_file_snapshots_batch(
        &self,
        repository_id: Uuid,
        updates: &[(String, Vec<String>, Option<String>)], // (file_path, entity_ids, git_commit)
    ) -> Result<()>;

    /// Batch fetch entities by (repository_id, entity_id) pairs
    ///
    /// Maximum batch size is 1000 entity references.
    async fn get_entities_by_ids(&self, entity_refs: &[(Uuid, String)]) -> Result<Vec<CodeEntity>>;

    /// Get all entities of a specific type in a repository
    ///
    /// Returns all non-deleted entities matching the specified EntityType.
    /// This is used during graph relationship resolution.
    async fn get_entities_by_type(
        &self,
        repository_id: Uuid,
        entity_type: codesearch_core::entities::EntityType,
    ) -> Result<Vec<CodeEntity>>;

    /// Get all type entities (structs, enums, classes, interfaces, type aliases) in a repository
    ///
    /// Returns all non-deleted entities that represent types.
    /// This is used during USES relationship resolution.
    async fn get_all_type_entities(&self, repository_id: Uuid) -> Result<Vec<CodeEntity>>;

    /// Mark entities as deleted and create outbox entries in a single transaction
    ///
    /// Maximum batch size is 1000 entity IDs.
    ///
    /// Token counts must be provided for BM25 statistics updates. The outbox processor
    /// will use these counts to update repository statistics after successful deletion from Qdrant.
    async fn mark_entities_deleted_with_outbox(
        &self,
        repository_id: Uuid,
        collection_name: &str,
        entity_ids: &[String],
        token_counts: &[usize],
    ) -> Result<()>;

    /// Store entities with outbox entries in a single transaction (batch operation)
    ///
    /// Maximum batch size is 1000 entities.
    async fn store_entities_with_outbox_batch(
        &self,
        repository_id: Uuid,
        collection_name: &str,
        entities: &[EntityOutboxBatchEntry<'_>],
    ) -> Result<Vec<Uuid>>;

    /// Get unprocessed outbox entries for a target store
    async fn get_unprocessed_outbox_entries(
        &self,
        target_store: TargetStore,
        limit: i64,
    ) -> Result<Vec<OutboxEntry>>;

    /// Mark outbox entry as processed
    async fn mark_outbox_processed(&self, outbox_id: Uuid) -> Result<()>;

    /// Increment retry count and record error
    async fn record_outbox_failure(&self, outbox_id: Uuid, error: &str) -> Result<()>;

    /// Get the last indexed commit for a repository
    async fn get_last_indexed_commit(&self, repository_id: Uuid) -> Result<Option<String>>;

    /// Set the last indexed commit for a repository
    async fn set_last_indexed_commit(&self, repository_id: Uuid, commit_hash: &str) -> Result<()>;

    /// Drop all data from all tables (destructive operation)
    ///
    /// Truncates all tables in the database, removing all data while preserving the schema.
    /// This is a destructive operation that cannot be undone.
    async fn drop_all_data(&self) -> Result<()>;

    /// Get embeddings by content hashes, returning embedding_id, dense_embedding, and sparse_embedding
    ///
    /// Returns a HashMap mapping content_hash to (embedding_id, dense_embedding, sparse_embedding).
    /// This is used during indexing to check if embeddings already exist.
    async fn get_embeddings_by_content_hash(
        &self,
        repository_id: Uuid,
        content_hashes: &[String],
        model_version: &str,
    ) -> Result<std::collections::HashMap<String, (i64, Vec<f32>, Option<Vec<(u32, f32)>>)>>;

    /// Store embeddings in entity_embeddings table, returning their IDs
    ///
    /// Inserts embeddings with repository-aware deduplication (ON CONFLICT DO NOTHING on (repository_id, content_hash)).
    /// Returns the embedding_id for each entry (either newly inserted or existing).
    async fn store_embeddings(
        &self,
        repository_id: Uuid,
        cache_entries: &[EmbeddingCacheEntry],
        model_version: &str,
        dimension: usize,
    ) -> Result<Vec<i64>>;

    /// Get an embedding by its ID (used by outbox processor)
    async fn get_embedding_by_id(&self, embedding_id: i64) -> Result<Option<Vec<f32>>>;

    /// Fetch both dense and sparse embeddings by ID from entity_embeddings table
    async fn get_embedding_with_sparse_by_id(
        &self,
        embedding_id: i64,
    ) -> Result<Option<(Vec<f32>, Option<Vec<(u32, f32)>>)>>;

    /// Get entity embeddings statistics (total entries, size, etc.)
    async fn get_cache_stats(&self) -> Result<crate::CacheStats>;

    /// Clear entity embeddings entries (optional: filter by model_version)
    async fn clear_cache(&self, model_version: Option<&str>) -> Result<u64>;
}
