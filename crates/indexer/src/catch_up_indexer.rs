//! Catch-up indexing based on git diff
//!
//! This module handles catching up the index when offline changes have occurred.

use crate::{common::ResultExt, file_change_processor::process_file_changes, Result};
use codesearch_core::config::SparseEmbeddingsConfig;
use codesearch_embeddings::EmbeddingManager;
use codesearch_storage::PostgresClientTrait;
use codesearch_watcher::{DiffStats, FileChange, FileDiffChangeType, FileMetadata, GitRepository};
use std::{path::Path, sync::Arc, time::SystemTime};
use tracing::{info, warn};
use uuid::Uuid;

/// Get the short form of a git commit hash (first 8 chars or full hash if shorter)
fn short_hash(hash: &str) -> &str {
    hash.get(..8).unwrap_or(hash)
}

/// Statistics for catch-up indexing
#[derive(Debug, Clone, Default)]
pub struct CatchUpStats {
    pub files_changed: usize,
    pub files_processed: usize,
    pub files_failed: usize,
    pub entities_added: usize,
    pub entities_updated: usize,
    pub entities_deleted: usize,
}

/// Run catch-up indexing based on git diff
///
/// This compares the last indexed commit with the current HEAD and processes all
/// changed files to bring the index up to date.
pub async fn catch_up_from_git(
    repo_root: &Path,
    repo_id: Uuid,
    postgres_client: &Arc<dyn PostgresClientTrait>,
    embedding_manager: &Arc<EmbeddingManager>,
    git_repo: &GitRepository,
    sparse_embeddings_config: &SparseEmbeddingsConfig,
) -> Result<CatchUpStats> {
    let mut stats = CatchUpStats::default();

    // Get last indexed commit from database
    let last_indexed_commit = postgres_client
        .get_last_indexed_commit(repo_id)
        .await
        .storage_err("Failed to get last indexed commit")?;

    // Get current HEAD commit
    let current_commit = git_repo
        .current_commit_hash()
        .storage_err("Failed to get current commit")?;

    // Check if we need to catch up
    if let Some(ref last_commit) = last_indexed_commit {
        if last_commit == &current_commit {
            info!(
                "Index is up-to-date at commit {}",
                short_hash(&current_commit)
            );
            return Ok(stats);
        }

        info!(
            "Catching up index from {}..{} (git diff)",
            short_hash(last_commit),
            short_hash(&current_commit)
        );
    } else {
        info!(
            "No previous index found, will update to commit {}",
            short_hash(&current_commit)
        );
    }

    // Get changed files using git diff
    let changed_files = git_repo
        .get_changed_files_between_commits(last_indexed_commit.as_deref(), &current_commit)
        .storage_err("Failed to get changed files from git")?;

    if changed_files.is_empty() {
        info!("No file changes detected");
        postgres_client
            .set_last_indexed_commit(repo_id, &current_commit)
            .await
            .storage_err("Failed to update last indexed commit")?;
        return Ok(stats);
    }

    stats.files_changed = changed_files.len();
    info!("Found {} changed files to process", changed_files.len());

    // Convert git diff to FileChange events
    let file_changes: Vec<FileChange> = changed_files
        .into_iter()
        .map(|file_diff| {
            match file_diff.change_type {
                FileDiffChangeType::Added => {
                    // Create minimal metadata for added files (we don't have full info from git diff)
                    let metadata = FileMetadata::new(0, SystemTime::now(), 0o644);
                    FileChange::Created(file_diff.path, metadata)
                }
                FileDiffChangeType::Modified => {
                    // Create empty diff stats (we don't compute line-level diffs during catch-up)
                    let diff_stats = DiffStats::new(Vec::new(), Vec::new(), Vec::new());
                    FileChange::Modified(file_diff.path, diff_stats)
                }
                FileDiffChangeType::Deleted => FileChange::Deleted(file_diff.path),
            }
        })
        .collect();

    // Process all changes as a batch
    match process_file_changes(
        file_changes,
        repo_id,
        repo_root,
        embedding_manager,
        postgres_client,
        sparse_embeddings_config,
    )
    .await
    {
        Ok(processing_stats) => {
            stats.files_processed = processing_stats.files_processed;
            stats.files_failed = processing_stats.files_failed;
            stats.entities_added = processing_stats.entities_added;
            stats.entities_updated = processing_stats.entities_updated;
            stats.entities_deleted = processing_stats.entities_deleted;
        }
        Err(e) => {
            warn!("Catch-up processing failed: {}", e);
            stats.files_failed = stats.files_changed;
        }
    }

    // Update last indexed commit
    postgres_client
        .set_last_indexed_commit(repo_id, &current_commit)
        .await
        .storage_err("Failed to update last indexed commit")?;

    info!(
        "Catch-up indexing completed at commit {} ({} processed, {} failed)",
        short_hash(&current_commit),
        stats.files_processed,
        stats.files_failed
    );

    Ok(stats)
}
