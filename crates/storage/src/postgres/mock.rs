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

/// Type alias for cached embedding entry: (embedding_id, dense, sparse)
type CachedEmbedding = (i64, Vec<f32>, Option<Vec<(u32, f32)>>);

/// Type alias for embedding data: (dense, sparse)
type EmbeddingData = (Vec<f32>, Option<Vec<(u32, f32)>>);

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
    embedding_id: Option<i64>,
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
            collection_name: "mock_collection".to_string(), // Mock uses a placeholder
            embedding_id: entry.embedding_id,
        }
    }
}

#[derive(Debug, Default)]
struct MockData {
    repositories: HashMap<Uuid, (String, String, String)>, // (repository_id -> (path, name, collection))
    collection_to_repo: HashMap<String, Uuid>,             // collection_name -> repository_id
    neo4j_databases: HashMap<Uuid, String>,                // repository_id -> neo4j_database_name
    entities: HashMap<(Uuid, String), EntityMetadata>,     // (repository_id, entity_id) -> metadata
    snapshots: HashMap<(Uuid, String), (Vec<String>, Option<String>)>, // (repo_id, file_path) -> (entity_ids, git_commit)
    outbox: Vec<MockOutboxEntry>,
    embedding_cache: HashMap<String, CachedEmbedding>, // content_hash -> (embedding_id, dense, sparse)
    embedding_by_id: HashMap<i64, EmbeddingData>,      // embedding_id -> (dense, sparse)
    embedding_id_counter: i64,                         // Auto-increment counter for embedding IDs
}

/// Mock PostgreSQL client for testing
pub struct MockPostgresClient {
    data: Arc<Mutex<MockData>>,
    max_entity_batch_size: usize,
}

impl MockPostgresClient {
    /// Create a new mock client
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(MockData::default())),
            max_entity_batch_size: 10000, // Default for tests
        }
    }

    /// Get number of entities stored
    #[cfg(test)]
    pub fn entity_count(&self) -> usize {
        self.data.lock().unwrap().entities.len()
    }

    /// Get number of non-deleted entities
    #[cfg(test)]
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
    #[cfg(test)]
    pub fn snapshot_count(&self) -> usize {
        self.data.lock().unwrap().snapshots.len()
    }

    /// Get number of outbox entries
    #[cfg(test)]
    pub fn outbox_count(&self) -> usize {
        self.data.lock().unwrap().outbox.len()
    }

    /// Get number of unprocessed outbox entries
    #[cfg(test)]
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
    #[cfg(test)]
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
    #[cfg(test)]
    pub fn get_snapshot_sync(&self, repository_id: Uuid, file_path: &str) -> Option<Vec<String>> {
        self.data
            .lock()
            .unwrap()
            .snapshots
            .get(&(repository_id, file_path.to_string()))
            .map(|(ids, _)| ids.clone())
    }

    /// Clear all data (for test cleanup)
    #[cfg(test)]
    pub fn clear(&self) {
        let mut data = self.data.lock().unwrap();
        data.repositories.clear();
        data.collection_to_repo.clear();
        data.entities.clear();
        data.snapshots.clear();
        data.outbox.clear();
        data.embedding_cache.clear();
        data.embedding_by_id.clear();
        data.embedding_id_counter = 0;
    }
}

impl Default for MockPostgresClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PostgresClientTrait for MockPostgresClient {
    fn max_entity_batch_size(&self) -> usize {
        self.max_entity_batch_size
    }

    fn get_pool(&self) -> &sqlx::PgPool {
        // Mock doesn't have a real pool - this should not be called in tests
        // If you need to test code that uses get_pool(), use integration tests with a real database
        panic!("MockPostgresClient::get_pool() should not be called - use integration tests with a real database")
    }

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

        // Generate deterministic repository ID from path
        let repository_id =
            codesearch_core::config::StorageConfig::generate_repository_id(repository_path)?;

        // Create new repository with the generated deterministic UUID
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

