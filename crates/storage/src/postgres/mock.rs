//! Mock PostgreSQL client for testing

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use async_trait::async_trait;
use codesearch_core::entities::CodeEntity;
use codesearch_core::error::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::{
    EntityOutboxBatchEntry, OutboxEntry, OutboxOperation, PostgresClientTrait, TargetStore,
};

/// In-memory entity metadata
#[derive(Debug, Clone)]
struct EntityMetadata {
    entity: CodeEntity,
    #[allow(dead_code)]
    git_commit_hash: Option<String>,
    qdrant_point_id: Uuid,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// In-memory outbox entry
#[derive(Debug, Clone)]
struct MockOutboxEntry {
    outbox_id: Uuid,
    repository_id: Uuid,
    entity_id: String,
    operation: OutboxOperation,
    target_store: TargetStore,
    payload: serde_json::Value,
    created_at: chrono::DateTime<chrono::Utc>,
    processed_at: Option<chrono::DateTime<chrono::Utc>>,
    retry_count: i32,
    last_error: Option<String>,
}

impl From<MockOutboxEntry> for OutboxEntry {
    fn from(entry: MockOutboxEntry) -> Self {
        Self {
            outbox_id: entry.outbox_id,
            repository_id: entry.repository_id,
            entity_id: entry.entity_id,
            operation: format!("{}", entry.operation),
            target_store: format!("{}", entry.target_store),
            payload: entry.payload,
            created_at: entry.created_at,
            processed_at: entry.processed_at,
            retry_count: entry.retry_count,
            last_error: entry.last_error,
        }
    }
}

#[derive(Debug, Default)]
struct MockData {
    repositories: HashMap<Uuid, (String, String, String)>, // (repository_id -> (path, name, collection))
    collection_to_repo: HashMap<String, Uuid>,             // collection_name -> repository_id
    entities: HashMap<(Uuid, String), EntityMetadata>,     // (repository_id, entity_id) -> metadata
    snapshots: HashMap<(Uuid, String), (Vec<String>, Option<String>)>, // (repo_id, file_path) -> (entity_ids, git_commit)
    outbox: Vec<MockOutboxEntry>,
}

/// Mock PostgreSQL client for testing
pub struct MockPostgresClient {
    data: Arc<Mutex<MockData>>,
}

impl MockPostgresClient {
    /// Create a new mock client
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(MockData::default())),
        }
    }

    /// Get number of entities stored
    pub fn entity_count(&self) -> usize {
        self.data.lock().unwrap().entities.len()
    }

    /// Get number of non-deleted entities
    pub fn active_entity_count(&self) -> usize {
        self.data
            .lock()
            .unwrap()
            .entities
            .values()
            .filter(|e| e.deleted_at.is_none())
            .count()
    }

    /// Get number of snapshots stored
    pub fn snapshot_count(&self) -> usize {
        self.data.lock().unwrap().snapshots.len()
    }

    /// Get number of outbox entries
    pub fn outbox_count(&self) -> usize {
        self.data.lock().unwrap().outbox.len()
    }

    /// Get number of unprocessed outbox entries
    pub fn unprocessed_outbox_count(&self) -> usize {
        self.data
            .lock()
            .unwrap()
            .outbox
            .iter()
            .filter(|e| e.processed_at.is_none())
            .count()
    }

    /// Check if entity is marked as deleted
    pub fn is_entity_deleted(&self, repository_id: Uuid, entity_id: &str) -> bool {
        self.data
            .lock()
            .unwrap()
            .entities
            .get(&(repository_id, entity_id.to_string()))
            .map(|e| e.deleted_at.is_some())
            .unwrap_or(false)
    }

    /// Get snapshot for testing
    pub fn get_snapshot_sync(&self, repository_id: Uuid, file_path: &str) -> Option<Vec<String>> {
        self.data
            .lock()
            .unwrap()
            .snapshots
            .get(&(repository_id, file_path.to_string()))
            .map(|(ids, _)| ids.clone())
    }

    /// Clear all data (for test cleanup)
    pub fn clear(&self) {
        let mut data = self.data.lock().unwrap();
        data.repositories.clear();
        data.collection_to_repo.clear();
        data.entities.clear();
        data.snapshots.clear();
        data.outbox.clear();
    }
}

