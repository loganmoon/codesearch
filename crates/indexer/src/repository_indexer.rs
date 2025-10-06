//! Repository indexer implementation
//!
//! Provides the main three-stage indexing pipeline for processing repositories.

use crate::common::find_files;
use crate::{IndexResult, IndexStats};
use async_trait::async_trait;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use codesearch_embeddings::EmbeddingManager;
use codesearch_languages::create_extractor;

use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::fs;
use tracing::{debug, error, info};

const DELIM: &str = " ";

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
    postgres_client: std::sync::Arc<dyn codesearch_storage::postgres::PostgresClientTrait>,
    git_repo: Option<codesearch_watcher::GitRepository>,
}

/// Extract embeddable content from a CodeEntity
fn extract_embedding_content(entity: &CodeEntity) -> String {
    // Combine relevant fields for embedding generation
    let mut content = String::with_capacity(500);

    // Add entity name and qualified name (moved)
    content.push_str(&format!("{} {}", entity.entity_type, entity.name));
    chain_delim(&mut content, &entity.qualified_name);

    // Add documentation summary if available
    if let Some(doc) = &entity.documentation_summary {
        chain_delim(&mut content, doc);
    }

    // Add signature information for functions/methods
    if let Some(sig) = &entity.signature {
        // Format parameters as "name: type" or just "name" if no type
        let _ = sig // collect into strings
            .parameters
            .iter()
            .map(|(name, type_opt)| {
                if let Some(ty) = type_opt {
                    // format
                    format!("{name}: {ty}")
                } else {
                    name.clone()
                }
            })
            .collect::<Vec<_>>()
            .iter()
            .map(|p| chain_delim(&mut content, p))
            .collect::<Vec<_>>();

        if let Some(ret_type) = &sig.return_type {
            chain_delim(&mut content, &format!("-> {ret_type}"));
        }
    }

    // Add the full entity content (most important for semantic search)
    if let Some(entity_content) = &entity.content {
        chain_delim(&mut content, entity_content);
    }

    content
}

fn chain_delim(out_str: &mut String, text: &str) {
    out_str.push_str(DELIM);
    out_str.push_str(text);
}