    async fn get_collection_name(&self, repository_id: Uuid) -> Result<Option<String>> {
        let data = self.data.lock().unwrap();
        Ok(data
            .repositories
            .get(&repository_id)
            .map(|(_, _, collection_name)| collection_name.clone()))
    }

    async fn get_neo4j_database_name(&self, repository_id: Uuid) -> Result<Option<String>> {
        let data = self.data.lock().unwrap();
        Ok(data.neo4j_databases.get(&repository_id).cloned())
    }

    async fn set_neo4j_database_name(&self, repository_id: Uuid, db_name: &str) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        data.neo4j_databases
            .insert(repository_id, db_name.to_string());
        Ok(())
    }

    async fn set_graph_ready(&self, _repository_id: Uuid, _ready: bool) -> Result<()> {
        // Mock - not implemented
        Ok(())
    }

    async fn is_graph_ready(&self, _repository_id: Uuid) -> Result<bool> {
        // Mock - always return true
        Ok(true)
    }

    async fn set_pending_relationship_resolution(
        &self,
        _repository_id: Uuid,
        _pending: bool,
    ) -> Result<()> {
        // Mock - no-op
        Ok(())
    }

    async fn has_pending_relationship_resolution(&self, _repository_id: Uuid) -> Result<bool> {
        // Mock - always return false (no pending resolution)
        Ok(false)
    }

    async fn get_repositories_with_pending_resolution(&self) -> Result<Vec<Uuid>> {
        // Mock - return empty list
        Ok(Vec::new())
    }

    async fn get_repository_by_collection(
        &self,
        collection_name: &str,
    ) -> Result<Option<(Uuid, std::path::PathBuf, String)>> {
        let data = self.data.lock().unwrap();

        if let Some(repo_id) = data.collection_to_repo.get(collection_name) {
            if let Some((path, name, _)) = data.repositories.get(repo_id) {
                return Ok(Some((
                    *repo_id,
                    std::path::PathBuf::from(path),
                    name.clone(),
                )));
            }
        }

        Ok(None)
    }

    async fn get_repository_by_path(
        &self,
        repository_path: &std::path::Path,
    ) -> Result<Option<(Uuid, String)>> {
        let data = self.data.lock().unwrap();

        let path_str = repository_path
            .to_str()
            .ok_or_else(|| codesearch_core::error::Error::storage("Invalid path"))?;

        for (repo_id, (stored_path, _, collection_name)) in data.repositories.iter() {
            if stored_path == path_str {
                return Ok(Some((*repo_id, collection_name.clone())));
            }
        }

        Ok(None)
    }

    async fn list_all_repositories(&self) -> Result<Vec<(Uuid, String, std::path::PathBuf)>> {
        let data = self.data.lock().unwrap();

        Ok(data
            .repositories
            .iter()
            .map(|(repo_id, (path, _, collection_name))| {
                (
                    *repo_id,
                    collection_name.clone(),
                    std::path::PathBuf::from(path),
                )
            })
            .collect())
    }

    async fn drop_repository(&self, repository_id: Uuid) -> Result<()> {
        let mut data = self.data.lock().unwrap();

        // Check if repository exists
        let repo_data = data.repositories.get(&repository_id).ok_or_else(|| {
            codesearch_core::error::Error::storage(format!("Repository {repository_id} not found"))
        })?;

        let collection_name = repo_data.2.clone();

        // Remove repository from maps
        data.repositories.remove(&repository_id);
        data.collection_to_repo.remove(&collection_name);

        // Remove all entities for this repository
        data.entities
            .retain(|(repo_id, _), _| *repo_id != repository_id);

        // Remove all snapshots for this repository
        data.snapshots
            .retain(|(repo_id, _), _| *repo_id != repository_id);

        // Remove all outbox entries for this repository
        data.outbox
            .retain(|entry| entry.repository_id != repository_id);

        tracing::info!("Deleted repository {repository_id} and all associated data");

        Ok(())
    }

    async fn get_bm25_statistics(&self, _repository_id: Uuid) -> Result<super::BM25Statistics> {
        Ok(super::BM25Statistics {
            avgdl: 50.0,
            total_tokens: 0,
            entity_count: 0,
        })
    }

    async fn get_bm25_statistics_in_tx(
        &self,
        _tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        _repository_id: Uuid,
    ) -> Result<super::BM25Statistics> {
        Ok(super::BM25Statistics {
            avgdl: 50.0,
            total_tokens: 0,
            entity_count: 0,
        })
    }

    async fn get_bm25_statistics_batch(
        &self,
        repository_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, super::BM25Statistics>> {
        let mut result = std::collections::HashMap::new();
        for &repo_id in repository_ids {
            result.insert(
                repo_id,
                super::BM25Statistics {
                    avgdl: 50.0,
                    total_tokens: 0,
                    entity_count: 0,
                },
            );
        }
        Ok(result)
    }

    async fn update_bm25_statistics_incremental_in_tx(
        &self,
        _tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        _repository_id: Uuid,
        _new_token_counts: &[usize],
    ) -> Result<f32> {
        Ok(50.0)
    }

    async fn update_bm25_statistics_incremental(
        &self,
        _repository_id: Uuid,
        _new_token_counts: &[usize],
    ) -> Result<f32> {
        Ok(50.0)
    }

    async fn update_bm25_statistics_after_deletion(
        &self,
        _repository_id: Uuid,
        _deleted_token_counts: &[usize],
    ) -> Result<f32> {
        Ok(50.0)
    }

    async fn get_entity_token_counts(&self, _entity_refs: &[(Uuid, String)]) -> Result<Vec<usize>> {
        Ok(vec![])
    }

    async fn get_entities_metadata_batch(
        &self,
        repository_id: Uuid,
        entity_ids: &[String],
    ) -> Result<std::collections::HashMap<String, (Uuid, Option<chrono::DateTime<chrono::Utc>>)>>
    {
        let data = self.data.lock().unwrap();

        let mut result = std::collections::HashMap::new();
        for entity_id in entity_ids {
            if let Some(metadata) = data.entities.get(&(repository_id, entity_id.clone())) {
                result.insert(
                    entity_id.clone(),
                    (metadata.qdrant_point_id, metadata.deleted_at),
                );
            }
        }

        Ok(result)
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

    async fn get_file_snapshots_batch(
        &self,
        file_refs: &[(Uuid, String)],
    ) -> Result<std::collections::HashMap<(Uuid, String), Vec<String>>> {
        let data = self.data.lock().unwrap();

        let mut result = std::collections::HashMap::new();
        for (repo_id, file_path) in file_refs {
            if let Some((entity_ids, _)) = data.snapshots.get(&(*repo_id, file_path.clone())) {
                result.insert((*repo_id, file_path.clone()), entity_ids.clone());
            }
        }

        Ok(result)
    }

    async fn update_file_snapshots_batch(
        &self,
        repository_id: Uuid,
        updates: &[(String, Vec<String>, Option<String>)],
    ) -> Result<()> {
        let mut data = self.data.lock().unwrap();

        for (file_path, entity_ids, git_commit) in updates {
            data.snapshots.insert(
                (repository_id, file_path.clone()),
                (entity_ids.clone(), git_commit.clone()),
            );
        }

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

    async fn get_entities_by_type(
        &self,
        repository_id: Uuid,
        entity_type: codesearch_core::entities::EntityType,
    ) -> Result<Vec<CodeEntity>> {
        let data = self.data.lock().unwrap();

        let entities: Vec<CodeEntity> = data
            .entities
            .iter()
            .filter_map(|((repo_id, _entity_id), metadata)| {
                if *repo_id == repository_id
                    && metadata.deleted_at.is_none()
                    && metadata.entity.entity_type == entity_type
                {
                    Some(metadata.entity.clone())
                } else {
                    None
                }
            })
            .collect();

        Ok(entities)
    }

    async fn get_all_type_entities(&self, repository_id: Uuid) -> Result<Vec<CodeEntity>> {
        use codesearch_core::entities::EntityType;
        let data = self.data.lock().unwrap();

        let entities: Vec<CodeEntity> = data
            .entities
            .iter()
            .filter_map(|((repo_id, _entity_id), metadata)| {
                if *repo_id == repository_id && metadata.deleted_at.is_none() {
                    match metadata.entity.entity_type {
                        EntityType::Struct
                        | EntityType::Enum
                        | EntityType::Class
                        | EntityType::Interface
                        | EntityType::TypeAlias => Some(metadata.entity.clone()),
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .collect();

        Ok(entities)
    }

    async fn search_entities_fulltext(
        &self,
        repository_id: Uuid,
        query: &str,
        limit: i64,
    ) -> Result<Vec<CodeEntity>> {
        let data = self.data.lock().unwrap();
        let query_lower = query.to_lowercase();

        let entities: Vec<CodeEntity> = data
            .entities
            .iter()
            .filter_map(|((repo_id, _entity_id), metadata)| {
                if *repo_id == repository_id && metadata.deleted_at.is_none() {
                    if let Some(content) = &metadata.entity.content {
                        if content.to_lowercase().contains(&query_lower) {
                            return Some(metadata.entity.clone());
                        }
                    }
                }
                None
            })
            .take(limit as usize)
            .collect();

        Ok(entities)
    }

    async fn mark_entities_deleted_with_outbox(
        &self,
        repository_id: Uuid,
        _collection_name: &str,
        entity_ids: &[String],
        _token_counts: &[usize],
    ) -> Result<()> {
        if entity_ids.is_empty() {
            return Ok(());
        }

        if entity_ids.len() > self.max_entity_batch_size {
            return Err(codesearch_core::error::Error::storage(format!(
                "Batch size {} exceeds maximum allowed size of {}",
                entity_ids.len(),
                self.max_entity_batch_size
            )));
        }

        let mut data = self.data.lock().unwrap();
        let now = chrono::Utc::now();

        // Mark entities as deleted and track which ones actually existed
        let mut deleted_entity_ids = Vec::new();
        for entity_id in entity_ids {
            if let Some(metadata) = data.entities.get_mut(&(repository_id, entity_id.clone())) {
                metadata.deleted_at = Some(now);
                deleted_entity_ids.push(entity_id.clone());
            }
        }

        // Create outbox entries only for entities that actually existed and were deleted
        for entity_id in deleted_entity_ids {
            let payload = serde_json::json!({
                "entity_ids": [&entity_id],
                "reason": "file_change"
            });

            data.outbox.push(MockOutboxEntry {
                outbox_id: Uuid::new_v4(),
                repository_id,
                entity_id,
                operation: OutboxOperation::Delete,
                target_store: TargetStore::Qdrant,
                payload,
                created_at: now,
                processed_at: None,
                retry_count: 0,
                last_error: None,
                embedding_id: None,
            });
        }

        Ok(())
    }

    async fn store_entities_with_outbox_batch(
        &self,
        repository_id: Uuid,
        _collection_name: &str,
        entities: &[EntityOutboxBatchEntry<'_>],
    ) -> Result<Vec<Uuid>> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }

        if entities.len() > self.max_entity_batch_size {
            return Err(codesearch_core::error::Error::storage(format!(
                "Batch size {} exceeds maximum allowed size of {}",
                entities.len(),
                self.max_entity_batch_size
            )));
        }

        let mut data = self.data.lock().unwrap();
        let mut outbox_ids = Vec::with_capacity(entities.len());
        let now = chrono::Utc::now();

        for (
            entity,
            embedding_id,
            operation,
            point_id,
            target_store,
            git_commit_hash,
            _token_count,
        ) in entities
        {
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
                "qdrant_point_id": point_id.to_string(),
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
                embedding_id: Some(*embedding_id),
            });

            outbox_ids.push(outbox_id);
        }

        Ok(outbox_ids)
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

    async fn count_pending_outbox_entries(&self) -> Result<i64> {
        let data = self.data.lock().unwrap();
        let count = data
            .outbox
            .iter()
            .filter(|e| e.processed_at.is_none())
            .count();
        Ok(count as i64)
    }

    async fn get_last_indexed_commit(&self, _repository_id: Uuid) -> Result<Option<String>> {
        // Mock implementation: return None (not tracking commits in mock)
        Ok(None)
    }

    async fn set_last_indexed_commit(
        &self,
        _repository_id: Uuid,
        _commit_hash: &str,
    ) -> Result<()> {
        // Mock implementation: do nothing
        Ok(())
    }

    async fn drop_all_data(&self) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        data.repositories.clear();
        data.collection_to_repo.clear();
        data.entities.clear();
        data.snapshots.clear();
        data.outbox.clear();
        data.embedding_cache.clear();
        data.embedding_by_id.clear();
        data.embedding_id_counter = 0;
        Ok(())
    }

    async fn get_embeddings_by_content_hash(
        &self,
        _repository_id: Uuid,
        content_hashes: &[String],
        _model_version: &str,
    ) -> Result<std::collections::HashMap<String, (i64, Vec<f32>, Option<Vec<(u32, f32)>>)>> {
        let data = self.data.lock().unwrap();
        let mut result = std::collections::HashMap::new();

        for hash in content_hashes {
            if let Some((embedding_id, embedding, sparse)) = data.embedding_cache.get(hash) {
                result.insert(
                    hash.clone(),
                    (*embedding_id, embedding.clone(), sparse.clone()),
                );
            }
        }

        Ok(result)
    }

    async fn store_embeddings(
        &self,
        _repository_id: Uuid,
        cache_entries: &[super::EmbeddingCacheEntry],
        _model_version: &str,
        _dimension: usize,
    ) -> Result<Vec<i64>> {
        let mut data = self.data.lock().unwrap();
        let mut embedding_ids = Vec::with_capacity(cache_entries.len());

        for (hash, embedding, sparse) in cache_entries {
            // Check if this content_hash already exists (deduplication)
            let embedding_id = if let Some((existing_id, _, _)) = data.embedding_cache.get(hash) {
                *existing_id
            } else {
                // Generate new auto-increment ID
                data.embedding_id_counter += 1;
                let new_id = data.embedding_id_counter;

                // Store in both maps with sparse embeddings
                data.embedding_cache
                    .insert(hash.clone(), (new_id, embedding.clone(), sparse.clone()));
                data.embedding_by_id
                    .insert(new_id, (embedding.clone(), sparse.clone()));

                new_id
            };

            embedding_ids.push(embedding_id);
        }

        Ok(embedding_ids)
    }

    async fn get_embedding_by_id(&self, embedding_id: i64) -> Result<Option<Vec<f32>>> {
        let data = self.data.lock().unwrap();
        Ok(data
            .embedding_by_id
            .get(&embedding_id)
            .map(|(dense, _sparse)| dense.clone()))
    }

    async fn get_embedding_with_sparse_by_id(
        &self,
        embedding_id: i64,
    ) -> Result<Option<(Vec<f32>, Option<Vec<(u32, f32)>>)>> {
        let data = self.data.lock().unwrap();
        Ok(data.embedding_by_id.get(&embedding_id).cloned())
    }

    async fn get_embeddings_with_sparse_by_ids(
        &self,
        embedding_ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, (Vec<f32>, Option<Vec<(u32, f32)>>)>> {
        let data = self.data.lock().unwrap();
        let mut result = std::collections::HashMap::with_capacity(embedding_ids.len());
        for &id in embedding_ids {
            if let Some(embedding) = data.embedding_by_id.get(&id) {
                result.insert(id, embedding.clone());
            }
        }
        Ok(result)
    }

    async fn get_cache_stats(&self) -> Result<crate::CacheStats> {
        let data = self.data.lock().unwrap();
        Ok(crate::CacheStats {
            total_entries: data.embedding_cache.len() as i64,
            total_size_bytes: 0, // Not tracked in mock
            entries_by_model: std::collections::HashMap::new(),
            oldest_entry: None,
            newest_entry: None,
        })
    }

    async fn clear_cache(&self, _model_version: Option<&str>) -> Result<u64> {
        let mut data = self.data.lock().unwrap();
        let count = data.embedding_cache.len() as u64;
        data.embedding_cache.clear();
        data.embedding_by_id.clear();
        data.embedding_id_counter = 0;
        Ok(count)
    }

    async fn get_embeddings_by_qualified_names(
        &self,
        repository_id: Uuid,
        qualified_names: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<f32>>> {
        if qualified_names.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let data = self.data.lock().unwrap();
        let mut result = std::collections::HashMap::new();

        for ((repo_id, _entity_id), metadata) in &data.entities {
            if *repo_id == repository_id
                && metadata.deleted_at.is_none()
                && qualified_names.contains(&metadata.entity.qualified_name)
            {
                // For the mock, we'll return a dummy embedding if we find the entity
                // In real usage, this would need to join with embedding_id
                let dummy_embedding = vec![0.1; 768]; // BGE-style embedding dimension
                result.insert(metadata.entity.qualified_name.clone(), dummy_embedding);
            }
        }

        Ok(result)
    }

    async fn get_entities_by_qualified_names(
        &self,
        repository_id: Uuid,
        qualified_names: &[String],
    ) -> Result<std::collections::HashMap<String, CodeEntity>> {
        if qualified_names.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let data = self.data.lock().unwrap();
        let mut result = std::collections::HashMap::new();

        for ((repo_id, _entity_id), metadata) in &data.entities {
            if *repo_id == repository_id
                && metadata.deleted_at.is_none()
                && qualified_names.contains(&metadata.entity.qualified_name)
            {
                result.insert(
                    metadata.entity.qualified_name.clone(),
                    metadata.entity.clone(),
                );
            }
        }

        Ok(result)
    }

    async fn insert_pending_relationships(
        &self,
        _repository_id: Uuid,
        _relationships: &[(String, String, String)],
    ) -> Result<u64> {
        // Mock - not implemented (would need additional mock data structure)
        Ok(0)
    }

    async fn resolve_pending_relationships(
        &self,
        _repository_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<(i64, String, String, String)>> {
        // Mock - return empty (no pending relationships in mock)
        Ok(Vec::new())
    }

    async fn delete_pending_relationships(&self, _pending_ids: &[i64]) -> Result<()> {
        // Mock - no-op
        Ok(())
    }

    async fn count_pending_relationships(&self, _repository_id: Uuid) -> Result<i64> {
        // Mock - return 0
        Ok(0)
    }
}

// Test helper methods (not part of the trait)
impl MockPostgresClient {
    /// Store entity metadata (for tests only)
    pub async fn store_entity_metadata(
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
                deleted_at: None,
            },
        );

        Ok(())
    }

    /// Get all entity IDs for a file path (for tests only)
    pub async fn get_entities_for_file(&self, file_path: &str) -> Result<Vec<String>> {
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

    /// Write outbox entry (for tests only)
    pub async fn write_outbox_entry(
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
            embedding_id: None,
        });

        Ok(outbox_id)
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

        // Mark as deleted (using batch method with single item)
        client
            .mark_entities_deleted_with_outbox(
                repo_id,
                "test_collection",
                std::slice::from_ref(&entity.entity_id),
                &[42], // token count
            )
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

        // Mark as deleted (using batch method with single item)
        client
            .mark_entities_deleted_with_outbox(
                repo_id,
                "test_collection",
                std::slice::from_ref(&entity.entity_id),
                &[42], // token count
            )
            .await
            .unwrap();

        assert_eq!(client.entity_count(), 1);
        assert_eq!(client.active_entity_count(), 0);
    }
}
