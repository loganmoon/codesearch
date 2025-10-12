#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use codesearch_core::config::{Config, StorageConfig};
use codesearch_core::error::Result;
use codesearch_outbox_processor::processor::OutboxProcessor;
use codesearch_storage::{create_postgres_client, create_storage_client};
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

    let config = load_config_from_env()?;

    info!(
        "Connecting to Postgres at {}:{}",
        config.storage.postgres_host, config.storage.postgres_port
    );
    let postgres_client = match create_postgres_client(&config.storage).await {
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
        config.storage.qdrant_host, config.storage.qdrant_port
    );
    // Verify Qdrant is reachable by creating a test client
    match create_storage_client(&config.storage, "test_connection").await {
        Ok(_) => info!("Qdrant connection verified"),
        Err(e) => {
            error!("Failed to connect to Qdrant: {e}");
            return Err(e);
        }
    }

    let qdrant_config = codesearch_storage::QdrantConfig {
        host: config.storage.qdrant_host.clone(),
        port: config.storage.qdrant_port,
        rest_port: config.storage.qdrant_rest_port,
    };

    let processor = OutboxProcessor::new(
        postgres_client,
        qdrant_config,
        Duration::from_millis(1000), // Poll every 1s
        100,                         // Batch size
        3,                           // Max retries
    );

    info!("Outbox processor configuration loaded successfully");

    processor.start().await?;

    Ok(())
}

fn load_config_from_env() -> Result<Config> {
    let storage_config = StorageConfig {
        postgres_host: std::env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string()),
        postgres_port: std::env::var("POSTGRES_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(5432),
        postgres_database: std::env::var("POSTGRES_DATABASE")
            .unwrap_or_else(|_| "codesearch".to_string()),
        postgres_user: std::env::var("POSTGRES_USER").unwrap_or_else(|_| "codesearch".to_string()),
        postgres_password: std::env::var("POSTGRES_PASSWORD")
            .unwrap_or_else(|_| "codesearch".to_string()),
        qdrant_host: std::env::var("QDRANT_HOST").unwrap_or_else(|_| "localhost".to_string()),
        qdrant_port: std::env::var("QDRANT_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(6334),
        qdrant_rest_port: std::env::var("QDRANT_REST_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(6333),
        collection_name: std::env::var("QDRANT_COLLECTION")
            .unwrap_or_else(|_| "codesearch".to_string()),
        auto_start_deps: false,
        docker_compose_file: None,
        max_entity_batch_size: std::env::var("MAX_ENTITY_BATCH_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000),
    };

    Ok(Config::builder(storage_config).build())
}