impl RepositoryIndexer {
    /// Create a new repository indexer
    pub fn new(
        repository_path: PathBuf,
        repository_id: String,
        embedding_manager: std::sync::Arc<EmbeddingManager>,
        postgres_client: std::sync::Arc<dyn codesearch_storage::postgres::PostgresClientTrait>,
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
        let mut errors = Vec::new();

        // Collect all entities from the batch
        let mut batch_entities = Vec::new();

        // Process files sequentially for now to avoid borrowing issues
        let mut extraction_results = Vec::new();
        for file_path in file_paths {
            let result = self.extract_from_file(file_path).await;
            extraction_results.push(result);
        }

        // Track all processed file paths for stale entity detection
        let mut processed_files: Vec<PathBuf> = Vec::new();

        // Process each extraction result
        for (file_path, result) in file_paths.iter().zip(extraction_results) {
            match result {
                Ok((entities, file_stats)) => {
                    // Just add entities directly to batch without transformation
                    batch_entities.extend(entities);
                    stats.merge(file_stats);
                    processed_files.push(file_path.clone());
                }
                Err(e) => {
                    error!("Failed to extract from {:?}: {}", file_path, e);
                    stats.increment_failed_files();
                    errors.push(e.to_string());
                }
            }
        }

        // Get repository_id and git_commit for all files
        let repository_id = uuid::Uuid::parse_str(&self.repository_id)
            .map_err(|e| Error::Storage(format!("Invalid repository ID: {e}")))?;
        let git_commit = self.current_git_commit().await.ok();

        // Track entities by file (will be empty for files with no entities)
        let mut entities_by_file: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        // Bulk load all entities from the batch
        if !batch_entities.is_empty() {
            info!("Bulk loading {} entities", batch_entities.len());

            // Generate embeddings for all entities
            let embedding_texts: Vec<String> = batch_entities // create embedding texts from each entity
                .iter() // iterate
                .map(extract_embedding_content)
                .collect();

            let option_embeddings = self // embed all texts
                .embedding_manager // access
                .embed(embedding_texts) // call embed
                .await
                .map_err(|e| Error::Storage(format!("Failed to generate embeddings: {e}")))?;

            // Filter entities with valid embeddings
            // Pair entities with their embeddings
            let mut entity_embedding_pairs: Vec<(CodeEntity, Vec<f32>)> = Vec::new();

            for (entity, opt_embedding) in batch_entities
                .into_iter()
                .zip(option_embeddings.into_iter())
            {
                if let Some(embedding) = opt_embedding {
                    entity_embedding_pairs.push((entity, embedding));
                } else {
                    stats.entities_skipped_size += 1;
                    debug!(
                        "Skipped entity due to size: {} in {}",
                        entity.qualified_name,
                        entity.file_path.display()
                    );
                }
            }

            debug!(
                "After embedding: entity_embedding_pairs={}",
                entity_embedding_pairs.len()
            );

            // Only store entities that have embeddings
            if !entity_embedding_pairs.is_empty() {
                info!(
                    "Processing {} entities with embeddings",
                    entity_embedding_pairs.len()
                );

                // Type alias for batch data entries
                type BatchEntry = (
                    CodeEntity,
                    Vec<f32>,
                    codesearch_storage::postgres::OutboxOperation,
                    uuid::Uuid,
                    codesearch_storage::postgres::TargetStore,
                    Option<String>,
                );

                // Prepare batch data for transactional insert
                let mut batch_data: Vec<BatchEntry> = Vec::with_capacity(entity_embedding_pairs.len());

                // Check existing metadata and determine operations for each entity
                for (entity, embedding) in &entity_embedding_pairs {
                    // Check if entity already exists
                    let existing_metadata = self
                        .postgres_client
                        .get_entity_metadata(repository_id, &entity.entity_id)
                        .await
                        .map_err(|e| {
                            error!(
                                "Failed to check existing entity metadata for {}: {e}",
                                entity.entity_id
                            );
                            e
                        })?;

                    let (point_id, operation) =
                        if let Some((existing_point_id, deleted_at)) = existing_metadata {
                            // Entity exists - reuse point_id and use UPDATE
                            if deleted_at.is_some() {
                                // Was deleted, now being re-added - use INSERT with new point_id
                                (
                                    uuid::Uuid::new_v4(),
                                    codesearch_storage::postgres::OutboxOperation::Insert,
                                )
                            } else {
                                // Still active - use UPDATE with existing point_id
                                (
                                    existing_point_id,
                                    codesearch_storage::postgres::OutboxOperation::Update,
                                )
                            }
                        } else {
                            // New entity - generate new point_id and use INSERT
                            (
                                uuid::Uuid::new_v4(),
                                codesearch_storage::postgres::OutboxOperation::Insert,
                            )
                        };

                    debug!(
                        "Entity {}: operation={:?}, point_id={}, existing={:?}",
                        entity.entity_id,
                        operation,
                        point_id,
                        existing_metadata.is_some()
                    );

                    // Track for file snapshot
                    let file_path_str = entity
                        .file_path
                        .to_str()
                        .ok_or_else(|| Error::Storage("Invalid file path".to_string()))?
                        .to_string();
                    entities_by_file
                        .entry(file_path_str.clone())
                        .or_default()
                        .push(entity.entity_id.clone());

                    // Add to batch
                    batch_data.push((
                        entity.clone(),
                        embedding.clone(),
                        operation,
                        point_id,
                        codesearch_storage::postgres::TargetStore::Qdrant,
                        git_commit.clone(),
                    ));
                }

                // Store all entities with outbox entries in a single transaction
                let batch_refs: Vec<_> = batch_data
                    .iter()
                    .map(|(entity, embedding, op, point_id, target, git_commit)| {
                        (
                            entity,
                            embedding.as_slice(),
                            *op,
                            *point_id,
                            *target,
                            git_commit.clone(),
                        )
                    })
                    .collect();

                self.postgres_client
                    .store_entities_with_outbox_batch(repository_id, &batch_refs)
                    .await
                    .map_err(|e| {
                        error!("Failed to store entities batch: {e}");
                        e
                    })?;

                debug!(
                    "Successfully wrote {} entities to Postgres with outbox in single transaction",
                    entity_embedding_pairs.len()
                );
            }

            debug!(
                "Successfully bulk loaded batch of {} files",
                file_paths.len()
            );
        }

        // Detect and handle stale entities for ALL processed files (even empty ones)
        for file_path in processed_files {
            let file_path_str = file_path
                .to_str()
                .ok_or_else(|| Error::Storage("Invalid file path".to_string()))?;
            let entity_ids = entities_by_file
                .get(file_path_str)
                .cloned()
                .unwrap_or_default();

            self.handle_file_change(repository_id, &file_path, entity_ids, git_commit.clone())
                .await?;
        }

        Ok(stats)
    }

