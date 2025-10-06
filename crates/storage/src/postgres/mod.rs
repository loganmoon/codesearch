mod client;
pub mod mock;

use async_trait::async_trait;
use codesearch_core::entities::CodeEntity;
use codesearch_core::error::Result;
use uuid::Uuid;

pub use client::{
    EntityOutboxBatchEntry, OutboxEntry, OutboxOperation, PostgresClient, TargetStore,
};

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

    /// Store or update entity metadata
    async fn store_entity_metadata(
        &self,
        repository_id: Uuid,
        entity: &CodeEntity,
        git_commit_hash: Option<String>,
        qdrant_point_id: Uuid,
    ) -> Result<()>;

    /// Get all entity IDs for a file path
    async fn get_entities_for_file(&self, file_path: &str) -> Result<Vec<String>>;

    /// Get entity metadata (qdrant_point_id and deleted_at) by entity_id
    async fn get_entity_metadata(
        &self,
        repository_id: Uuid,
        entity_id: &str,
    ) -> Result<Option<(Uuid, Option<chrono::DateTime<chrono::Utc>>)>>;

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
    async fn get_entities_by_ids(&self, entity_refs: &[(Uuid, String)]) -> Result<Vec<CodeEntity>>;

    /// Mark entities as deleted (soft delete)
    async fn mark_entities_deleted(&self, repository_id: Uuid, entity_ids: &[String])
        -> Result<()>;

    /// Store entities with outbox entries in a single transaction (batch operation)
    async fn store_entities_with_outbox_batch(
        &self,
        repository_id: Uuid,
        entities: &[EntityOutboxBatchEntry<'_>],
    ) -> Result<Vec<Uuid>>;

    /// Write outbox entry for entity operation
    async fn write_outbox_entry(
        &self,
        repository_id: Uuid,
        entity_id: &str,
        operation: OutboxOperation,
        target_store: TargetStore,
        payload: serde_json::Value,
    ) -> Result<Uuid>;

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
}
