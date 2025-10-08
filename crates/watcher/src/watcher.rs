//! Core file system watcher implementation
//!
//! This module provides the main file watcher using the notify crate
//! with cross-platform support and comprehensive error handling.

#![allow(dead_code)]

use crate::{
    config::{RecoveryConfig, WatcherConfig},
    debouncer::EventDebouncer,
    events::FileChange,
    git::{BranchWatcher, GitRepository},
    ignore::IgnoreFilter,
};
use codesearch_core::error::{Error, Result};
use notify::{
    Config as NotifyConfig, Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode,
    Watcher as NotifyWatcher,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, trace, warn};

/// Main file system watcher
pub struct FileWatcher {
    /// Configuration
    config: Arc<WatcherConfig>,
    /// Recovery configuration
    recovery_config: Arc<RecoveryConfig>,
    /// Ignore filter
    ignore_filter: Arc<IgnoreFilter>,
    /// Git repository (if available)
    git_repo: Option<Arc<GitRepository>>,
    /// Branch watcher (if Git is available)
    branch_watcher: Option<Arc<RwLock<BranchWatcher>>>,
    /// Active notify watcher
    watcher: Option<Arc<RwLock<RecommendedWatcher>>>,
    /// Paths being watched
    watched_paths: Arc<RwLock<Vec<PathBuf>>>,
    /// Cancellation token for stopping background tasks
    cancellation_token: Arc<tokio_util::sync::CancellationToken>,
}

impl FileWatcher {
    /// Create a new file watcher
    pub fn new(config: WatcherConfig) -> Result<Self> {
        let ignore_filter = IgnoreFilter::builder()
            .patterns(config.ignore_patterns.clone())
            .follow_symlinks(config.follow_symlinks)
            .max_file_size(config.max_file_size)
            .build()
            .map_err(|e| Error::watcher(format!("Failed to create ignore filter: {e}")))?;

        let recovery_config = RecoveryConfig::default();

        Ok(Self {
            config: Arc::new(config),
            recovery_config: Arc::new(recovery_config),
            ignore_filter: Arc::new(ignore_filter),
            git_repo: None,
            branch_watcher: None,
            watcher: None,
            watched_paths: Arc::new(RwLock::new(Vec::new())),
            cancellation_token: Arc::new(tokio_util::sync::CancellationToken::new()),
        })
    }

    /// Initialize Git integration for the given path
    pub async fn init_git(&mut self, path: &Path) -> Result<()> {
        match GitRepository::open(path) {
            Ok(repo) => {
                info!("Initialized Git repository at {:?}", repo.root_path());
                let branch_watcher = repo.watch_for_branch_changes().await?;
                self.branch_watcher = Some(Arc::new(RwLock::new(branch_watcher)));
                self.git_repo = Some(Arc::new(repo));
                Ok(())
            }
            Err(e) => {
                warn!("Failed to initialize Git repository: {}", e);
                // Continue without Git integration
                Ok(())
            }
        }
    }

    /// Start watching a path
    pub async fn watch(&mut self, path: impl AsRef<Path>) -> Result<mpsc::Receiver<FileChange>> {
        let path = path.as_ref().to_path_buf();

        // Initialize Git if not already done
        if self.git_repo.is_none() {
            self.init_git(&path).await?;
        }

        // Create channels
        let (notify_tx, notify_rx) = mpsc::channel(self.config.max_queue_size);
        let (debounced_tx, debounced_rx) = mpsc::channel(self.config.max_queue_size);

        // Create debouncer
        let debouncer = EventDebouncer::new(self.config.debounce_duration(), debounced_tx);

        // Start event processor
        self.start_event_processor(notify_rx, debouncer);

        // Initialize notify watcher with retry logic
        let mut watcher = self.init_watcher_with_retry(notify_tx).await?;

        // Watch the path
        self.add_watch_path(&mut watcher, &path).await?;

        // Store watcher and path
        self.watcher = Some(Arc::new(RwLock::new(watcher)));
        self.watched_paths.write().await.push(path);

        // Start branch monitoring if Git is enabled
        if let Some(branch_watcher) = &self.branch_watcher {
            self.start_branch_monitor(Arc::clone(branch_watcher));
        }

        Ok(debounced_rx)
    }

