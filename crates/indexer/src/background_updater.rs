//! Background index updating based on configured strategy
//!
//! This module provides background index updating that runs during `codesearch serve`.
//! The update strategy determines how the index is kept in sync:
//!
//! - `MainOnly`: Poll git periodically, detect main branch changes, re-index
//! - `Live`: Watch filesystem changes via watcher, process incrementally
//! - `Disabled`: No automatic updating (user runs `codesearch index` manually)

use crate::{catch_up_from_git, start_watching, IndexerConfig, Result};
use codesearch_core::config::{UpdateStrategy, WatcherConfig};
use codesearch_embeddings::EmbeddingManager;
use codesearch_storage::PostgresClientTrait;
use codesearch_watcher::{FileWatcher, GitRepository};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Handle for managing background index updating
///
/// Dropping this handle will signal the background task to shut down.
pub struct BackgroundUpdaterHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    task_handle: Option<JoinHandle<()>>,
}

impl BackgroundUpdaterHandle {
    /// Create a no-op handle (for Disabled strategy)
    fn noop() -> Self {
        Self {
            shutdown_tx: None,
            task_handle: None,
        }
    }

    /// Signal the background task to shut down
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    /// Wait for the background task to complete
    pub async fn wait(mut self) {
        self.shutdown();
        if let Some(handle) = self.task_handle.take() {
            let _ = handle.await;
        }
    }
}

impl Drop for BackgroundUpdaterHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Start background index updating based on the configured strategy.
///
/// # Arguments
/// * `update_strategy` - The update strategy to use
/// * `repo_id` - UUID of the repository
/// * `repo_path` - Path to the repository root
/// * `embedding_manager` - Embedding manager for generating embeddings
/// * `postgres_client` - PostgreSQL client for database operations
/// * `indexer_config` - Configuration for the indexer
/// * `watcher_config` - Configuration for the file watcher
///
/// # Returns
/// A handle that can be used to shut down the background task.
pub async fn start_background_updater(
    update_strategy: UpdateStrategy,
    repo_id: Uuid,
    repo_path: PathBuf,
    embedding_manager: Arc<EmbeddingManager>,
    postgres_client: Arc<dyn PostgresClientTrait>,
    indexer_config: IndexerConfig,
    watcher_config: WatcherConfig,
) -> Result<BackgroundUpdaterHandle> {
    match update_strategy {
        UpdateStrategy::MainOnly => {
            start_main_only_updater(
                repo_id,
                repo_path,
                embedding_manager,
                postgres_client,
                indexer_config,
                watcher_config.main_branch_poll_interval_secs,
            )
            .await
        }
        UpdateStrategy::Live => {
            start_live_updater(
                repo_id,
                repo_path,
                embedding_manager,
                postgres_client,
                indexer_config,
                watcher_config,
            )
            .await
        }
        UpdateStrategy::Disabled => {
            info!("Background index updating disabled");
            Ok(BackgroundUpdaterHandle::noop())
        }
    }
}

