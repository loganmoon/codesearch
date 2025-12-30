#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

pub mod generic_resolver;
pub mod neo4j_relationship_resolver;
pub mod processor;

use codesearch_core::config::OutboxConfig;
use codesearch_core::error::Result;
use codesearch_storage::{PostgresClientTrait, QdrantConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info};

// Re-export for ease of use
pub use generic_resolver::{
    associates_resolver, calls_resolver, extends_resolver, implements_resolver, imports_resolver,
    inherits_resolver, uses_resolver, GenericResolver,
};
pub use neo4j_relationship_resolver::{
    resolve_external_references, resolve_relationships_generic, CallGraphResolver,
    ContainsResolver, EntityCache, ImportsResolver, InheritanceResolver, RelationshipResolver,
    TraitImplResolver, TypeUsageResolver,
};
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
    storage_config: codesearch_core::StorageConfig,
    config: &OutboxConfig,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<()> {
    let processor = OutboxProcessor::new(
        postgres_client,
        qdrant_config.clone(),
        storage_config,
        Duration::from_millis(config.poll_interval_ms),
        config.entries_per_poll,
        config.max_retries,
        config.max_embedding_dim,
        config.max_cached_collections as u64,
    );

    info!(
        entries_per_poll = config.entries_per_poll,
        poll_interval_ms = config.poll_interval_ms,
        "Outbox processor started"
    );

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

/// Start outbox processor with drain mode support
///
/// This function starts the outbox processor and runs concurrently with other operations.
/// When the drain signal is received, it continues processing until the outbox is completely
/// empty, then exits cleanly.
///
/// # Arguments
/// * `postgres_client` - Arc to PostgreSQL client implementing PostgresClientTrait
/// * `qdrant_config` - Configuration for connecting to Qdrant
/// * `storage_config` - Storage configuration
/// * `config` - Outbox processor configuration (poll interval, batch size, etc.)
/// * `drain_rx` - Oneshot receiver that signals when to enter drain mode
///
/// # Drain Mode Behavior
/// When the drain signal is received:
/// 1. The processor continues processing batches
/// 2. After each batch, it checks if the outbox is empty
/// 3. When empty, it returns Ok(()) indicating successful drain
///
/// This is useful for `codesearch index` where we want to run the outbox processor
/// concurrently during indexing, then drain any remaining entries after indexing completes.
pub async fn start_outbox_processor_with_drain(
    postgres_client: Arc<dyn PostgresClientTrait>,
    qdrant_config: &QdrantConfig,
    storage_config: codesearch_core::StorageConfig,
    config: &OutboxConfig,
    drain_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<()> {
    let processor = OutboxProcessor::new(
        Arc::clone(&postgres_client),
        qdrant_config.clone(),
        storage_config,
        Duration::from_millis(config.poll_interval_ms),
        config.entries_per_poll,
        config.max_retries,
        config.max_embedding_dim,
        config.max_cached_collections as u64,
    );

    info!(
        entries_per_poll = config.entries_per_poll,
        poll_interval_ms = config.poll_interval_ms,
        drain_timeout_secs = config.drain_timeout_secs,
        "Outbox processor started (with drain mode support)"
    );

    // Use a watch channel to track drain mode
    let (drain_flag_tx, mut drain_flag_rx) = tokio::sync::watch::channel(false);

    // Spawn task to listen for drain signal
    tokio::spawn(async move {
        let _ = drain_rx.await;
        let _ = drain_flag_tx.send(true);
    });

    loop {
        // Process batch
        let mut work_done = false;
        match processor.process_batch().await {
            Ok(had_work) => {
                work_done = had_work;
            }
            Err(e) => {
                error!("Outbox batch processing error: {e}");
                // Continue processing despite errors
            }
        }

        // Check if we're in drain mode
        let in_drain_mode = *drain_flag_rx.borrow();

        if in_drain_mode {
            // Optimization: If we processed a batch, immediately try to process another
            // without sleep or expensive count check
            if work_done {
                continue;
            }

            // In drain mode: check if outbox is empty
            match postgres_client.count_pending_outbox_entries().await {
                Ok(0) => {
                    // Resolve all pending relationships now that all entities are indexed
                    info!("Outbox drained. Resolving pending relationships...");
                    processor
                        .resolve_pending_relationships()
                        .await
                        .map_err(|e| {
                            error!("Failed to resolve pending relationships: {e}");
                            e
                        })?;
                    info!("Outbox drained and relationships resolved, processor exiting");
                    return Ok(());
                }
                Ok(count) => {
                    info!("Drain mode: {} entries remaining", count);
                    // Short sleep before next batch in drain mode
                    sleep(Duration::from_millis(100)).await;
                }
                Err(e) => {
                    error!("Failed to count pending outbox entries: {e}");
                    // Continue trying
                    sleep(Duration::from_millis(config.poll_interval_ms)).await;
                }
            }
        } else {
            // Normal mode: use select for the sleep to detect drain signal
            tokio::select! {
                _ = sleep(Duration::from_millis(config.poll_interval_ms)) => {},
                _ = drain_flag_rx.changed() => {
                    info!("Drain signal received, will process until outbox is empty");
                    // Continue loop - drain mode is now active
                }
            }
        }
    }
}