    /// Initialize notify watcher with retry logic
    async fn init_watcher_with_retry(
        &self,
        tx: mpsc::Sender<NotifyEvent>,
    ) -> Result<RecommendedWatcher> {
        let mut attempts = 0;
        let max_attempts = self.recovery_config.max_init_retries;

        loop {
            attempts += 1;

            match self.create_notify_watcher(tx.clone()) {
                Ok(watcher) => {
                    info!("File watcher initialized successfully");
                    return Ok(watcher);
                }
                Err(e) if attempts < max_attempts => {
                    warn!(
                        "Failed to initialize watcher (attempt {}/{}): {}",
                        attempts, max_attempts, e
                    );
                    tokio::time::sleep(Duration::from_millis(self.recovery_config.retry_delay_ms))
                        .await;
                }
                Err(e) => {
                    error!("Failed to initialize watcher after {} attempts", attempts);
                    return Err(Error::watcher(format!(
                        "Watcher initialization failed: {e}"
                    )));
                }
            }
        }
    }

    /// Create a notify watcher
    fn create_notify_watcher(&self, tx: mpsc::Sender<NotifyEvent>) -> Result<RecommendedWatcher> {
        let config = NotifyConfig::default()
            .with_poll_interval(Duration::from_millis(
                self.recovery_config.polling_interval_ms,
            ))
            .with_compare_contents(false);

        let tx_clone = tx.clone();
        let watcher = RecommendedWatcher::new(
            move |res: std::result::Result<NotifyEvent, notify::Error>| match res {
                Ok(event) => {
                    if let Err(e) = tx_clone.try_send(event) {
                        error!("Failed to send notify event: {}", e);
                    }
                }
                Err(e) => {
                    error!("Notify error: {}", e);
                }
            },
            config,
        )
        .map_err(|e| Error::watcher(format!("Failed to create watcher: {e}")))?;

        Ok(watcher)
    }

    /// Add a path to watch
    async fn add_watch_path(&self, watcher: &mut RecommendedWatcher, path: &Path) -> Result<()> {
        let recursive = if self.config.recursive_depth > 0 {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };

        watcher
            .watch(path, recursive)
            .map_err(|e| Error::watcher(format!("Failed to watch path {path:?}: {e}")))?;

        info!(
            "Watching path: {:?} (recursive: {})",
            path,
            matches!(recursive, RecursiveMode::Recursive)
        );
        Ok(())
    }

    /// Start the event processor
    fn start_event_processor(
        &self,
        mut notify_rx: mpsc::Receiver<NotifyEvent>,
        debouncer: EventDebouncer,
    ) {
        let ignore_filter = Arc::clone(&self.ignore_filter);
        let git_repo = self.git_repo.clone();
        let config = Arc::clone(&self.config);

        tokio::spawn(async move {
            while let Some(event) = notify_rx.recv().await {
                trace!("Received notify event: {:?}", event);

                // Convert notify events to our FileChange events
                if let Some(file_change) =
                    Self::convert_notify_event(event, &ignore_filter, git_repo.as_deref(), &config)
                        .await
                {
                    debouncer.process_event(file_change).await;
                }
            }
            debug!("Event processor stopped");
        });
    }