/// Start the MainOnly strategy updater
///
/// Polls git periodically to detect changes on the main branch and re-indexes.
async fn start_main_only_updater(
    repo_id: Uuid,
    repo_path: PathBuf,
    embedding_manager: Arc<EmbeddingManager>,
    postgres_client: Arc<dyn PostgresClientTrait>,
    indexer_config: IndexerConfig,
    poll_interval_secs: u64,
) -> Result<BackgroundUpdaterHandle> {
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    // Open git repository
    let git_repo = GitRepository::open(&repo_path)?;

    let task_handle = tokio::spawn(async move {
        info!(
            "Starting MainOnly background updater (polling every {}s)",
            poll_interval_secs
        );

        let poll_interval = Duration::from_secs(poll_interval_secs);
        let mut last_commit: Option<String> = None;

        loop {
            // Check for shutdown signal
            match shutdown_rx.try_recv() {
                Ok(()) | Err(oneshot::error::TryRecvError::Closed) => {
                    info!("MainOnly updater received shutdown signal");
                    break;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
            }

            // Check if we're on a main branch
            let current_branch = match git_repo.current_branch() {
                Ok(branch) => branch,
                Err(e) => {
                    debug!("Failed to get current branch: {e}");
                    tokio::time::sleep(poll_interval).await;
                    continue;
                }
            };

            let is_main_branch = is_main_branch_name(&current_branch);

            if !is_main_branch {
                debug!(
                    "Not on main branch (current: {}), skipping update",
                    current_branch
                );
                tokio::time::sleep(poll_interval).await;
                continue;
            }

            // Check if HEAD has changed
            let current_commit = match git_repo.current_commit_hash() {
                Ok(commit) => commit,
                Err(e) => {
                    debug!("Failed to get current commit: {e}");
                    tokio::time::sleep(poll_interval).await;
                    continue;
                }
            };

            let should_update = !matches!(&last_commit, Some(last) if last == &current_commit);

            if should_update {
                info!(
                    "Detected changes on {} branch (commit: {})",
                    current_branch,
                    &current_commit[..8.min(current_commit.len())]
                );

                match catch_up_from_git(
                    &repo_path,
                    repo_id,
                    &postgres_client,
                    &embedding_manager,
                    &git_repo,
                    &indexer_config.sparse_embeddings,
                )
                .await
                {
                    Ok(stats) => {
                        info!(
                            "Background index update complete: {} files processed, {} failed",
                            stats.files_processed, stats.files_failed
                        );
                        last_commit = Some(current_commit);
                    }
                    Err(e) => {
                        error!("Background index update failed: {e}");
                    }
                }
            } else {
                debug!("No changes detected on main branch");
            }

            tokio::time::sleep(poll_interval).await;
        }

        info!("MainOnly updater stopped");
    });

    Ok(BackgroundUpdaterHandle {
        shutdown_tx: Some(shutdown_tx),
        task_handle: Some(task_handle),
    })
}

/// Start the Live strategy updater
///
/// Watches the filesystem for changes and processes them incrementally.
async fn start_live_updater(
    repo_id: Uuid,
    repo_path: PathBuf,
    embedding_manager: Arc<EmbeddingManager>,
    postgres_client: Arc<dyn PostgresClientTrait>,
    indexer_config: IndexerConfig,
    watcher_config: WatcherConfig,
) -> Result<BackgroundUpdaterHandle> {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    // Create file watcher with the watcher crate's config
    let watcher_crate_config = codesearch_watcher::WatcherConfig::builder()
        .debounce_ms(watcher_config.debounce_ms)
        .ignore_patterns(watcher_config.ignore_patterns.clone())
        .build();

    let mut file_watcher = FileWatcher::new(watcher_crate_config)?;

    // Start watching the repository
    let event_rx = file_watcher.watch(&repo_path).await?;

    info!(
        "Starting Live background updater for {}",
        repo_path.display()
    );

    // Use the existing start_watching function from lib.rs
    let indexer_handle = start_watching(
        event_rx,
        repo_id,
        repo_path,
        embedding_manager,
        postgres_client,
        indexer_config,
    );

    // Spawn a task that waits for shutdown and then stops the watcher
    let task_handle = tokio::spawn(async move {
        // Wait for shutdown signal
        let _ = shutdown_rx.await;

        // Drop the file watcher to stop watching
        drop(file_watcher);

        // Wait for the indexer task to finish processing remaining events
        match indexer_handle.await {
            Ok(Ok(())) => info!("Live updater stopped cleanly"),
            Ok(Err(e)) => warn!("Live updater stopped with error: {e}"),
            Err(e) => warn!("Live updater task panicked: {e}"),
        }
    });

    Ok(BackgroundUpdaterHandle {
        shutdown_tx: Some(shutdown_tx),
        task_handle: Some(task_handle),
    })
}

/// Check if a branch name is a main/default branch
fn is_main_branch_name(branch: &str) -> bool {
    matches!(branch.to_lowercase().as_str(), "main" | "master" | "trunk")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_main_branch_name() {
        assert!(is_main_branch_name("main"));
        assert!(is_main_branch_name("Main"));
        assert!(is_main_branch_name("MAIN"));
        assert!(is_main_branch_name("master"));
        assert!(is_main_branch_name("Master"));
        assert!(is_main_branch_name("trunk"));

        assert!(!is_main_branch_name("develop"));
        assert!(!is_main_branch_name("feature/foo"));
        assert!(!is_main_branch_name("release/1.0"));
        assert!(!is_main_branch_name("detached:abc123"));
    }

    #[test]
    fn test_background_updater_handle_noop() {
        let handle = BackgroundUpdaterHandle::noop();
        assert!(handle.shutdown_tx.is_none());
        assert!(handle.task_handle.is_none());
    }
}
