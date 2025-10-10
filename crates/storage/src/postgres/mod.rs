mod client;
pub mod mock;

use async_trait::async_trait;
use codesearch_core::entities::CodeEntity;
use codesearch_core::error::Result;
use uuid::Uuid;

pub(crate) use client::{EntityOutboxBatchEntry, PostgresClient};

// Re-export types needed externally
pub use client::{OutboxEntry, OutboxOperation, TargetStore};

/// Trait for PostgreSQL metadata operations
#[async_trait]
pub trait PostgresClientTrait: Send + Sync {
    /// Run database migrations
    async fn run_migrations(&self) -> Result<()>;

    /// Ensure repository exists, return repository_id
    async fn ensure_repository(
        &self,
        repository_path: &std::path::Path,
        collection_name: &str,
        repository_name: Option<&str>,
    ) -> Result<Uuid>;

    /// Get repository by collection name
    async fn get_repository_id(&self, collection_name: &str) -> Result<Option<Uuid>>;

    /// Get entity metadata (qdrant_point_id and deleted_at) by entity_id
    async fn get_entity_metadata(
        &self,
        repository_id: Uuid,
        entity_id: &str,
    ) -> Result<Option<(Uuid, Option<chrono::DateTime<chrono::Utc>>)>>;

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

    /// Batch fetch entities by (repository_id, entity_id) pairs
    ///
    /// Maximum batch size is 1000 entity references.
    async fn get_entities_by_ids(&self, entity_refs: &[(Uuid, String)]) -> Result<Vec<CodeEntity>>;

    /// Mark entities as deleted (soft delete)
    ///
    /// Maximum batch size is 1000 entity IDs.
    async fn mark_entities_deleted(&self, repository_id: Uuid, entity_ids: &[String])
        -> Result<()>;

    /// Mark entities as deleted and create outbox entries in a single transaction
    ///
    /// Maximum batch size is 1000 entity IDs.
    async fn mark_entities_deleted_with_outbox(
        &self,
        repository_id: Uuid,
        entity_ids: &[String],
    ) -> Result<()>;

    /// Store entities with outbox entries in a single transaction (batch operation)
    ///
    /// Maximum batch size is 1000 entities.
    async fn store_entities_with_outbox_batch(
        &self,
        repository_id: Uuid,
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
}