    /// Convert notify event to FileChange
    async fn convert_notify_event(
        event: NotifyEvent,
        ignore_filter: &IgnoreFilter,
        git_repo: Option<&GitRepository>,
        _config: &WatcherConfig,
    ) -> Option<FileChange> {
        // Check all paths in the event
        for path in &event.paths {
            // Check ignore patterns
            if ignore_filter.should_ignore(path) {
                trace!("Ignoring path: {:?}", path);
                continue;
            }

            // Check Git ignore if available
            if let Some(repo) = git_repo {
                if repo.should_ignore(path) {
                    trace!("Git ignoring path: {:?}", path);
                    continue;
                }
            }

            // Convert based on event kind
            match event.kind {
                EventKind::Create(_) => {
                    if let Ok(metadata) = tokio::fs::metadata(path).await {
                        if !ignore_filter.exceeds_size_limit(metadata.len()) {
                            let modified_time = match metadata.modified() {
                                Ok(time) => time,
                                Err(e) => {
                                    warn!("Failed to get modified time for {path:?}: {e}");
                                    continue;
                                }
                            };
                            let file_meta =
                                crate::events::FileMetadata::new(metadata.len(), modified_time, {
                                    #[cfg(unix)]
                                    {
                                        use std::os::unix::fs::PermissionsExt;
                                        metadata.permissions().mode()
                                    }
                                    #[cfg(not(unix))]
                                    {
                                        0o644
                                    }
                                });
                            return Some(FileChange::Created(path.clone(), file_meta));
                        }
                    }
                }
                EventKind::Modify(_) => {
                    // Check file size limit for modifications too
                    if let Ok(metadata) = tokio::fs::metadata(path).await {
                        if !ignore_filter.exceeds_size_limit(metadata.len()) {
                            // Create simple diff stats for now
                            let diff_stats = crate::events::DiffStats::new(vec![], vec![], vec![]);
                            return Some(FileChange::Modified(path.clone(), diff_stats));
                        }
                    }
                }
                EventKind::Remove(_) => {
                    return Some(FileChange::Deleted(path.clone()));
                }
                _ => {}
            }
        }

        None
    }

    /// Start monitoring for branch changes
    fn start_branch_monitor(&self, branch_watcher: Arc<RwLock<BranchWatcher>>) {
        let cancel_token = self.cancellation_token.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        info!("Branch monitor shutting down");
                        break;
                    }
                    _ = interval.tick() => {
                        let mut watcher = branch_watcher.write().await;
                        match watcher.has_branch_changed().await {
                            Ok(Some(change)) => {
                                info!("Branch changed from {} to {}", change.from, change.to);
                                // TODO: Trigger reindexing
                                debug!("Would trigger reindexing for branch change");
                            }
                            Ok(None) => {
                                // No change
                            }
                            Err(e) => {
                                error!("Error checking branch: {}", e);
                            }
                        }
                    }
                }
            }
        });
    }

    /// Stop watching all paths
    pub async fn stop(&mut self) -> Result<()> {
        self.cancellation_token.cancel();
        if let Some(_watcher) = self.watcher.take() {
            // Clear watched paths
            self.watched_paths.write().await.clear();
            info!("File watcher stopped");
        }
        Ok(())
    }

    /// Get currently watched paths
    pub async fn watched_paths(&self) -> Vec<PathBuf> {
        self.watched_paths.read().await.clone()
    }

    /// Check if a path is being watched
    pub async fn is_watching(&self, path: &Path) -> bool {
        self.watched_paths
            .read()
            .await
            .iter()
            .any(|p| path.starts_with(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup_test_watcher() -> (TempDir, FileWatcher) {
        let temp_dir = TempDir::new().expect("test setup failed");
        let config = WatcherConfig::default();
        let watcher = FileWatcher::new(config).expect("test setup failed");
        (temp_dir, watcher)
    }

    #[tokio::test]
    async fn test_watcher_initialization() {
        let (_temp_dir, watcher) = setup_test_watcher().await;
        assert!(watcher.watched_paths().await.is_empty());
    }

    #[tokio::test]
    async fn test_watch_path() {
        let (temp_dir, mut watcher) = setup_test_watcher().await;
        let _rx = watcher
            .watch(temp_dir.path())
            .await
            .expect("test setup failed");

        let watched = watcher.watched_paths().await;
        assert_eq!(watched.len(), 1);
        assert_eq!(watched[0], temp_dir.path());
    }

    #[tokio::test]
    async fn test_is_watching() {
        let (temp_dir, mut watcher) = setup_test_watcher().await;
        let _rx = watcher
            .watch(temp_dir.path())
            .await
            .expect("test setup failed");

        assert!(watcher.is_watching(temp_dir.path()).await);
        assert!(watcher.is_watching(&temp_dir.path().join("subdir")).await);
        assert!(!watcher.is_watching(Path::new("/other/path")).await);
    }
}
