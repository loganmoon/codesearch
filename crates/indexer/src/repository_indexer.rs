//! Repository indexer implementation
//!
//! Provides the main three-stage indexing pipeline for processing repositories.

use crate::common::find_files;
use crate::entity_processor;
use crate::{IndexResult, IndexStats};
use async_trait::async_trait;
use codesearch_core::error::{Error, Result};
use codesearch_embeddings::EmbeddingManager;

use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, error, info};

/// Progress tracking for indexing operations (internal)
#[derive(Debug, Clone)]
struct IndexProgress {
    #[allow(dead_code)]
    pub total_files: usize,
    pub processed_files: usize,
    pub failed_files: usize,
    pub current_file: Option<String>,
}

impl IndexProgress {
    fn new(total_files: usize) -> Self {
        Self {
            total_files,
            processed_files: 0,
            failed_files: 0,
            current_file: None,
        }
    }

    fn update(&mut self, file: &str, success: bool) {
        self.current_file = Some(file.to_string());
        if success {
            self.processed_files += 1;
        } else {
            self.failed_files += 1;
        }
    }
}

/// Main repository indexer
pub struct RepositoryIndexer {
    repository_path: PathBuf,
    repository_id: String,
    embedding_manager: std::sync::Arc<EmbeddingManager>,
    postgres_client: std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
    git_repo: Option<codesearch_watcher::GitRepository>,
}

impl RepositoryIndexer {
    /// Create a new repository indexer
    pub fn new(
        repository_path: PathBuf,
        repository_id: String,
        embedding_manager: std::sync::Arc<EmbeddingManager>,
        postgres_client: std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
        git_repo: Option<codesearch_watcher::GitRepository>,
    ) -> Self {
        Self {
            repository_path,
            repository_id,
            embedding_manager,
            postgres_client,
            git_repo,
        }
    }

    /// Get the repository path
    pub fn repository_path(&self) -> &Path {
        &self.repository_path
    }

    /// Process a batch of files for better performance
    async fn process_batch(
        &mut self,
        file_paths: &[PathBuf],
        _pb: &indicatif::ProgressBar,
    ) -> Result<IndexStats> {
        debug!("Processing batch of {} files", file_paths.len());

        // Process statistics
        let mut stats = IndexStats::default();

        // Collect all entities from the batch
        let mut batch_entities = Vec::new();

        // Track all processed file paths for stale entity detection
        let mut processed_files: Vec<PathBuf> = Vec::new();

        // Extract entities from all files
        for file_path in file_paths {
            match entity_processor::extract_entities_from_file(file_path, &self.repository_id).await
            {
                Ok(entities) => {
                    let entity_count = entities.len();
                    batch_entities.extend(entities);
                    stats.set_entities_extracted(stats.entities_extracted() + entity_count);
                    processed_files.push(file_path.clone());
                }
                Err(e) => {
                    error!("Failed to extract from {}: {}", file_path.display(), e);
                    stats.increment_failed_files();
                }
            }
        }

        // Get repository_id and git_commit for all files
        let repository_id = uuid::Uuid::parse_str(&self.repository_id)
            .map_err(|e| Error::Storage(format!("Invalid repository ID: {e}")))?;
        let git_commit = self.current_git_commit().await.ok();

        // Process entities with embeddings using shared logic
        let (batch_stats, entities_by_file) = entity_processor::process_entity_batch(
            batch_entities,
            repository_id,
            git_commit.clone(),
            &self.embedding_manager,
            self.postgres_client.as_ref(),
        )
        .await?;

        stats.entities_skipped_size = batch_stats.entities_skipped_size;

        // Detect and handle stale entities for ALL processed files (even empty ones)
        for file_path in processed_files {
            let file_path_str = file_path
                .to_str()
                .ok_or_else(|| Error::Storage("Invalid file path".to_string()))?;
            let entity_ids = entities_by_file
                .get(file_path_str)
                .cloned()
                .unwrap_or_default();

            entity_processor::update_file_snapshot_and_mark_stale(
                repository_id,
                file_path_str,
                entity_ids,
                git_commit.clone(),
                self.postgres_client.as_ref(),
            )
            .await?;
        }

        Ok(stats)
    }

    /// Get current Git commit hash
    async fn current_git_commit(&self) -> Result<String> {
        if let Some(git) = &self.git_repo {
            git.current_commit_hash()
                .map_err(|e| Error::Storage(format!("Failed to get Git commit: {e}")))
        } else {
            Ok("no-git".to_string())
        }
    }
}