impl Default for MockPostgresClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PostgresClientTrait for MockPostgresClient {
    async fn run_migrations(&self) -> Result<()> {
        // Mock - no migrations needed
        Ok(())
    }

    async fn ensure_repository(
        &self,
        repository_path: &std::path::Path,
        collection_name: &str,
        repository_name: Option<&str>,
    ) -> Result<Uuid> {
        let mut data = self.data.lock().unwrap();

        // Check if repository exists by collection name
        if let Some(repo_id) = data.collection_to_repo.get(collection_name) {
            return Ok(*repo_id);
        }

        // Create new repository
        let repository_id = Uuid::new_v4();
        let path_str = repository_path
            .to_str()
            .ok_or_else(|| codesearch_core::error::Error::storage("Invalid path"))?
            .to_string();
        let name = repository_name
            .or_else(|| repository_path.file_name()?.to_str())
            .unwrap_or("unknown")
            .to_string();

        data.repositories
            .insert(repository_id, (path_str, name, collection_name.to_string()));
        data.collection_to_repo
            .insert(collection_name.to_string(), repository_id);

        Ok(repository_id)
    }

    async fn get_repository_id(&self, collection_name: &str) -> Result<Option<Uuid>> {
        let data = self.data.lock().unwrap();
        Ok(data.collection_to_repo.get(collection_name).copied())
    }

    async fn store_entity_metadata(
        &self,
        repository_id: Uuid,
        entity: &CodeEntity,
        git_commit_hash: Option<String>,
        qdrant_point_id: Uuid,
    ) -> Result<()> {
        let mut data = self.data.lock().unwrap();

        data.entities.insert(
            (repository_id, entity.entity_id.clone()),
            EntityMetadata {
                entity: entity.clone(),
                git_commit_hash,
                qdrant_point_id,
                deleted_at: None, // Reset deleted_at on upsert
            },
        );

        Ok(())
    }

    async fn get_entities_for_file(&self, file_path: &str) -> Result<Vec<String>> {
        let data = self.data.lock().unwrap();

        let entity_ids: Vec<String> = data
            .entities
            .iter()
            .filter(|(_, metadata)| {
                metadata.deleted_at.is_none()
                    && metadata.entity.file_path.to_str() == Some(file_path)
            })
            .map(|((_, entity_id), _)| entity_id.clone())
            .collect();

        Ok(entity_ids)
    }

    async fn get_entity_metadata(
        &self,
        repository_id: Uuid,
        entity_id: &str,
    ) -> Result<Option<(Uuid, Option<chrono::DateTime<chrono::Utc>>)>> {
        let data = self.data.lock().unwrap();

        Ok(data
            .entities
            .get(&(repository_id, entity_id.to_string()))
            .map(|metadata| (metadata.qdrant_point_id, metadata.deleted_at)))
    }

    async fn get_file_snapshot(
        &self,
        repository_id: Uuid,
        file_path: &str,
    ) -> Result<Option<Vec<String>>> {
        let data = self.data.lock().unwrap();

        Ok(data
            .snapshots
            .get(&(repository_id, file_path.to_string()))
            .map(|(ids, _)| ids.clone()))
    }

    async fn update_file_snapshot(
        &self,
        repository_id: Uuid,
        file_path: &str,
        entity_ids: Vec<String>,
        git_commit_hash: Option<String>,
    ) -> Result<()> {
        let mut data = self.data.lock().unwrap();

        data.snapshots.insert(
            (repository_id, file_path.to_string()),
            (entity_ids, git_commit_hash),
        );

        Ok(())
    }

    async fn get_entities_by_ids(&self, entity_refs: &[(Uuid, String)]) -> Result<Vec<CodeEntity>> {
        let data = self.data.lock().unwrap();

        let entities: Vec<CodeEntity> = entity_refs
            .iter()
            .filter_map(|(repo_id, entity_id)| {
                data.entities
                    .get(&(*repo_id, entity_id.clone()))
                    .filter(|metadata| metadata.deleted_at.is_none())
                    .map(|metadata| metadata.entity.clone())
            })
            .collect();

        Ok(entities)
    }

