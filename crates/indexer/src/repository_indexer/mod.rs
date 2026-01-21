//! Repository indexer implementation
//!
//! Provides the main pipelined indexing pipeline for processing repositories.

mod batches;
mod stages;

use crate::common::get_current_commit;
use crate::config::IndexerConfig;
use crate::{IndexResult, IndexStats};
use anyhow::anyhow;
use async_trait::async_trait;
use batches::{EmbeddedBatch, EntityBatch, FileBatch, StoredBatch};
use codesearch_core::error::{Error, Result};
use codesearch_core::project_manifest::{detect_manifest, PackageMap};
use stages::{
    stage_extract_entities, stage_file_discovery, stage_generate_embeddings, stage_store_entities,
    stage_update_snapshots,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Main repository indexer
pub struct RepositoryIndexer {
    repository_path: PathBuf,
    repository_id: uuid::Uuid,
    embedding_manager: std::sync::Arc<codesearch_embeddings::EmbeddingManager>,
    /// Pre-initialized sparse embedding manager (optional - falls back to lazy creation if None)
    sparse_manager: Option<Arc<codesearch_embeddings::SparseEmbeddingManager>>,
    postgres_client: std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
    git_repo: Option<codesearch_watcher::GitRepository>,
    config: IndexerConfig,
    /// Package manifest for qualified name derivation
    package_map: Option<Arc<PackageMap>>,
}

impl RepositoryIndexer {
    /// Create a new repository indexer
    ///
    /// # Arguments
    /// * `repository_path` - Path to the repository root
    /// * `repository_id` - UUID string identifying the repository
    /// * `embedding_manager` - Manager for generating dense embeddings
    /// * `sparse_manager` - Optional pre-initialized sparse embedding manager (for Granite).
    ///   If None, falls back to creating sparse manager lazily (required for BM25 which needs avgdl).
    /// * `postgres_client` - PostgreSQL client for storage operations
    /// * `git_repo` - Optional Git repository handle
    /// * `config` - Indexer configuration
    pub fn new(
        repository_path: PathBuf,
        repository_id: String,
        embedding_manager: std::sync::Arc<codesearch_embeddings::EmbeddingManager>,
        sparse_manager: Option<Arc<codesearch_embeddings::SparseEmbeddingManager>>,
        postgres_client: std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
        git_repo: Option<codesearch_watcher::GitRepository>,
        config: IndexerConfig,
    ) -> Result<Self> {
        debug!(
            "RepositoryIndexer::new called with repository_id string = {}",
            repository_id
        );
        let repository_id = uuid::Uuid::parse_str(&repository_id)
            .map_err(|e| Error::Storage(format!("Invalid repository ID: {e}")))?;

        debug!("RepositoryIndexer::new parsed UUID = {}", repository_id);

        // Detect project manifest for qualified name derivation
        let package_map = match detect_manifest(&repository_path) {
            Ok(Some(manifest)) => {
                info!(
                    "Detected {:?} project with {} package(s)",
                    manifest.project_type,
                    manifest.packages.len()
                );
                Some(Arc::new(manifest.packages))
            }
            Ok(None) => {
                debug!("No project manifest detected");
                None
            }
            Err(e) => {
                warn!("Failed to detect project manifest: {e}");
                None
            }
        };

        Ok(Self {
            repository_path,
            repository_id,
            embedding_manager,
            sparse_manager,
            postgres_client,
            git_repo,
            config,
            package_map,
        })
    }

    /// Get the repository path
    pub fn repository_path(&self) -> &Path {
        &self.repository_path
    }
}

#[async_trait]
impl crate::Indexer for RepositoryIndexer {
    /// Index the entire repository using a pipelined architecture
    async fn index_repository(&mut self) -> Result<IndexResult> {
        let start_time = Instant::now();
        let config = &self.config;

        info!(
            repository_path = %self.repository_path.display(),
            "Starting pipelined repository indexing with config: \
             index_batch_size={}, max_entity_batch_size={}, channel_buffer_size={}, \
             file_extraction_concurrency={}, snapshot_update_concurrency={}",
            config.index_batch_size,
            config.max_entity_batch_size,
            config.channel_buffer_size,
            config.file_extraction_concurrency,
            config.snapshot_update_concurrency
        );

        // Create channels with configurable buffer sizes
        let (file_tx, file_rx) = mpsc::channel::<FileBatch>(config.channel_buffer_size);
        let (entity_tx, entity_rx) = mpsc::channel::<EntityBatch>(config.channel_buffer_size);
        let (embedded_tx, embedded_rx) = mpsc::channel::<EmbeddedBatch>(config.channel_buffer_size);
        let (stored_tx, stored_rx) = mpsc::channel::<StoredBatch>(config.channel_buffer_size);

        // Clone shared state for each stage
        let repo_path = self.repository_path.clone();
        let repo_id = self.repository_id;
        let git_repo = self.git_repo.clone();
        let git_commit = get_current_commit(git_repo.as_ref(), &repo_path);
        let embedding_manager = self.embedding_manager.clone();
        let postgres_client = self.postgres_client.clone();
        let postgres_client_2 = self.postgres_client.clone();

        // Fetch collection_name once for entire pipeline
        let collection_name = postgres_client
            .get_collection_name(repo_id)
            .await
            .map_err(|e| Error::Other(anyhow!("Failed to get collection name: {e}")))?
            .ok_or_else(|| Error::Other(anyhow!("Repository not found for repo_id {repo_id}")))?;

        // Spawn all 5 stages concurrently
        let repo_path_for_stage2 = repo_path.clone();
        let stage1 = tokio::spawn(stage_file_discovery(
            file_tx,
            repo_path,
            config.index_batch_size,
        ));

        let package_map = self.package_map.clone();
        let stage2 = tokio::spawn(stage_extract_entities(
            file_rx,
            entity_tx,
            repo_id,
            git_commit.clone(),
            collection_name.clone(),
            config.max_entity_batch_size,
            config.file_extraction_concurrency,
            package_map,
            repo_path_for_stage2,
        ));

        let postgres_client_3 = self.postgres_client.clone();
        let sparse_embeddings_config = self.config.sparse_embeddings.clone();
        let sparse_manager = self.sparse_manager.clone();
        let stage3 = tokio::spawn(stage_generate_embeddings(
            entity_rx,
            embedded_tx,
            embedding_manager,
            postgres_client_3,
            sparse_embeddings_config,
            sparse_manager,
        ));

        let stage4 = tokio::spawn(stage_store_entities(
            embedded_rx,
            stored_tx,
            postgres_client,
        ));

        let stage5 = tokio::spawn(stage_update_snapshots(
            stored_rx,
            postgres_client_2,
            config.snapshot_update_concurrency,
        ));

        // Await all stages and handle errors
        let stage1_result = stage1
            .await
            .map_err(|e| Error::Other(anyhow!("Stage 1 panicked: {e}")))?;
        let stage2_result = stage2
            .await
            .map_err(|e| Error::Other(anyhow!("Stage 2 panicked: {e}")))?;
        let stage3_result = stage3
            .await
            .map_err(|e| Error::Other(anyhow!("Stage 3 panicked: {e}")))?;
        let stage4_result = stage4
            .await
            .map_err(|e| Error::Other(anyhow!("Stage 4 panicked: {e}")))?;
        let stage5_result = stage5
            .await
            .map_err(|e| Error::Other(anyhow!("Stage 5 panicked: {e}")))?;

        // Aggregate results
        let total_files = stage1_result?;
        let (entities_extracted, failed_files) = stage2_result?;
        let _entities_embedded = stage3_result?;
        let _entities_stored = stage4_result?;
        let _snapshots_updated = stage5_result?;

        // Build final statistics
        let mut stats = IndexStats::new();
        stats.set_total_files(total_files);
        stats.set_entities_extracted(entities_extracted);
        stats.set_processing_time_ms(start_time.elapsed().as_millis() as u64);

        // Track failed files from extraction stage
        for _ in 0..failed_files {
            stats.increment_failed_files();
        }

        // Note: entities_skipped_size is tracked internally by embedding stage
        // but not aggregated to final stats in pipelined version (logged instead)

        // Set last indexed commit
        let commit_hash = git_commit.unwrap_or_else(|| "indexed".to_string());
        self.postgres_client
            .set_last_indexed_commit(self.repository_id, &commit_hash)
            .await?;
        info!(commit = %commit_hash, "Updated last indexed commit");

        let total_time = start_time.elapsed();
        let throughput = if total_time.as_secs_f64() > 0.0 {
            entities_extracted as f64 / total_time.as_secs_f64()
        } else {
            0.0
        };

        info!(
            total_files = stats.total_files(),
            entities_extracted = stats.entities_extracted(),
            processing_time_s = stats.processing_time_ms() as f64 / 1000.0,
            failed_files = stats.failed_files(),
            throughput_entities_per_sec = format!("{throughput:.1}"),
            "Pipeline completed"
        );

        // No granular errors tracked in pipelined version (all logged during processing)
        Ok(IndexResult::new(stats, Vec::new()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::expect_used)]
mod tests {
    use crate::entity_processor;
    use codesearch_core::entities::{
        EntityMetadata, EntityType, Language, SourceLocation, Visibility,
    };
    use codesearch_core::{CodeEntity, QualifiedName};
    use codesearch_storage::MockPostgresClient;
    use codesearch_storage::PostgresClientTrait;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn create_test_entity(
        name: &str,
        entity_id: &str,
        file_path: &str,
        repo_id: &str,
    ) -> CodeEntity {
        CodeEntity {
            entity_id: entity_id.to_string(),
            repository_id: repo_id.to_string(),
            name: name.to_string(),
            qualified_name: QualifiedName::parse(name).expect("Invalid qn"),
            path_entity_identifier: None,
            entity_type: EntityType::Function,
            language: Language::Rust,
            file_path: PathBuf::from(file_path),
            location: SourceLocation {
                start_line: 1,
                end_line: 10,
                start_column: 0,
                end_column: 10,
            },
            visibility: Some(Visibility::Public),
            parent_scope: None,
            signature: None,
            documentation_summary: None,
            content: Some(format!("fn {name}() {{}}")),
            metadata: EntityMetadata::default(),
            relationships: Default::default(),
        }
    }

    #[tokio::test]
    async fn test_handle_file_change_detects_stale_entities() {
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        // Register repository with mock and get the repo UUID
        let repo_uuid = postgres
            .ensure_repository(std::path::Path::new("/test/repo"), "test_collection", None)
            .await
            .unwrap();
        let repo_id = repo_uuid.to_string();

        let file_path = "test.rs";

        // Setup: store entities in mock database
        let entity1 = create_test_entity("entity1", "entity1", file_path, &repo_id);
        let entity2 = create_test_entity("entity2", "entity2", file_path, &repo_id);
        postgres
            .store_entity_metadata(repo_uuid, &entity1, None, Uuid::new_v4())
            .await
            .unwrap();
        postgres
            .store_entity_metadata(repo_uuid, &entity2, None, Uuid::new_v4())
            .await
            .unwrap();

        // Setup: previous snapshot had two entities
        let old_entities = vec!["entity1".to_string(), "entity2".to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // New state: only entity1 remains
        let new_entities = vec!["entity1".to_string()];

        // Run update_file_snapshot_and_mark_stale
        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // Verify entity2 was marked as deleted
        let entity_ids = vec!["entity2".to_string(), "entity1".to_string()];
        let metadata_map = postgres
            .get_entities_metadata_batch(repo_uuid, &entity_ids)
            .await
            .unwrap();

        let entity2_meta = metadata_map.get("entity2").unwrap();
        assert!(entity2_meta.1.is_some()); // deleted_at is Some

        let entity1_meta = metadata_map.get("entity1").unwrap();
        assert!(entity1_meta.1.is_none()); // deleted_at is None

        // Verify snapshot was updated
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap();
        assert_eq!(snapshot, Some(new_entities));

        // Verify DELETE outbox entry was created
        use codesearch_storage::TargetStore;
        let entries = postgres
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn test_handle_file_change_detects_renamed_function() {
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        // Register repository with mock and get the repo UUID
        let repo_uuid = postgres
            .ensure_repository(std::path::Path::new("/test/repo"), "test_collection", None)
            .await
            .unwrap();
        let repo_id = repo_uuid.to_string();

        let file_path = "test.rs";

        // Setup: store old entity
        let old_entity = create_test_entity("old_name", "entity_old_name", file_path, &repo_id);
        postgres
            .store_entity_metadata(repo_uuid, &old_entity, None, Uuid::new_v4())
            .await
            .unwrap();

        // Old snapshot: function named "old_name"
        let old_entities = vec!["entity_old_name".to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // New state: function renamed to "new_name" (different entity ID)
        let new_entities = vec!["entity_new_name".to_string()];

        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // Old entity should be marked deleted
        let entity_ids = vec!["entity_old_name".to_string()];
        let metadata_map = postgres
            .get_entities_metadata_batch(repo_uuid, &entity_ids)
            .await
            .unwrap();
        let old_entity_meta = metadata_map.get("entity_old_name").unwrap();
        assert!(old_entity_meta.1.is_some()); // deleted_at is Some
    }

    #[tokio::test]
    async fn test_handle_file_change_handles_added_entities() {
        let repo_uuid = Uuid::new_v4();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let file_path = "test.rs";

        // Old snapshot: one entity
        let old_entities = vec!["entity1".to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // New state: added entity2
        let new_entities = vec!["entity1".to_string(), "entity2".to_string()];

        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // Snapshot should be updated
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap();
        assert_eq!(snapshot, Some(new_entities));

        // No DELETE outbox entries
        use codesearch_storage::TargetStore;
        let entries = postgres
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await
            .unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[tokio::test]
    async fn test_handle_file_change_empty_file() {
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        // Register repository with mock and get the repo UUID
        let repo_uuid = postgres
            .ensure_repository(std::path::Path::new("/test/repo"), "test_collection", None)
            .await
            .unwrap();
        let repo_id = repo_uuid.to_string();

        let file_path = "test.rs";

        // Setup: store entities
        for i in 1..=3 {
            let entity = create_test_entity(
                &format!("entity{i}"),
                &format!("entity{i}"),
                file_path,
                &repo_id,
            );
            postgres
                .store_entity_metadata(repo_uuid, &entity, None, Uuid::new_v4())
                .await
                .unwrap();
        }

        // Old snapshot: three entities
        let old_entities = vec![
            "entity1".to_string(),
            "entity2".to_string(),
            "entity3".to_string(),
        ];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // New state: file is now empty (all entities removed)
        let new_entities = vec![];

        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // All entities should be marked as deleted
        let entity_ids = vec![
            "entity1".to_string(),
            "entity2".to_string(),
            "entity3".to_string(),
        ];
        let metadata_map = postgres
            .get_entities_metadata_batch(repo_uuid, &entity_ids)
            .await
            .unwrap();

        let entity1_meta = metadata_map.get("entity1").unwrap();
        assert!(entity1_meta.1.is_some()); // deleted_at is Some

        let entity2_meta = metadata_map.get("entity2").unwrap();
        assert!(entity2_meta.1.is_some()); // deleted_at is Some

        let entity3_meta = metadata_map.get("entity3").unwrap();
        assert!(entity3_meta.1.is_some()); // deleted_at is Some

        // Should have 3 DELETE outbox entries
        use codesearch_storage::TargetStore;
        let entries = postgres
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await
            .unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[tokio::test]
    async fn test_handle_file_change_no_previous_snapshot() {
        let repo_uuid = Uuid::new_v4();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let file_path = "test.rs";

        // No previous snapshot
        let new_entities = vec!["entity1".to_string(), "entity2".to_string()];

        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // No entities should be deleted (first time indexing)
        use codesearch_storage::TargetStore;
        let entries = postgres
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await
            .unwrap();
        assert_eq!(entries.len(), 0);

        // Snapshot should be created
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap();
        assert_eq!(snapshot, Some(new_entities));
    }

    #[tokio::test]
    async fn test_handle_file_change_no_changes() {
        let repo_uuid = Uuid::new_v4();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let file_path = "test.rs";

        // Old snapshot
        let entities = vec!["entity1".to_string(), "entity2".to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, entities.clone(), None)
            .await
            .unwrap();

        // Re-index with same entities
        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // No entities deleted
        use codesearch_storage::TargetStore;
        let entries = postgres
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await
            .unwrap();
        assert_eq!(entries.len(), 0);

        // Snapshot still updated (for git commit tracking)
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap();
        assert_eq!(snapshot, Some(entities));
    }

    #[tokio::test]
    async fn test_handle_file_change_writes_delete_to_outbox() {
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        // Register repository with mock and get the repo UUID
        let repo_uuid = postgres
            .ensure_repository(std::path::Path::new("/test/repo"), "test_collection", None)
            .await
            .unwrap();

        let file_path = "test.rs";

        // Setup with entities - store entity in metadata first
        let old_entity_id = "stale_entity";
        let old_entity =
            create_test_entity("stale_fn", old_entity_id, file_path, &repo_uuid.to_string());
        postgres
            .store_entity_metadata(repo_uuid, &old_entity, None, uuid::Uuid::new_v4())
            .await
            .unwrap();

        let old_entities = vec![old_entity_id.to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // Remove entity
        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            vec![],
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // Verify outbox entry
        let entries = postgres
            .get_unprocessed_outbox_entries(codesearch_storage::TargetStore::Qdrant, 10)
            .await
            .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entity_id, "stale_entity");
        assert_eq!(entries[0].operation, "DELETE");
        assert_eq!(entries[0].target_store, "qdrant");

        // Verify payload contains reason
        let payload = &entries[0].payload;
        assert_eq!(payload["reason"], "file_change");
        assert!(payload["entity_ids"].is_array());
    }

    #[tokio::test]
    async fn test_handle_file_change_updates_snapshot_with_git_commit() {
        let repo_uuid = Uuid::new_v4();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let file_path = "test.rs";
        let git_commit = Some("abc123".to_string());
        let new_entities = vec!["entity1".to_string()];

        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            new_entities.clone(),
            git_commit.clone(),
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // Snapshot should be stored with git commit
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap()
            .expect("Snapshot should exist");
        assert_eq!(snapshot, new_entities);
    }

    mod create_crate_root_tests {
        use super::super::stages::create_crate_root_entities;
        use codesearch_core::project_manifest::{PackageInfo, PackageMap};
        use std::path::PathBuf;
        use tempfile::TempDir;

        fn create_package(name: &str, source_root: PathBuf) -> (PathBuf, PackageInfo) {
            let pkg_dir = source_root.parent().unwrap_or(&source_root).to_path_buf();
            (
                pkg_dir,
                PackageInfo {
                    name: name.to_string(),
                    source_root,
                    crates: Vec::new(), // Empty for fallback file-based discovery
                },
            )
        }

        #[test]
        fn test_create_crate_root_with_lib_rs() {
            let temp_dir = TempDir::new().unwrap();
            let src_dir = temp_dir.path().join("src");
            std::fs::create_dir_all(&src_dir).unwrap();
            std::fs::write(src_dir.join("lib.rs"), "// lib").unwrap();

            let mut package_map = PackageMap::new();
            let (pkg_dir, info) = create_package("my_crate", src_dir);
            package_map.add(pkg_dir, info);

            let entities = create_crate_root_entities(&package_map, "test-repo-id");

            assert_eq!(entities.len(), 1);
            let entity = &entities[0];
            assert_eq!(entity.name, "my_crate");
            assert_eq!(entity.qualified_name.to_string(), "my_crate");
            assert!(entity.file_path.ends_with("lib.rs"));
        }

        #[test]
        fn test_create_crate_root_with_main_rs() {
            let temp_dir = TempDir::new().unwrap();
            let src_dir = temp_dir.path().join("src");
            std::fs::create_dir_all(&src_dir).unwrap();
            std::fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();

            let mut package_map = PackageMap::new();
            let (pkg_dir, info) = create_package("my_binary", src_dir);
            package_map.add(pkg_dir, info);

            let entities = create_crate_root_entities(&package_map, "test-repo-id");

            assert_eq!(entities.len(), 1);
            let entity = &entities[0];
            assert_eq!(entity.name, "my_binary");
            assert!(entity.file_path.ends_with("main.rs"));
        }

        #[test]
        fn test_create_crate_root_prefers_lib_over_main() {
            let temp_dir = TempDir::new().unwrap();
            let src_dir = temp_dir.path().join("src");
            std::fs::create_dir_all(&src_dir).unwrap();
            std::fs::write(src_dir.join("lib.rs"), "// lib").unwrap();
            std::fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();

            let mut package_map = PackageMap::new();
            let (pkg_dir, info) = create_package("dual_crate", src_dir);
            package_map.add(pkg_dir, info);

            let entities = create_crate_root_entities(&package_map, "test-repo-id");

            assert_eq!(entities.len(), 1);
            let entity = &entities[0];
            // lib.rs should be preferred over main.rs
            assert!(entity.file_path.ends_with("lib.rs"));
        }

        #[test]
        fn test_create_crate_root_missing_entry_point() {
            let temp_dir = TempDir::new().unwrap();
            let src_dir = temp_dir.path().join("src");
            std::fs::create_dir_all(&src_dir).unwrap();
            // No lib.rs or main.rs

            let mut package_map = PackageMap::new();
            let (pkg_dir, info) = create_package("empty_crate", src_dir);
            package_map.add(pkg_dir, info);

            let entities = create_crate_root_entities(&package_map, "test-repo-id");

            // Should return empty vec, not panic
            assert!(entities.is_empty());
        }

        #[test]
        fn test_create_crate_root_multiple_packages() {
            let temp_dir = TempDir::new().unwrap();

            // Create first package with lib.rs
            let src1 = temp_dir.path().join("crate1/src");
            std::fs::create_dir_all(&src1).unwrap();
            std::fs::write(src1.join("lib.rs"), "// lib1").unwrap();

            // Create second package with main.rs
            let src2 = temp_dir.path().join("crate2/src");
            std::fs::create_dir_all(&src2).unwrap();
            std::fs::write(src2.join("main.rs"), "fn main() {}").unwrap();

            let mut package_map = PackageMap::new();
            let (pkg1, info1) = create_package("crate1", src1);
            let (pkg2, info2) = create_package("crate2", src2);
            package_map.add(pkg1, info1);
            package_map.add(pkg2, info2);

            let entities = create_crate_root_entities(&package_map, "test-repo-id");

            assert_eq!(entities.len(), 2);
            let names: Vec<_> = entities.iter().map(|e| e.name.as_str()).collect();
            assert!(names.contains(&"crate1"));
            assert!(names.contains(&"crate2"));
        }
    }
}
