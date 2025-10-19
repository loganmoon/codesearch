mod client;
pub mod mock;

use async_trait::async_trait;
use codesearch_core::entities::CodeEntity;
use codesearch_core::error::Result;
use uuid::Uuid;

// Re-export client types
pub use client::{
    EntityOutboxBatchEntry, OutboxEntry, OutboxOperation, PostgresClient, TargetStore,
};

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

    /// Mark entities as deleted and create outbox entries in a single transaction
    ///
    /// Maximum batch size is 1000 entity IDs.
    async fn mark_entities_deleted_with_outbox(
        &self,
        repository_id: Uuid,
        collection_name: &str,
        entity_ids: &[String],
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

    /// Get embeddings by content hashes, returning both embedding_id and embedding vector
    ///
    /// Returns a HashMap mapping content_hash to (embedding_id, embedding_vector).
    /// This is used during indexing to check if embeddings already exist.
    async fn get_embeddings_by_content_hash(
        &self,
        content_hashes: &[String],
        model_version: &str,
    ) -> Result<std::collections::HashMap<String, (i64, Vec<f32>)>>;

    /// Store embeddings in entity_embeddings table, returning their IDs
    ///
    /// Inserts embeddings with content-based deduplication (ON CONFLICT DO NOTHING on content_hash).
    /// Returns the embedding_id for each entry (either newly inserted or existing).
    async fn store_embeddings(
        &self,
        cache_entries: &[(String, Vec<f32>)],
        model_version: &str,
        dimension: usize,
    ) -> Result<Vec<i64>>;

    /// Get an embedding by its ID (used by outbox processor)
    async fn get_embedding_by_id(&self, embedding_id: i64) -> Result<Option<Vec<f32>>>;

    /// Get entity embeddings statistics (total entries, size, etc.)
    async fn get_cache_stats(&self) -> Result<crate::CacheStats>;

    /// Clear entity embeddings entries (optional: filter by model_version)
    async fn clear_cache(&self, model_version: Option<&str>) -> Result<u64>;
}
