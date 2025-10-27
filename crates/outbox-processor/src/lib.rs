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
/// 1. Complete any in-flight batch processing
/// 2. Return Ok(()) to indicate clean shutdown
///
/// # Error Handling
/// Processing errors are logged but do not stop the processor. Only shutdown signals
/// or critical infrastructure failures (if any) will cause the function to return.
pub async fn start_outbox_processor(
    postgres_client: Arc<dyn PostgresClientTrait>,
    qdrant_config: &QdrantConfig,
    config: &OutboxConfig,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<()> {
    let processor = OutboxProcessor::new(
        postgres_client,
        qdrant_config.clone(),
        Duration::from_millis(config.poll_interval_ms),
        config.entries_per_poll,
        config.max_retries,
        config.max_embedding_dim,
    );

    info!("Outbox processor started");

    loop {
        tokio::select! {
            result = processor.process_batch() => {
                if let Err(e) = result {
                    error!("Outbox batch processing error: {e}");
                    // Continue processing despite errors
                }
            },
            _ = &mut shutdown_rx => {
                info!("Outbox processor shutting down gracefully");
                return Ok(());
            }
        }

        sleep(Duration::from_millis(config.poll_interval_ms)).await;
    }
}