    /// Extract entities from a single file (used for parallel processing)
    async fn extract_from_file(
        &mut self,
        file_path: &Path,
    ) -> Result<(Vec<CodeEntity>, IndexStats)> {
        debug!("Extracting from file: {:?}", file_path);

        let mut stats = IndexStats::default();

        // Create extractor for this file
        let extractor = match create_extractor(file_path, &self.repository_id) {
            Some(ext) => ext,
            None => {
                debug!("No extractor available for file: {:?}", file_path);
                return Ok((Vec::new(), stats));
            }
        };

        // Read file
        let content = fs::read_to_string(file_path)
            .await
            .map_err(|e| Error::parse(file_path.display().to_string(), e.to_string()))?;

        // Extract entities
        let entities = extractor.extract(&content, file_path)?;
        debug!("Extracted {} entities from {:?}", entities.len(), file_path);

        // Update stats
        stats.set_entities_extracted(entities.len());
        // Note: Relationships are not directly exposed in CodeEntity yet

        Ok((entities, stats))
    }

    /// Detect and mark stale entities when re-indexing a file
    async fn handle_file_change(
        &self,
        repository_id: uuid::Uuid,
        file_path: &Path,
        new_entity_ids: Vec<String>,
        git_commit: Option<String>,
    ) -> Result<()> {
        let file_path_str = file_path
            .to_str()
            .ok_or_else(|| Error::Storage("Invalid file path".to_string()))?;

        // Get previous snapshot
        let old_entity_ids = self
            .postgres_client
            .get_file_snapshot(repository_id, file_path_str)
            .await?
            .unwrap_or_default();

        // Find stale entities (in old but not in new)
        let stale_ids: Vec<String> = old_entity_ids
            .iter()
            .filter(|old_id| !new_entity_ids.contains(old_id))
            .cloned()
            .collect();

        if !stale_ids.is_empty() {
            tracing::info!(
                "Found {} stale entities in {}",
                stale_ids.len(),
                file_path_str
            );

            // Mark entities as deleted
            self.postgres_client
                .mark_entities_deleted(repository_id, &stale_ids)
                .await?;

            // Write DELETE entries to outbox
            for entity_id in &stale_ids {
                let payload = serde_json::json!({
                    "entity_ids": [entity_id],
                    "reason": "file_change"
                });

                self.postgres_client
                    .write_outbox_entry(
                        repository_id,
                        entity_id,
                        codesearch_storage::postgres::OutboxOperation::Delete,
                        codesearch_storage::postgres::TargetStore::Qdrant,
                        payload,
                    )
                    .await?;
            }
        }

        // Update snapshot with current state
        self.postgres_client
            .update_file_snapshot(repository_id, file_path_str, new_entity_ids, git_commit)
            .await?;

        Ok(())
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
    use super::*;
    use codesearch_core::entities::{
        EntityMetadata, EntityType, Language, SourceLocation, Visibility,
    };
    use codesearch_embeddings::MockEmbeddingProvider;
    use codesearch_storage::postgres::mock::MockPostgresClient;
    use codesearch_storage::postgres::PostgresClientTrait;
    use std::path::PathBuf;
    use tempfile::TempDir;
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

    fn create_test_indexer(
        temp_dir: &TempDir,
        repository_id: &str,
        postgres_client: std::sync::Arc<MockPostgresClient>,
    ) -> RepositoryIndexer {
        let embedding_manager = std::sync::Arc::new(EmbeddingManager::new(std::sync::Arc::new(
            MockEmbeddingProvider::new(384),
        )));

        RepositoryIndexer::new(
            temp_dir.path().to_path_buf(),
            repository_id.to_string(),
            embedding_manager,
            postgres_client,
            None,
        )
    }

    #[tokio::test]
    async fn test_handle_file_change_detects_stale_entities() {
        let temp_dir = TempDir::new().unwrap();
        let repo_uuid = Uuid::new_v4();
        let repo_id = repo_uuid.to_string();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let indexer = create_test_indexer(&temp_dir, &repo_id, postgres.clone());

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

        // Run handle_file_change
        indexer
            .handle_file_change(
                repo_uuid,
                std::path::Path::new(file_path),
                new_entities.clone(),
                None,
            )
            .await
            .unwrap();

        // Verify entity2 was marked as deleted
        assert!(postgres.is_entity_deleted(repo_uuid, "entity2"));
        assert!(!postgres.is_entity_deleted(repo_uuid, "entity1"));

        // Verify snapshot was updated
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap();
        assert_eq!(snapshot, Some(new_entities));

        // Verify DELETE outbox entry was created
        assert_eq!(postgres.unprocessed_outbox_count(), 1);
    }

    #[tokio::test]
    async fn test_handle_file_change_detects_renamed_function() {
        let temp_dir = TempDir::new().unwrap();
        let repo_uuid = Uuid::new_v4();
        let repo_id = repo_uuid.to_string();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let indexer = create_test_indexer(&temp_dir, &repo_id, postgres.clone());

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

        indexer
            .handle_file_change(
                repo_uuid,
                std::path::Path::new(file_path),
                new_entities.clone(),
                None,
            )
            .await
            .unwrap();

        // Old entity should be marked deleted
        assert!(postgres.is_entity_deleted(repo_uuid, "entity_old_name"));
    }

    #[tokio::test]
    async fn test_handle_file_change_handles_added_entities() {
        let temp_dir = TempDir::new().unwrap();
        let repo_uuid = Uuid::new_v4();
        let repo_id = repo_uuid.to_string();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let indexer = create_test_indexer(&temp_dir, &repo_id, postgres.clone());

        let file_path = "test.rs";

        // Old snapshot: one entity
        let old_entities = vec!["entity1".to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // New state: added entity2
        let new_entities = vec!["entity1".to_string(), "entity2".to_string()];

        indexer
            .handle_file_change(
                repo_uuid,
                std::path::Path::new(file_path),
                new_entities.clone(),
                None,
            )
            .await
            .unwrap();

        // No entities should be marked as deleted
        assert!(!postgres.is_entity_deleted(repo_uuid, "entity1"));
        assert!(!postgres.is_entity_deleted(repo_uuid, "entity2"));

        // Snapshot should be updated
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap();
        assert_eq!(snapshot, Some(new_entities));

        // No DELETE outbox entries
        assert_eq!(postgres.unprocessed_outbox_count(), 0);
    }

    #[tokio::test]
    async fn test_handle_file_change_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let repo_uuid = Uuid::new_v4();
        let repo_id = repo_uuid.to_string();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let indexer = create_test_indexer(&temp_dir, &repo_id, postgres.clone());

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

        indexer
            .handle_file_change(
                repo_uuid,
                std::path::Path::new(file_path),
                new_entities.clone(),
                None,
            )
            .await
            .unwrap();

        // All entities should be marked as deleted
        assert!(postgres.is_entity_deleted(repo_uuid, "entity1"));
        assert!(postgres.is_entity_deleted(repo_uuid, "entity2"));
        assert!(postgres.is_entity_deleted(repo_uuid, "entity3"));

        // Should have 3 DELETE outbox entries
        assert_eq!(postgres.unprocessed_outbox_count(), 3);
    }

    #[tokio::test]
    async fn test_handle_file_change_no_previous_snapshot() {
        let temp_dir = TempDir::new().unwrap();
        let repo_uuid = Uuid::new_v4();
        let repo_id = repo_uuid.to_string();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let indexer = create_test_indexer(&temp_dir, &repo_id, postgres.clone());

        let file_path = "test.rs";

        // No previous snapshot
        let new_entities = vec!["entity1".to_string(), "entity2".to_string()];

        indexer
            .handle_file_change(
                repo_uuid,
                std::path::Path::new(file_path),
                new_entities.clone(),
                None,
            )
            .await
            .unwrap();

        // No entities should be deleted (first time indexing)
        assert_eq!(postgres.unprocessed_outbox_count(), 0);

        // Snapshot should be created
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap();
        assert_eq!(snapshot, Some(new_entities));
    }

    #[tokio::test]
    async fn test_handle_file_change_no_changes() {
        let temp_dir = TempDir::new().unwrap();
        let repo_uuid = Uuid::new_v4();
        let repo_id = repo_uuid.to_string();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let indexer = create_test_indexer(&temp_dir, &repo_id, postgres.clone());

        let file_path = "test.rs";

        // Old snapshot
        let entities = vec!["entity1".to_string(), "entity2".to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, entities.clone(), None)
            .await
            .unwrap();

        // Re-index with same entities
        indexer
            .handle_file_change(
                repo_uuid,
                std::path::Path::new(file_path),
                entities.clone(),
                None,
            )
            .await
            .unwrap();

        // No entities deleted
        assert_eq!(postgres.unprocessed_outbox_count(), 0);

        // Snapshot still updated (for git commit tracking)
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap();
        assert_eq!(snapshot, Some(entities));
    }

    #[tokio::test]
    async fn test_handle_file_change_writes_delete_to_outbox() {
        let temp_dir = TempDir::new().unwrap();
        let repo_uuid = Uuid::new_v4();
        let repo_id = repo_uuid.to_string();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let indexer = create_test_indexer(&temp_dir, &repo_id, postgres.clone());

        let file_path = "test.rs";

        // Setup with entities
        let old_entities = vec!["stale_entity".to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // Remove entity
        indexer
            .handle_file_change(repo_uuid, std::path::Path::new(file_path), vec![], None)
            .await
            .unwrap();

        // Verify outbox entry
        let entries = postgres
            .get_unprocessed_outbox_entries(codesearch_storage::postgres::TargetStore::Qdrant, 10)
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
        let temp_dir = TempDir::new().unwrap();
        let repo_uuid = Uuid::new_v4();
        let repo_id = repo_uuid.to_string();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let indexer = create_test_indexer(&temp_dir, &repo_id, postgres.clone());

        let file_path = "test.rs";
        let git_commit = Some("abc123".to_string());
        let new_entities = vec!["entity1".to_string()];

        indexer
            .handle_file_change(
                repo_uuid,
                std::path::Path::new(file_path),
                new_entities.clone(),
                git_commit.clone(),
            )
            .await
            .unwrap();

        // Snapshot should be stored with git commit
        let snapshot = postgres
            .get_snapshot_sync(repo_uuid, file_path)
            .expect("Snapshot should exist");
        assert_eq!(snapshot, new_entities);
    }
}