    async fn mark_entities_deleted(
        &self,
        repository_id: Uuid,
        entity_ids: &[String],
    ) -> Result<()> {
        if entity_ids.is_empty() {
            return Ok(());
        }

        const MAX_BATCH_SIZE: usize = 1000;
        if entity_ids.len() > MAX_BATCH_SIZE {
            return Err(codesearch_core::error::Error::storage(format!(
                "Batch size {} exceeds maximum allowed size of {}",
                entity_ids.len(),
                MAX_BATCH_SIZE
            )));
        }

        let mut data = self.data.lock().unwrap();
        let now = chrono::Utc::now();

        for entity_id in entity_ids {
            if let Some(metadata) = data.entities.get_mut(&(repository_id, entity_id.clone())) {
                metadata.deleted_at = Some(now);
            }
        }

        Ok(())
    }

    async fn store_entities_with_outbox_batch(
        &self,
        repository_id: Uuid,
        entities: &[EntityOutboxBatchEntry<'_>],
    ) -> Result<Vec<Uuid>> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }

        const MAX_BATCH_SIZE: usize = 1000;
        if entities.len() > MAX_BATCH_SIZE {
            return Err(codesearch_core::error::Error::storage(format!(
                "Batch size {} exceeds maximum allowed size of {}",
                entities.len(),
                MAX_BATCH_SIZE
            )));
        }

        let mut data = self.data.lock().unwrap();
        let mut outbox_ids = Vec::with_capacity(entities.len());
        let now = chrono::Utc::now();

        for (entity, embedding, operation, point_id, target_store, git_commit_hash) in entities {
            // Store entity metadata
            data.entities.insert(
                (repository_id, entity.entity_id.clone()),
                EntityMetadata {
                    entity: (*entity).clone(),
                    git_commit_hash: git_commit_hash.clone(),
                    qdrant_point_id: *point_id,
                    deleted_at: None,
                },
            );

            // Write outbox entry
            let outbox_id = Uuid::new_v4();
            let payload = serde_json::json!({
                "entity": entity,
                "embedding": embedding,
                "qdrant_point_id": point_id.to_string()
            });

            data.outbox.push(MockOutboxEntry {
                outbox_id,
                repository_id,
                entity_id: entity.entity_id.clone(),
                operation: *operation,
                target_store: *target_store,
                payload,
                created_at: now,
                processed_at: None,
                retry_count: 0,
                last_error: None,
            });

            outbox_ids.push(outbox_id);
        }