#[async_trait]
impl crate::Indexer for RepositoryIndexer {
    /// Index the entire repository
    async fn index_repository(&mut self) -> Result<IndexResult> {
        info!("Starting repository indexing: {:?}", self.repository_path);
        let start_time = Instant::now();

        // Find all files to process
        let files = find_files(&self.repository_path)?;
        info!("Found {} files to process", files.len());

        // Create progress tracking
        let mut progress = IndexProgress::new(files.len());
        let pb = create_progress_bar(files.len());

        // Process statistics
        let mut stats = IndexStats::new();

        // Process files in batches for better performance
        const BATCH_SIZE: usize = 100; // Configurable batch size

        for chunk in files.chunks(BATCH_SIZE) {
            pb.set_message(format!("Processing batch of {} files", chunk.len()));

            match self.process_batch(chunk, &pb).await {
                Ok(batch_stats) => {
                    stats.merge(batch_stats);
                    for file_path in chunk {
                        progress.update(&file_path.to_string_lossy(), true);
                        pb.inc(1);
                    }
                }
                Err(e) => {
                    error!("Failed to process batch: {}", e);
                    // Process failed batch files individually as fallback (batch size of 1)
                    for file_path in chunk {
                        match self
                            .process_batch(std::slice::from_ref(file_path), &pb)
                            .await
                        {
                            Ok(file_stats) => {
                                stats.merge(file_stats);
                                progress.update(&file_path.to_string_lossy(), true);
                            }
                            Err(e) => {
                                error!("Failed to process file {:?}: {}", file_path, e);
                                stats.increment_failed_files();
                                progress.update(&file_path.to_string_lossy(), false);
                            }
                        }
                        pb.inc(1);
                    }
                }
            }
        }

        pb.finish_with_message("Indexing complete");

        // Calculate final statistics
        stats.set_total_files(files.len());
        stats.set_processing_time_ms(start_time.elapsed().as_millis() as u64);

        info!(
            "Indexing complete: {} files, {} entities, {} relationships in {:.2}s",
            stats.total_files(),
            stats.entities_extracted(),
            stats.relationships_extracted(),
            stats.processing_time_ms() as f64 / 1000.0
        );

        Ok(IndexResult::new(stats, Vec::new()))
    }
}

/// Create a progress bar for indexing operations
fn create_progress_bar(total: usize) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}")
            .map_err(|e| error!("Failed to set progress bar style: {}", e))
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("##-"),
    );
    pb
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::expect_used)]
mod tests {
    use crate::entity_processor;
    use codesearch_core::entities::{
        EntityMetadata, EntityType, Language, SourceLocation, Visibility,
    };
    use codesearch_core::CodeEntity;
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
            content: Some(format!("fn {name}() {{}}")),
            metadata: EntityMetadata::default(),
        }
    }

    #[tokio::test]
    async fn test_handle_file_change_detects_stale_entities() {
        let repo_uuid = Uuid::new_v4();
        let repo_id = repo_uuid.to_string();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

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
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // Verify entity2 was marked as deleted
        let entity2_meta = postgres
            .get_entity_metadata(repo_uuid, "entity2")
            .await
            .unwrap();
        assert!(entity2_meta.unwrap().1.is_some()); // deleted_at is Some

        let entity1_meta = postgres
            .get_entity_metadata(repo_uuid, "entity1")
            .await
            .unwrap();
        assert!(entity1_meta.unwrap().1.is_none()); // deleted_at is None

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
        let repo_uuid = Uuid::new_v4();
        let repo_id = repo_uuid.to_string();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

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
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // Old entity should be marked deleted
        let old_entity_meta = postgres
            .get_entity_metadata(repo_uuid, "entity_old_name")
            .await
            .unwrap();
        assert!(old_entity_meta.unwrap().1.is_some()); // deleted_at is Some
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
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // No entities should be marked as deleted
        let entity1_meta = postgres
            .get_entity_metadata(repo_uuid, "entity1")
            .await
            .unwrap();
        assert!(entity1_meta.unwrap().1.is_none()); // deleted_at is None

        let entity2_meta = postgres
            .get_entity_metadata(repo_uuid, "entity2")
            .await
            .unwrap();
        assert!(entity2_meta.unwrap().1.is_none()); // deleted_at is None

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
        let repo_uuid = Uuid::new_v4();
        let repo_id = repo_uuid.to_string();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

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
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // All entities should be marked as deleted
        let entity1_meta = postgres
            .get_entity_metadata(repo_uuid, "entity1")
            .await
            .unwrap();
        assert!(entity1_meta.unwrap().1.is_some()); // deleted_at is Some

        let entity2_meta = postgres
            .get_entity_metadata(repo_uuid, "entity2")
            .await
            .unwrap();
        assert!(entity2_meta.unwrap().1.is_some()); // deleted_at is Some

        let entity3_meta = postgres
            .get_entity_metadata(repo_uuid, "entity3")
            .await
            .unwrap();
        assert!(entity3_meta.unwrap().1.is_some()); // deleted_at is Some

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
        let repo_uuid = Uuid::new_v4();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let file_path = "test.rs";

        // Setup with entities
        let old_entities = vec!["stale_entity".to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // Remove entity
        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
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
}
