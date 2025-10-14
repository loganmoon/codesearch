use codesearch_storage::{PostgresConfig, QdrantConfig};

/// Configuration for the outbox processor
#[derive(Debug, Clone)]
pub struct OutboxProcessorConfig {
    pub postgres: PostgresConfig,
    pub qdrant: QdrantConfig,
    pub poll_interval_ms: u64,
    pub batch_size: i64,
    pub max_retries: i32,
    pub max_embedding_dim: usize,
}

impl OutboxProcessorConfig {
    /// Load configuration from environment variables
    pub fn load_from_env() -> Self {
        let postgres = PostgresConfig {
            host: std::env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: std::env::var("POSTGRES_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(5432),
            database: std::env::var("POSTGRES_DATABASE")
                .unwrap_or_else(|_| "codesearch".to_string()),
            user: std::env::var("POSTGRES_USER").unwrap_or_else(|_| "codesearch".to_string()),
            password: std::env::var("POSTGRES_PASSWORD")
                .unwrap_or_else(|_| "codesearch".to_string()),
            max_entity_batch_size: std::env::var("MAX_ENTITY_BATCH_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1000),
        };

        let qdrant = QdrantConfig {
            host: std::env::var("QDRANT_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: std::env::var("QDRANT_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(6334),
            rest_port: std::env::var("QDRANT_REST_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(6333),
        };

        Self {
            postgres,
            qdrant,
            poll_interval_ms: std::env::var("POLL_INTERVAL_MS")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(1000),
            batch_size: std::env::var("BATCH_SIZE")
                .ok()
                .and_then(|b| b.parse().ok())
                .unwrap_or(100),
            max_retries: std::env::var("MAX_RETRIES")
                .ok()
                .and_then(|r| r.parse().ok())
                .unwrap_or(3),
            max_embedding_dim: std::env::var("MAX_EMBEDDING_DIM")
                .ok()
                .and_then(|d| d.parse().ok())
                .unwrap_or(100_000),
        }
    }
}
