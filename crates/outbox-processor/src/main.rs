#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use codesearch_core::error::Result;
use codesearch_outbox_processor::config::OutboxProcessorConfig;
use codesearch_outbox_processor::processor::OutboxProcessor;
use codesearch_storage::{create_postgres_client_from_config, create_storage_client_from_config};
use std::time::Duration;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("Starting outbox processor");

    let config = OutboxProcessorConfig::load_from_env()?;

    info!(
        "Connecting to Postgres at {}:{}",
        config.postgres.host, config.postgres.port
    );
    let postgres_client = match create_postgres_client_from_config(&config.postgres).await {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to connect to Postgres: {e}");
            return Err(e);
        }
    };

    info!("Running database migrations");
    if let Err(e) = postgres_client.run_migrations().await {
        error!("Failed to run database migrations: {e}");
        return Err(e);
    }
    info!("Database migrations completed successfully");

    info!(
        "Connecting to Qdrant at {}:{}",
        config.qdrant.host, config.qdrant.port
    );
    // Verify Qdrant is reachable by creating a test client
    match create_storage_client_from_config(&config.qdrant, "test_connection").await {
        Ok(_) => info!("Qdrant connection verified"),
        Err(e) => {
            error!("Failed to connect to Qdrant: {e}");
            return Err(e);
        }
    }

    let processor = OutboxProcessor::new(
        postgres_client,
        config.qdrant.clone(),
        Duration::from_millis(config.poll_interval_ms),
        config.batch_size,
        config.max_retries,
        config.max_embedding_dim,
    );

    info!("Outbox processor configuration loaded successfully");

    processor.start().await?;

    Ok(())
}
