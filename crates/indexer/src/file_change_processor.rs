//! File change processing for real-time indexing
//!
//! Processes file change events from the watcher in batches for optimal throughput.

use crate::common::{get_current_commit, path_to_str};
use crate::entity_processor;
use crate::Result;
use codesearch_core::config::SparseEmbeddingsConfig;
use codesearch_core::project_manifest::{detect_manifest, PackageMap};
use codesearch_embeddings::EmbeddingManager;
use codesearch_storage::PostgresClientTrait;
use codesearch_watcher::FileChange;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Statistics for file change processing
#[derive(Debug, Clone, Default)]
pub struct ProcessingStats {
    pub files_processed: usize,
    pub files_failed: usize,
    pub entities_added: usize,
    pub entities_updated: usize,
    pub entities_deleted: usize,
}

impl ProcessingStats {
    /// Merge another stats instance into this one
    pub fn merge(&mut self, other: ProcessingStats) {
        self.files_processed += other.files_processed;
        self.files_failed += other.files_failed;
        self.entities_added += other.entities_added;
        self.entities_updated += other.entities_updated;
        self.entities_deleted += other.entities_deleted;
    }
}

/// Process a batch of file changes (always batched, even if batch size is 1)
pub async fn process_file_changes(
    changes: Vec<FileChange>,
    repo_id: Uuid,
    repo_root: &Path,
    embedding_manager: &Arc<EmbeddingManager>,
    postgres_client: &Arc<dyn PostgresClientTrait>,
    sparse_embeddings_config: &SparseEmbeddingsConfig,
) -> Result<ProcessingStats> {
    if changes.is_empty() {
        return Ok(ProcessingStats::default());
    }

    info!(
        batch_size = changes.len(),
        "Processing batch of file changes"
    );
    let mut stats = ProcessingStats::default();

    // Fetch collection_name once for all operations in this batch
    let collection_name = postgres_client
        .get_collection_name(repo_id)
        .await?
        .ok_or_else(|| {
            codesearch_core::error::Error::Storage(
                "Repository collection_name not found".to_string(),
            )
        })?;

    // Detect project manifest for qualified name derivation
    let package_map: Option<PackageMap> = match detect_manifest(repo_root) {
        Ok(Some(manifest)) => {
            debug!(
                "Detected {:?} project with {} package(s) for incremental indexing",
                manifest.project_type,
                manifest.packages.len()
            );
            Some(manifest.packages)
        }
        Ok(None) => None,
        Err(e) => {
            debug!("Failed to detect project manifest for incremental indexing: {e}");
            None
        }
    };

    // Separate changes by type
    let mut files_to_index = Vec::new();
    let mut files_to_delete = Vec::new();
    let mut renames = Vec::new();

    for change in changes {
        match change {
            FileChange::Created(path, _) | FileChange::Modified(path, _) => {
                files_to_index.push(path);
            }
            FileChange::Deleted(path) => {
                files_to_delete.push(path);
            }
            FileChange::Renamed { from, to } => {
                renames.push((from, to));
            }
            FileChange::PermissionsChanged(_) => {
                // Ignore permission changes
            }
        }
    }

    // Handle renames (delete old, index new)
    for (from, to) in renames {
        let from_str = path_to_str(&from)?;

        if let Err(e) = entity_processor::mark_file_entities_deleted(
            repo_id,
            &collection_name,
            from_str,
            postgres_client.as_ref(),
        )
        .await
        {
            warn!(
                file_path = %from.display(),
                error = %e,
                "Failed to handle rename deletion"
            );
            stats.files_failed += 1;
        } else {
            stats.entities_deleted += 1;
        }
        files_to_index.push(to);
    }

    // Process deletions
    for path in files_to_delete {
        let path_str = path_to_str(&path)?;

        match entity_processor::mark_file_entities_deleted(
            repo_id,
            &collection_name,
            path_str,
            postgres_client.as_ref(),
        )
        .await
        {
            Ok(count) => {
                stats.files_processed += 1;
                stats.entities_deleted += count;
            }
            Err(e) => {
                warn!(
                    file_path = %path.display(),
                    error = %e,
                    "Failed to handle deletion"
                );
                stats.files_failed += 1;
            }
        }
    }

    // Process files to index (in batch)
    if !files_to_index.is_empty() {
        match process_file_batch(
            &files_to_index,
            repo_id,
            &collection_name,
            repo_root,
            embedding_manager,
            postgres_client,
            package_map.as_ref(),
            sparse_embeddings_config,
        )
        .await
        {
            Ok(batch_stats) => {
                stats.files_processed += batch_stats.files_processed;
                stats.files_failed += batch_stats.files_failed;
                stats.entities_added += batch_stats.entities_added;
                stats.entities_updated += batch_stats.entities_updated;
            }
            Err(e) => {
                warn!(
                    error = %e,
                    file_count = files_to_index.len(),
                    "Batch processing failed"
                );
                stats.files_failed += files_to_index.len();
            }
        }
    }

    info!(
        files_processed = stats.files_processed,
        files_failed = stats.files_failed,
        entities_added = stats.entities_added,
        entities_updated = stats.entities_updated,
        entities_deleted = stats.entities_deleted,
        "Batch complete"
    );

    Ok(stats)
}

