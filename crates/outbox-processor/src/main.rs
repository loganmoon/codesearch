mod processor;

use codesearch_core::config::Config;
use codesearch_core::error::Result;
use codesearch_storage::{create_postgres_client, create_storage_client};
use processor::OutboxProcessor;
use std::time::Duration;
use tracing::info;

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
    let postgres_client = create_postgres_client(&config.storage).await?;

    info!(
        "Connecting to Qdrant at {}:{} (collection: {})",
        config.storage.qdrant_host, config.storage.qdrant_port, config.storage.collection_name
    );
    let storage_client =
        create_storage_client(&config.storage, &config.storage.collection_name).await?;

    let processor = OutboxProcessor::new(
        postgres_client,
        storage_client,
        Duration::from_millis(1000), // Poll every 1s
        100,                         // Batch size
        3,                           // Max retries
    );

    info!("Outbox processor configuration loaded successfully");

    processor.start().await?;

    Ok(())
}

fn load_config_from_env() -> Result<Config> {
    let postgres_host = std::env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string());
    let postgres_port = std::env::var("POSTGRES_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(5432);
    let postgres_database =
        std::env::var("POSTGRES_DATABASE").unwrap_or_else(|_| "codesearch".to_string());
    let postgres_user = std::env::var("POSTGRES_USER").unwrap_or_else(|_| "codesearch".to_string());
    let postgres_password =
        std::env::var("POSTGRES_PASSWORD").unwrap_or_else(|_| "codesearch".to_string());

    let qdrant_host = std::env::var("QDRANT_HOST").unwrap_or_else(|_| "localhost".to_string());
    let qdrant_port = std::env::var("QDRANT_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(6334);
    let qdrant_rest_port = std::env::var("QDRANT_REST_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(6333);
    let collection_name =
        std::env::var("QDRANT_COLLECTION").unwrap_or_else(|_| "codesearch".to_string());

    let config_toml = format!(
        r#"
[indexer]

[storage]
qdrant_host = "{qdrant_host}"
qdrant_port = {qdrant_port}
qdrant_rest_port = {qdrant_rest_port}
collection_name = "{collection_name}"
auto_start_deps = false
postgres_host = "{postgres_host}"
postgres_port = {postgres_port}
postgres_database = "{postgres_database}"
postgres_user = "{postgres_user}"
postgres_password = "{postgres_password}"

[embeddings]
provider = "mock"

[watcher]
debounce_ms = 500
ignore_patterns = ["*.log", "target", ".git"]
branch_strategy = "index_current"

[languages]
enabled = ["rust", "python", "javascript", "typescript", "go"]
"#
    );

    Config::from_toml_str(&config_toml)
}