        Ok(outbox_ids)
    }

    async fn write_outbox_entry(
        &self,
        repository_id: Uuid,
        entity_id: &str,
        operation: OutboxOperation,
        target_store: TargetStore,
        payload: serde_json::Value,
    ) -> Result<Uuid> {
        let mut data = self.data.lock().unwrap();

        let outbox_id = Uuid::new_v4();
        data.outbox.push(MockOutboxEntry {
            outbox_id,
            repository_id,
            entity_id: entity_id.to_string(),
            operation,
            target_store,
            payload,
            created_at: chrono::Utc::now(),
            processed_at: None,
            retry_count: 0,
            last_error: None,
        });

        Ok(outbox_id)
    }

    async fn get_unprocessed_outbox_entries(
        &self,
        target_store: TargetStore,
        limit: i64,
    ) -> Result<Vec<OutboxEntry>> {
        let data = self.data.lock().unwrap();

        let entries: Vec<OutboxEntry> = data
            .outbox
            .iter()
            .filter(|e| {
                e.processed_at.is_none()
                    && format!("{}", e.target_store) == format!("{target_store}")
            })
            .take(limit as usize)
            .cloned()
            .map(|e| e.into())
            .collect();

        Ok(entries)
    }

    async fn mark_outbox_processed(&self, outbox_id: Uuid) -> Result<()> {
        let mut data = self.data.lock().unwrap();

        if let Some(entry) = data.outbox.iter_mut().find(|e| e.outbox_id == outbox_id) {
            entry.processed_at = Some(chrono::Utc::now());
        }

        Ok(())
    }

    async fn record_outbox_failure(&self, outbox_id: Uuid, error: &str) -> Result<()> {
        let mut data = self.data.lock().unwrap();

        if let Some(entry) = data.outbox.iter_mut().find(|e| e.outbox_id == outbox_id) {
            entry.retry_count += 1;
            entry.last_error = Some(error.to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesearch_core::entities::{EntityType, Language, SourceLocation, Visibility};
    use std::path::PathBuf;

    fn create_test_entity(name: &str, file_path: &str) -> CodeEntity {
        use codesearch_core::entities::EntityMetadata;

        CodeEntity {
            entity_id: format!("test_{name}"),
            repository_id: "test_repo".to_string(),
            name: name.to_string(),
            qualified_name: name.to_string(),
            entity_type: EntityType::Function,
            language: Language::Rust,
            file_path: PathBuf::from(file_path),
            location: SourceLocation {
                start_line: 1,
                end_line: 10,
                start_column: 0,
                end_column: 10,
            },
            visibility: Visibility::Public,
            parent_scope: None,
            dependencies: Vec::new(),
            signature: None,
            documentation_summary: None,
            content: Some("fn test() {}".to_string()),
            metadata: EntityMetadata::default(),
        }
    }

    #[tokio::test]
    async fn test_mock_ensure_repository() {
        let client = MockPostgresClient::new();

        let repo_id1 = client
            .ensure_repository(
                std::path::Path::new("/test/repo"),
                "test_collection",
                Some("test_repo"),
            )
            .await
            .unwrap();

        // Calling again with same collection should return same ID
        let repo_id2 = client
            .ensure_repository(
                std::path::Path::new("/test/repo"),
                "test_collection",
                Some("test_repo"),
            )
            .await
            .unwrap();

        assert_eq!(repo_id1, repo_id2);
    }

    #[tokio::test]
    async fn test_mock_store_and_get_snapshot() {
        let client = MockPostgresClient::new();
        let repo_id = Uuid::new_v4();
        let file_path = "test.rs";

        // Initially no snapshot
        let snapshot = client.get_file_snapshot(repo_id, file_path).await.unwrap();
        assert!(snapshot.is_none());

        // Store snapshot
        let entity_ids = vec!["entity1".to_string(), "entity2".to_string()];
        client
            .update_file_snapshot(repo_id, file_path, entity_ids.clone(), None)
            .await
            .unwrap();

        // Retrieve snapshot
        let snapshot = client
            .get_file_snapshot(repo_id, file_path)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot, entity_ids);
    }

    #[tokio::test]
    async fn test_mock_mark_entities_deleted() {
        let client = MockPostgresClient::new();
        let repo_id = Uuid::new_v4();

        // Store an entity
        let entity = create_test_entity("test_fn", "test.rs");
        client
            .store_entity_metadata(repo_id, &entity, None, Uuid::new_v4())
            .await
            .unwrap();

        assert!(!client.is_entity_deleted(repo_id, &entity.entity_id));

        // Mark as deleted
        client
            .mark_entities_deleted(repo_id, &[entity.entity_id.clone()])
            .await
            .unwrap();

        assert!(client.is_entity_deleted(repo_id, &entity.entity_id));
    }

    #[tokio::test]
    async fn test_mock_outbox_operations() {
        let client = MockPostgresClient::new();
        let repo_id = Uuid::new_v4();

        // Write outbox entry
        let outbox_id = client
            .write_outbox_entry(
                repo_id,
                "test_entity",
                OutboxOperation::Insert,
                TargetStore::Qdrant,
                serde_json::json!({"test": "data"}),
            )
            .await
            .unwrap();

        // Verify unprocessed count
        assert_eq!(client.unprocessed_outbox_count(), 1);

        // Get unprocessed entries
        let entries = client
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entity_id, "test_entity");

        // Mark as processed
        client.mark_outbox_processed(outbox_id).await.unwrap();

        assert_eq!(client.unprocessed_outbox_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_helper_methods() {
        let client = MockPostgresClient::new();
        let repo_id = Uuid::new_v4();

        assert_eq!(client.entity_count(), 0);
        assert_eq!(client.active_entity_count(), 0);

        // Add entity
        let entity = create_test_entity("test", "test.rs");
        client
            .store_entity_metadata(repo_id, &entity, None, Uuid::new_v4())
            .await
            .unwrap();

        assert_eq!(client.entity_count(), 1);
        assert_eq!(client.active_entity_count(), 1);

        // Mark as deleted
        client
            .mark_entities_deleted(repo_id, &[entity.entity_id.clone()])
            .await
            .unwrap();

        assert_eq!(client.entity_count(), 1);
        assert_eq!(client.active_entity_count(), 0);
    }
}
