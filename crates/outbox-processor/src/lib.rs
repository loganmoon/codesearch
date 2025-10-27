#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

pub mod processor;

use codesearch_core::config::OutboxConfig;
use codesearch_core::error::Result;
use codesearch_storage::{PostgresClientTrait, QdrantConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info};

// Re-export for ease of use
pub use processor::OutboxProcessor;

/// Public API for starting outbox processor
///
/// This function starts the outbox processor in an embedded mode, allowing it to run
/// as a background task within another process (e.g., the server).
///
/// # Arguments
/// * `postgres_client` - Arc to PostgreSQL client implementing PostgresClientTrait
/// * `qdrant_config` - Configuration for connecting to Qdrant
/// * `config` - Outbox processor configuration (poll interval, batch size, etc.)
/// * `shutdown_rx` - Oneshot receiver for graceful shutdown signal
///
/// # Graceful Shutdown
/// When the shutdown signal is received via `shutdown_rx`, the processor will:
/// 1. Complete the current in-flight batch processing (if any)
/// 2. Check the shutdown flag before starting the next batch
/// 3. Return Ok(()) to indicate clean shutdown
///
/// The shutdown is graceful in the sense that it will not interrupt an ongoing
/// batch operation, but will exit cleanly after the current batch completes.
///
/// # Error Handling
/// Processing errors are logged but do not stop the processor. Only shutdown signals
/// or critical infrastructure failures (if any) will cause the function to return.
pub async fn start_outbox_processor(
    postgres_client: Arc<dyn PostgresClientTrait>,
    qdrant_config: &QdrantConfig,
    config: &OutboxConfig,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<()> {
    let processor = OutboxProcessor::new(
        postgres_client,
        qdrant_config.clone(),
        Duration::from_millis(config.poll_interval_ms),
        config.entries_per_poll,
        config.max_retries,
        config.max_embedding_dim,
        config.max_cached_collections as u64,
    );

    info!("Outbox processor started");

    // Use a watch channel to allow graceful shutdown that completes current batch
    let (shutdown_flag_tx, mut shutdown_flag_rx) = tokio::sync::watch::channel(false);

    // Spawn task to listen for shutdown signal and set the flag
    tokio::spawn(async move {
        let _ = shutdown_rx.await;
        let _ = shutdown_flag_tx.send(true);
    });

    loop {
        // Check shutdown flag before starting next batch
        if *shutdown_flag_rx.borrow() {
            info!("Outbox processor shutting down gracefully");
            return Ok(());
        }

        // Process batch (will complete even if shutdown signal arrives during processing)
        if let Err(e) = processor.process_batch().await {
            error!("Outbox batch processing error: {e}");
            // Continue processing despite errors
        }

        // Use select for the sleep to allow early exit on shutdown
        tokio::select! {
            _ = sleep(Duration::from_millis(config.poll_interval_ms)) => {},
            _ = shutdown_flag_rx.changed() => {
                // Shutdown signal received during sleep, exit after current batch completed
                info!("Outbox processor shutting down gracefully");
                return Ok(());
            }
        }
    }
}
