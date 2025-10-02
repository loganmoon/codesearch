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
use codesearch_storage::EmbeddedEntity;

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
    embedding_manager: std::sync::Arc<EmbeddingManager>,
    postgres_client: std::sync::Arc<codesearch_storage::postgres::PostgresClient>,
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
        embedding_manager: std::sync::Arc<EmbeddingManager>,
        postgres_client: std::sync::Arc<codesearch_storage::postgres::PostgresClient>,
        git_repo: Option<codesearch_watcher::GitRepository>,
    ) -> Self {
        Self {
            repository_path,
            embedding_manager,
            postgres_client,
            git_repo,
        }
    }

    /// Get the repository path
    pub fn repository_path(&self) -> &Path {
        &self.repository_path
    }

    /// Index the entire repository
    pub async fn index_repository(&mut self) -> Result<IndexResult> {
        info!("Starting repository indexing: {:?}", self.repository_path);
        let start_time = Instant::now();

        // Find all files to process
        let files = find_files(&self.repository_path)?;
        info!("Found {} files to process", files.len());

        // Create progress tracking
        let mut progress = IndexProgress::new(files.len());
        let pb = create_progress_bar(files.len());

        // Process statistics
        let mut stats = IndexStats::default();

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
                    // Process failed batch files individually as fallback
                    for file_path in chunk {
                        match self.process_file(file_path).await {
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
        stats.total_files = files.len();
        stats.processing_time_ms = start_time.elapsed().as_millis() as u64;

        info!(
            "Indexing complete: {} files, {} entities, {} relationships{} in {:.2}s",
            stats.total_files,
            stats.entities_extracted,
            stats.relationships_extracted,
            if stats.entities_skipped_size > 0 {
                format!(
                    " ({} entities skipped due to size)",
                    stats.entities_skipped_size
                )
            } else {
                String::new()
            },
            stats.processing_time_ms as f64 / 1000.0
        );

        Ok(IndexResult {
            stats,
            errors: Vec::new(),
        })
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

        // Process each extraction result
        for (file_path, result) in file_paths.iter().zip(extraction_results) {
            match result {
                Ok((entities, file_stats)) => {
                    // Just add entities directly to batch without transformation
                    batch_entities.extend(entities);
                    stats.merge(file_stats);
                }
                Err(e) => {
                    error!("Failed to extract from {:?}: {}", file_path, e);
                    stats.increment_failed_files();
                    errors.push(e.to_string());
                }
            }
        }

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
            let mut embedded_entities: Vec<EmbeddedEntity> = Vec::new(); // create destination
            let mut entities_with_embeddings: Vec<CodeEntity> = Vec::new(); // track entities for postgres

            for (entity, opt_embedding) in batch_entities
                .into_iter()
                .zip(option_embeddings.into_iter())
            {
                if let Some(embedding) = opt_embedding {
                    entities_with_embeddings.push(entity.clone());
                    embedded_entities.push(EmbeddedEntity { entity, embedding });
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
                "After embedding: embedded_entities={}, entities_with_embeddings={}",
                embedded_entities.len(),
                entities_with_embeddings.len()
            );

            // Only store entities that have embeddings
            if !embedded_entities.is_empty() {
                info!(
                    "Processing {} entities with embeddings",
                    embedded_entities.len()
                );

                // Write entities to Postgres outbox within transaction
                let git_commit = self.current_git_commit().await.ok();
                debug!(
                    "Writing {} entities to Postgres outbox (git commit: {:?})",
                    entities_with_embeddings.len(),
                    git_commit
                );

                for entity in &entities_with_embeddings {
                    // Store metadata and version
                    let version_id = self
                        .postgres_client
                        .store_entity_metadata(entity, uuid::Uuid::new_v4(), git_commit.clone())
                        .await
                        .map_err(|e| {
                            error!(
                                "Failed to store entity {} metadata in Postgres: {e}",
                                entity.entity_id
                            );
                            e
                        })?;

                    // Write to outbox for Qdrant
                    let payload = serde_json::to_value(entity)
                        .map_err(|e| Error::Storage(format!("Failed to serialize entity: {e}")))?;

                    self.postgres_client
                        .write_outbox_entry(
                            &entity.entity_id,
                            codesearch_storage::postgres::OutboxOperation::Insert,
                            codesearch_storage::postgres::TargetStore::Qdrant,
                            payload,
                            version_id,
                        )
                        .await
                        .map_err(|e| {
                            error!(
                                "Failed to write outbox entry for entity {}: {e}",
                                entity.entity_id
                            );
                            e
                        })?;
                }

                debug!(
                    "Successfully wrote {} entities to Postgres outbox",
                    entities_with_embeddings.len()
                );
            }

            debug!(
                "Successfully bulk loaded batch of {} files",
                file_paths.len()
            );
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
        let extractor = match create_extractor(file_path) {
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

    /// Process a single file through the indexing pipeline
    async fn process_file(&mut self, file_path: &Path) -> Result<IndexStats> {
        debug!("Processing file: {:?}", file_path);

        // Initialize stats for this file
        let mut stats = IndexStats::default();

        // Stage 1: Create extractor for file
        let extractor = match create_extractor(file_path) {
            Some(ext) => ext,
            None => {
                debug!("No extractor available for file: {:?}", file_path);
                return Ok(stats);
            }
        };

        // Read file
        let content = fs::read_to_string(file_path)
            .await
            .map_err(|e| Error::parse(file_path.display().to_string(), e.to_string()))?;

        // Extract entities
        let entities = extractor.extract(&content, file_path)?;
        debug!("Extracted {} entities from {:?}", entities.len(), file_path);

        if entities.is_empty() {
            return Ok(stats);
        }

        // Update extraction stats
        stats.entities_extracted = entities.len();
        // Note: Relationships are not directly exposed in CodeEntity yet

        // Stage 2: Store - Bulk load to storage
        debug!("Storing {} entities", entities.len());

        // Generate embeddings for entities
        let embedding_texts: Vec<String> = entities.iter().map(extract_embedding_content).collect();

        let option_embeddings = self
            .embedding_manager
            .embed(embedding_texts)
            .await
            .map_err(|e| Error::Storage(format!("Failed to generate embeddings: {e}")))?;

        // Filter entities with valid embeddings
        let mut embedded_entities: Vec<EmbeddedEntity> = Vec::new();
        let mut entities_with_embeddings: Vec<CodeEntity> = Vec::new();

        for (entity, opt_embedding) in entities.into_iter().zip(option_embeddings.into_iter()) {
            if let Some(embedding) = opt_embedding {
                entities_with_embeddings.push(entity.clone());
                embedded_entities.push(EmbeddedEntity { entity, embedding });
            } else {
                stats.entities_skipped_size += 1;
                info!(
                    "Skipped entity due to size: {} in {}",
                    entity.name,
                    entity.file_path.display()
                );
            }
        }

        // Only store entities that have embeddings
        if !embedded_entities.is_empty() {
            // Write entities to Postgres outbox
            let git_commit = self.current_git_commit().await.ok();

            for entity in &entities_with_embeddings {
                // Store metadata and version
                let version_id = self
                    .postgres_client
                    .store_entity_metadata(entity, uuid::Uuid::new_v4(), git_commit.clone())
                    .await
                    .map_err(|e| Error::Storage(format!("Failed to store entity metadata: {e}")))?;

                // Write to outbox for Qdrant
                let payload = serde_json::to_value(entity)
                    .map_err(|e| Error::Storage(format!("Failed to serialize entity: {e}")))?;

                self.postgres_client
                    .write_outbox_entry(
                        &entity.entity_id,
                        codesearch_storage::postgres::OutboxOperation::Insert,
                        codesearch_storage::postgres::TargetStore::Qdrant,
                        payload,
                        version_id,
                    )
                    .await
                    .map_err(|e| Error::Storage(format!("Failed to write outbox entry: {e}")))?;
            }
        }

        debug!("Successfully stored entities from {:?}", file_path);

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
                    // Process failed batch files individually as fallback
                    for file_path in chunk {
                        match self.process_file(file_path).await {
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
mod tests {
    // Tests disabled: RepositoryIndexer now requires PostgresClient
    // TODO: Create MockPostgresClient to enable unit tests
}
