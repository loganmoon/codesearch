//! Stage 1: File discovery
//!
//! Discovers all files in the repository and streams them in batches for processing.

use crate::repository_indexer::batches::FileBatch;
use anyhow::anyhow;
use codesearch_core::error::{Error, Result};
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Stage 1: Discover all files in the repository and stream them in batches
///
/// This function implements streaming file discovery with the following optimizations:
/// - **Parallel traversal**: Uses multiple threads (auto-detected, capped at 12, defaults to 4 if detection fails) for faster discovery
/// - **Gitignore support**: Automatically respects `.gitignore`, `.git/info/exclude`, and global ignore files
/// - **Streaming batches**: Sends batches to downstream stages as they're discovered, enabling
///   pipeline parallelism where Stage 2 (entity extraction) begins processing files before
///   Stage 1 completes discovery
/// - **Memory efficiency**: Only keeps one batch in memory at a time, rather than all file paths
/// - **Lock-free architecture**: Uses channels instead of shared mutable state (Arc<Mutex<Vec>>)
///
/// Benefits over collect-then-batch approach:
/// - Reduced time-to-first-extraction: Downstream stages start immediately
/// - Better CPU utilization: All pipeline stages can run concurrently
/// - Lower peak memory usage: No need to hold all paths in memory
/// - No mutex contention between walker threads
pub(crate) async fn stage_file_discovery(
    file_tx: mpsc::Sender<FileBatch>,
    repo_path: PathBuf,
    batch_size: usize,
) -> Result<usize> {
    use ignore::WalkBuilder;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // Calculate parallelism: min(available cores, 12)
    // Higher cap for I/O-bound file discovery (benefits from concurrency on modern SSDs)
    let parallelism = std::thread::available_parallelism()
        .map(|n| n.get().min(12))
        .unwrap_or(4);

    debug!(
        "Streaming file discovery using {} threads for {}",
        parallelism,
        repo_path.display()
    );

    // Create bounded channel for individual paths from walker threads
    // Capacity of batch_size * 2 provides buffering while preventing unbounded memory growth
    // Walker threads will apply backpressure if coordinator falls behind
    let (path_tx, mut path_rx) = mpsc::channel::<PathBuf>(batch_size * 2);
    let total_files = Arc::new(AtomicUsize::new(0));

    // Spawn coordinator task to batch individual paths
    let batch_tx = file_tx.clone();
    let total_for_coordinator = Arc::clone(&total_files);
    let coordinator = tokio::spawn(async move {
        let mut current_batch = Vec::with_capacity(batch_size);

        while let Some(path) = path_rx.recv().await {
            current_batch.push(path);
            total_for_coordinator.fetch_add(1, Ordering::Relaxed);

            // Send batch when it reaches batch_size
            if current_batch.len() >= batch_size {
                let batch = std::mem::replace(&mut current_batch, Vec::with_capacity(batch_size));
                if let Err(e) = batch_tx.send(FileBatch { paths: batch }).await {
                    warn!("Failed to send file batch: {}", e);
                    break;
                }
            }
        }

        // Send any remaining files in the last batch
        if !current_batch.is_empty() {
            if let Err(e) = batch_tx
                .send(FileBatch {
                    paths: current_batch,
                })
                .await
            {
                warn!("Failed to send final file batch: {}", e);
            }
        }
    });

    // Build parallel walker with gitignore support
    // Run in blocking task since WalkBuilder::run is synchronous
    let walk_handle = tokio::task::spawn_blocking(move || {
        WalkBuilder::new(&repo_path)
            .standard_filters(true)
            .hidden(false)
            .parents(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .require_git(false)
            .threads(parallelism)
            .build_parallel()
            .run(|| {
                let tx = path_tx.clone();

                Box::new(move |entry_result| {
                    use crate::common::{has_supported_extension, should_include_file};
                    use ignore::WalkState;

                    match entry_result {
                        Ok(entry) => {
                            let path = entry.path();

                            // Apply filters in order of cost (cheap to expensive)
                            // 1. Check file type first (already cached in DirEntry, free)
                            if let Some(file_type) = entry.file_type() {
                                if !file_type.is_file() {
                                    return WalkState::Continue;
                                }
                            }

                            // 2. Check extension (cheap string operation)
                            if !has_supported_extension(path) {
                                return WalkState::Continue;
                            }

                            // 3. Check symlink/size (requires metadata syscall)
                            if !should_include_file(path) {
                                return WalkState::Continue;
                            }

                            // Send path to coordinator for batching
                            // Use blocking_send since we're in a sync context with bounded channel
                            if let Err(e) = tx.blocking_send(path.to_path_buf()) {
                                warn!("Failed to send path to coordinator: {}", e);
                                return WalkState::Quit;
                            }

                            WalkState::Continue
                        }
                        Err(e) => {
                            warn!("Error reading file entry: {}", e);
                            WalkState::Continue
                        }
                    }
                })
            });
    });

    // Wait for walker to complete
    // When this completes, path_tx is automatically dropped, signaling coordinator
    walk_handle
        .await
        .map_err(|e| Error::Other(anyhow!("Walker task panicked: {e}")))?;

    // Wait for coordinator to finish sending all batches
    coordinator
        .await
        .map_err(|e| Error::Other(anyhow!("Coordinator task panicked: {e}")))?;

    let total = total_files.load(Ordering::Relaxed);
    info!("Discovered {total} files to index");
    Ok(total)
}
