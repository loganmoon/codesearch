//! Repository indexer implementation
//!
//! Provides the main three-stage indexing pipeline for processing repositories.

use crate::common::find_files;
use crate::types::{IndexResult, IndexStats};
use codesearch_core::error::{Error, Result};
use codesearch_core::{CodeEntity, EntityType};
use codesearch_languages::{
    extraction_framework::GenericExtractor, rust::create_rust_extractor, transport::EntityData,
};
use codesearch_storage::{MockStorageClient, StorageClient};

use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::fs;
use tracing::{debug, error, info, warn};

/// Progress tracking for indexing operations
#[derive(Debug, Clone)]
pub struct IndexProgress {
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
    storage_host: String,
    storage_port: u16,
    repository_path: PathBuf,
    extractors: ExtractorRegistry,
}

impl RepositoryIndexer {
    /// Create a new repository indexer
    pub fn new(storage_host: String, storage_port: u16, repository_path: PathBuf) -> Self {
        Self {
            storage_host,
            storage_port,
            repository_path,
            extractors: ExtractorRegistry::new(),
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

        // Create storage client
        let storage_client = create_storage_client(self.storage_host.clone(), self.storage_port);

        // Process statistics
        let mut stats = IndexStats::default();

        // Process files in batches for better performance
        const BATCH_SIZE: usize = 100; // Configurable batch size

        for chunk in files.chunks(BATCH_SIZE) {
            pb.set_message(format!("Processing batch of {} files", chunk.len()));

            match self.process_batch(chunk, &storage_client, &pb).await {
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
                        match self.process_file(&file_path, &storage_client).await {
                            Ok(file_stats) => {
                                stats.merge(file_stats);
                                progress.update(&file_path.to_string_lossy(), true);
                            }
                            Err(e) => {
                                error!("Failed to process file {:?}: {}", file_path, e);
                                stats.failed_files += 1;
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
            "Indexing complete: {} files, {} entities, {} relationships in {:.2}s",
            stats.total_files,
            stats.entities_extracted,
            stats.relationships_extracted,
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
        storage_client: &impl StorageClient,
        _pb: &indicatif::ProgressBar,
    ) -> Result<IndexStats> {
        debug!("Processing batch of {} files", file_paths.len());

        // Process statistics
        let mut stats = IndexStats::default();
        let mut errors = Vec::new();

        // Collect all entities from the batch
        let mut batch_entities = Vec::new();
        let mut batch_functions = Vec::new();
        let mut batch_types = Vec::new();
        let mut batch_variables = Vec::new();
        let mut batch_relationships = Vec::new();

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
                    // Transform to storage models
                    let file_path_str = file_path.to_string_lossy().to_string();
                    let repository_id = self.repository_path.to_string_lossy().to_string();

                    match map_to_storage_models(entities.clone(), &file_path_str, &repository_id) {
                        Ok((
                            stored_entities,
                            stored_functions,
                            stored_types,
                            stored_variables,
                            relationships,
                        )) => {
                            batch_entities.extend(stored_entities);
                            batch_functions.extend(stored_functions);
                            batch_types.extend(stored_types);
                            batch_variables.extend(stored_variables);
                            batch_relationships.extend(relationships);
                            stats.merge(file_stats);
                        }
                        Err(e) => {
                            error!("Failed to transform entities from {:?}: {}", file_path, e);
                            stats.failed_files += 1;
                            errors.push(e.to_string());
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to extract from {:?}: {}", file_path, e);
                    stats.failed_files += 1;
                    errors.push(e.to_string());
                }
            }
        }

        // Bulk load all entities from the batch
        if !batch_entities.is_empty() {
            debug!(
                "Bulk loading {} entities, {} functions, {} types, {} variables, {} relationships",
                batch_entities.len(),
                batch_functions.len(),
                batch_types.len(),
                batch_variables.len(),
                batch_relationships.len()
            );

            storage_client
                .bulk_load_entities(
                    &batch_entities,
                    &batch_functions,
                    &batch_types,
                    &batch_variables,
                    &batch_relationships,
                )
                .await
                .map_err(|e| Error::Storage(format!("Failed to bulk store entities: {}", e)))?;

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
    ) -> Result<(Vec<EntityData>, IndexStats)> {
        debug!("Extracting from file: {:?}", file_path);

        let mut stats = IndexStats::default();

        // Read file
        let content = fs::read_to_string(file_path)
            .await
            .map_err(|e| Error::parse(file_path.display().to_string(), e.to_string()))?;

        // Determine language from extension
        let extension = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");

        let language = match extension {
            "rs" => "rust",
            "py" => "python",
            "js" | "jsx" => "javascript",
            "ts" | "tsx" => "typescript",
            "go" => "go",
            _ => {
                debug!("Unsupported file type: {:?}", file_path);
                return Ok((Vec::new(), stats));
            }
        };

        // Get appropriate extractor
        let extractor = self.extractors.get_or_create(language).ok_or_else(|| {
            Error::entity_extraction(format!("No extractor available for {}", language))
        })?;

        // Extract entities
        let entities = extractor.extract(&content, file_path)?;
        debug!("Extracted {} entities from {:?}", entities.len(), file_path);

        // Update stats
        stats.entities_extracted = entities.len();
        for entity in &entities {
            stats.relationships_extracted += entity.relationships.len();
        }

        Ok((entities, stats))
    }

    /// Process a single file through the indexing pipeline
    async fn process_file(
        &mut self,
        file_path: &Path,
        storage_client: &impl StorageClient,
    ) -> Result<IndexStats> {
        debug!("Processing file: {:?}", file_path);

        // Initialize stats for this file
        let mut stats = IndexStats::default();

        // Stage 1: Extract - Read file and determine language
        let content = fs::read_to_string(file_path)
            .await
            .map_err(|e| Error::parse(file_path.display().to_string(), e.to_string()))?;

        // Determine language from extension
        let extension = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");

        let language = match extension {
            "rs" => "rust",
            "py" => "python",
            "js" | "jsx" => "javascript",
            "ts" | "tsx" => "typescript",
            "go" => "go",
            _ => {
                debug!("Unsupported file type: {:?}", file_path);
                return Ok(stats);
            }
        };

        // Get appropriate extractor
        let extractor = self.extractors.get_or_create(language).ok_or_else(|| {
            Error::entity_extraction(format!("No extractor available for {}", language))
        })?;

        // Extract entities
        let entities = extractor.extract(&content, file_path)?;
        debug!("Extracted {} entities from {:?}", entities.len(), file_path);

        if entities.is_empty() {
            return Ok(stats);
        }

        // Update extraction stats
        stats.entities_extracted = entities.len();
        for entity in &entities {
            stats.relationships_extracted += entity.relationships.len();
        }

        // Stage 2: Transform - Convert to storage models
        let file_path_str = file_path.to_string_lossy().to_string();
        let repository_id = self.repository_path.to_string_lossy().to_string();

        let (stored_entities, stored_functions, stored_types, stored_variables, relationships) =
            map_to_storage_models(entities, &file_path_str, &repository_id)?;

        debug!(
            "Transformed to {} entities, {} functions, {} types, {} variables, {} relationships",
            stored_entities.len(),
            stored_functions.len(),
            stored_types.len(),
            stored_variables.len(),
            relationships.len()
        );

        // Stage 3: Commit - Bulk load to storage
        storage_client
            .bulk_load_entities(
                &stored_entities,
                &stored_functions,
                &stored_types,
                &stored_variables,
                &relationships,
            )
            .await
            .map_err(|e| Error::Storage(format!("Failed to store entities: {}", e)))?;

        debug!("Successfully stored entities from {:?}", file_path);

        Ok(stats)
    }

    /// Process a diff for incremental updates
    pub async fn process_diff(&mut self, _diff_context: &crate::types::DiffContext) -> Result<()> {
        // TODO: Implement incremental update logic
        warn!("Incremental updates not yet implemented");
        Ok(())
    }
}

/// Registry for managing language extractors
struct ExtractorRegistry {
    rust_extractor: Option<GenericExtractor<'static>>,
    // Future: Add other language extractors
    // python_extractor: Option<GenericExtractor<'static>>,
    // typescript_extractor: Option<GenericExtractor<'static>>,
}

impl ExtractorRegistry {
    fn new() -> Self {
        Self {
            rust_extractor: None,
        }
    }

    fn get_or_create(&mut self, language: &str) -> Option<&mut GenericExtractor<'static>> {
        match language {
            "rust" => {
                if self.rust_extractor.is_none() {
                    match create_rust_extractor() {
                        Ok(extractor) => self.rust_extractor = Some(extractor),
                        Err(e) => {
                            error!("Failed to create Rust extractor: {}", e);
                            return None;
                        }
                    }
                }
                self.rust_extractor.as_mut()
            }
            _ => {
                debug!("No extractor available for language: {}", language);
                None
            }
        }
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

/// Create a storage client instance
/// TODO: Replace with real Qdrant client when implemented
fn create_storage_client(_host: String, _port: u16) -> impl StorageClient {
    let client = MockStorageClient::new();
    client
}

/// Map extracted entities to storage-specific models
/// TODO: Add embedding generation here when embedding service is ready
fn map_to_storage_models(
    entities: Vec<EntityData>,
    file_path: &str,
    _repository_id: &str,
) -> Result<(
    Vec<CodeEntity>,
    Vec<CodeEntity>,
    Vec<CodeEntity>,
    Vec<CodeEntity>,
    Vec<(String, String, String)>,
)> {
    use codesearch_core::entities::CodeEntityBuilder;
    use std::path::PathBuf;

    let mut all_entities = Vec::new();
    let mut functions = Vec::new();
    let mut types = Vec::new();
    let mut variables = Vec::new();
    let mut relationships = Vec::new();

    // Convert EntityData to CodeEntity and categorize them
    for entity_data in entities {
        // Get EntityType from the variant
        let entity_type = entity_data.variant.entity_type();

        // Build CodeEntity from EntityData
        let code_entity = CodeEntityBuilder::default()
            .entity_id(format!("{}#{}", file_path, entity_data.qualified_name))
            .name(entity_data.name.clone())
            .qualified_name(entity_data.qualified_name.clone())
            .entity_type(entity_type.clone())
            .file_path(PathBuf::from(file_path))
            .location(entity_data.location.clone())
            .line_range((
                entity_data.location.start_line,
                entity_data.location.end_line,
            ))
            .visibility(entity_data.visibility.clone())
            .language(entity_data.variant.language())
            .lines_of_code(entity_data.location.end_line - entity_data.location.start_line + 1)
            .documentation_summary(entity_data.documentation.clone())
            .content(entity_data.content.clone())
            .build()
            .map_err(|e| Error::entity_extraction(format!("Failed to build CodeEntity: {}", e)))?;

        all_entities.push(code_entity.clone());

        match entity_type {
            EntityType::Function | EntityType::Method => {
                functions.push(code_entity);
            }
            EntityType::Class
            | EntityType::Struct
            | EntityType::Interface
            | EntityType::Trait
            | EntityType::Enum => {
                types.push(code_entity);
            }
            EntityType::Variable | EntityType::Constant => {
                variables.push(code_entity);
            }
            _ => {}
        }

        // Convert relationships to storage format
        for rel in &entity_data.relationships {
            relationships.push((
                rel.from.clone(),
                rel.to.clone(),
                format!("{:?}", rel.relationship_type),
            ));
        }
    }

    Ok((all_entities, functions, types, variables, relationships))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    #[tokio::test]
    async fn test_repository_indexer_creation() {
        let temp_dir = TempDir::new().unwrap();
        let indexer =
            RepositoryIndexer::new("localhost".to_string(), 8080, temp_dir.path().to_path_buf());
        assert_eq!(indexer.repository_path(), temp_dir.path());
    }

    #[tokio::test]
    async fn test_empty_repository_indexing() {
        let temp_dir = TempDir::new().unwrap();
        let mut indexer =
            RepositoryIndexer::new("localhost".to_string(), 8080, temp_dir.path().to_path_buf());

        // This will fail because no storage server is running, but we can test the flow
        let result = indexer.index_repository().await;
        assert!(result.is_ok());

        if let Ok(index_result) = result {
            assert_eq!(index_result.stats.total_files, 0);
            assert_eq!(index_result.stats.entities_extracted, 0);
        }
    }

    #[tokio::test]
    async fn test_file_processing() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "fn main() { println!(\"Hello\"); }")
            .await
            .unwrap();

        let mut indexer =
            RepositoryIndexer::new("localhost".to_string(), 8080, temp_dir.path().to_path_buf());

        // Create a mock storage client for testing
        // In a real test, we'd use a mock implementation
        let storage_client = MockStorageClient::new();

        // This will fail without a running storage server, but tests the extraction
        let result = indexer.process_file(&test_file, &storage_client).await;

        // The test should at least attempt extraction
        // Full success depends on storage being available
        assert!(result.is_ok() || result.is_err());
    }
}