/// Process a batch of files for indexing
#[allow(clippy::too_many_arguments)]
async fn process_file_batch(
    file_paths: &[PathBuf],
    repo_id: Uuid,
    collection_name: &str,
    repo_root: &Path,
    embedding_manager: &Arc<EmbeddingManager>,
    postgres_client: &Arc<dyn PostgresClientTrait>,
    package_map: Option<&PackageMap>,
    sparse_embeddings_config: &SparseEmbeddingsConfig,
) -> Result<ProcessingStats> {
    let mut stats = ProcessingStats::default();
    let mut batch_entities = Vec::new();

    // Canonicalize repo root once before the loop
    let canonical_repo = match repo_root.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            warn!(
                repo_root = %repo_root.display(),
                error = %e,
                "Failed to canonicalize repository root"
            );
            stats.files_failed = file_paths.len();
            return Ok(stats);
        }
    };

    // Extract entities from all files
    for file_path in file_paths {
        // Make path absolute relative to repo root
        let absolute_path = if file_path.is_absolute() {
            file_path.clone()
        } else {
            repo_root.join(file_path)
        };

        // Validate path is within repository bounds (prevent path traversal)
        let canonical_path = match absolute_path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                warn!(
                    file_path = %file_path.display(),
                    error = %e,
                    "Failed to canonicalize path"
                );
                stats.files_failed += 1;
                continue;
            }
        };

        if !canonical_path.starts_with(&canonical_repo) {
            warn!(
                file_path = %file_path.display(),
                "Path traversal attempt detected: file is outside repository"
            );
            stats.files_failed += 1;
            continue;
        }

        // Look up package context for this file
        let (package_name, source_root) = package_map
            .as_ref()
            .and_then(|pm| pm.find_package_for_file(&canonical_path))
            .map(|pkg| (Some(pkg.name.as_str()), Some(pkg.source_root.as_path())))
            .unwrap_or((None, None));

        match entity_processor::extract_entities_from_file(
            &canonical_path,
            &repo_id.to_string(),
            package_name,
            source_root,
        )
        .await
        {
            Ok(entities) => {
                batch_entities.extend(entities);
                stats.files_processed += 1;
            }
            Err(e) => {
                warn!(
                    file_path = %file_path.display(),
                    error = %e,
                    "Failed to extract entities"
                );
                stats.files_failed += 1;
            }
        }
    }

    // Get git commit
    let git_commit = get_current_commit(None, repo_root);

    // Process entities with embeddings using shared logic
    let (batch_stats, entities_by_file) = entity_processor::process_entity_batch(
        batch_entities,
        repo_id,
        collection_name.to_string(),
        git_commit.clone(),
        embedding_manager,
        postgres_client.as_ref(),
        postgres_client.max_entity_batch_size(),
        sparse_embeddings_config,
    )
    .await?;

    stats.entities_added = batch_stats.entities_added;
    stats.entities_updated = batch_stats.entities_updated;

    // Update file snapshots and handle stale entities
    for file_path in file_paths {
        let file_path_str = path_to_str(file_path)?;

        let new_entity_ids = entities_by_file
            .get(file_path_str)
            .cloned()
            .unwrap_or_default();

        if let Err(e) = entity_processor::update_file_snapshot_and_mark_stale(
            repo_id,
            collection_name,
            file_path_str,
            new_entity_ids,
            git_commit.clone(),
            postgres_client.as_ref(),
        )
        .await
        {
            warn!(
                file_path = file_path_str,
                error = %e,
                "Failed to update file snapshot"
            );
        }
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processing_stats_merge() {
        let mut stats1 = ProcessingStats {
            files_processed: 5,
            files_failed: 1,
            entities_added: 10,
            entities_updated: 3,
            entities_deleted: 2,
        };

        let stats2 = ProcessingStats {
            files_processed: 3,
            files_failed: 2,
            entities_added: 7,
            entities_updated: 1,
            entities_deleted: 0,
        };

        stats1.merge(stats2);

        assert_eq!(stats1.files_processed, 8);
        assert_eq!(stats1.files_failed, 3);
        assert_eq!(stats1.entities_added, 17);
        assert_eq!(stats1.entities_updated, 4);
        assert_eq!(stats1.entities_deleted, 2);
    }

    #[test]
    fn test_processing_stats_merge_with_empty() {
        let mut stats = ProcessingStats {
            files_processed: 5,
            files_failed: 1,
            entities_added: 10,
            entities_updated: 3,
            entities_deleted: 2,
        };

        stats.merge(ProcessingStats::default());

        assert_eq!(stats.files_processed, 5);
        assert_eq!(stats.files_failed, 1);
        assert_eq!(stats.entities_added, 10);
        assert_eq!(stats.entities_updated, 3);
        assert_eq!(stats.entities_deleted, 2);
    }
}
