use anyhow::Context;
use codesearch_core::error::Result;
use codesearch_embeddings::EmbeddingManager;
use codesearch_storage::postgres::PostgresClient;
use codesearch_watcher::{FileDiffChangeType, GitRepository};
use std::{path::Path, sync::Arc};
use tracing::{info, warn};
use uuid::Uuid;

/// Run catch-up indexing based on git diff
///
/// This compares the last indexed commit with the current HEAD and processes all
/// changed files to bring the index up to date.
pub(crate) async fn catch_up_index(
    repo_root: &Path,
    repository_id: Uuid,
    postgres_client: &Arc<PostgresClient>,
    embedding_manager: &Arc<EmbeddingManager>,
    git_repo: &GitRepository,
) -> Result<()> {
    // Get last indexed commit from database
    let last_indexed_commit = postgres_client
        .get_last_indexed_commit(repository_id)
        .await
        .context("Failed to get last indexed commit")?;

    // Get current HEAD commit
    let current_commit = git_repo
        .current_commit_hash()
        .context("Failed to get current commit")?;

    // Check if we need to catch up
    if let Some(ref last_commit) = last_indexed_commit {
        if last_commit == &current_commit {
            info!("Index is up-to-date at commit {}", &current_commit[..8]);
            return Ok(());
        }

        info!(
            "Catching up index from {}..{} ({})",
            &last_commit[..8],
            &current_commit[..8],
            if last_commit.len() >= 8 && current_commit.len() >= 8 {
                "git diff"
            } else {
                "full scan"
            }
        );
    } else {
        info!(
            "No previous index found, will update to commit {}",
            &current_commit[..8]
        );
    }

    // Get changed files using git diff
    let changed_files = git_repo
        .get_changed_files_between_commits(last_indexed_commit.as_deref(), &current_commit)
        .context("Failed to get changed files from git")?;

    if changed_files.is_empty() {
        info!("No file changes detected");
        postgres_client
            .set_last_indexed_commit(repository_id, &current_commit)
            .await
            .context("Failed to update last indexed commit")?;
        return Ok(());
    }

    info!("Found {} changed files to process", changed_files.len());

    // Process each changed file
    for file_diff in changed_files {
        match file_diff.change_type {
            FileDiffChangeType::Added | FileDiffChangeType::Modified => {
                // Re-index the file
                if let Err(e) = crate::file_watcher::reindex_single_file(
                    repo_root,
                    repository_id,
                    &file_diff.path,
                    postgres_client,
                    embedding_manager,
                )
                .await
                {
                    warn!("Failed to reindex file {}: {}", file_diff.path.display(), e);
                }
            }
            FileDiffChangeType::Deleted => {
                // Mark all entities in the file as deleted
                if let Err(e) = crate::file_watcher::handle_file_deletion(
                    repository_id,
                    &file_diff.path,
                    postgres_client,
                )
                .await
                {
                    warn!(
                        "Failed to handle deletion of file {}: {}",
                        file_diff.path.display(),
                        e
                    );
                }
            }
        }
    }

    // Update last indexed commit
    postgres_client
        .set_last_indexed_commit(repository_id, &current_commit)
        .await
        .context("Failed to update last indexed commit")?;

    info!(
        "✅ Catch-up indexing completed at commit {}",
        &current_commit[..8]
    );
    Ok(())
}
