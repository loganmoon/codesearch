use codesearch_core::error::{Error, Result};
use codesearch_storage::{PostgresConfig, QdrantConfig};

/// Validate a hostname to prevent host injection and SSRF attacks
///
/// Ensures the hostname does not contain:
/// - Protocol separators (://)
/// - User credentials (@)
/// - Path separators (/)
fn validate_hostname(host: &str) -> Result<()> {
    if host.contains("://") || host.contains('@') || host.contains('/') {
        return Err(Error::config(format!(
            "Invalid hostname '{host}': contains forbidden characters"
        )));
    }
    if host.is_empty() {
        return Err(Error::config("Hostname cannot be empty".to_string()));
    }
    Ok(())
}

/// Validate a database name
///
/// Ensures the database name:
/// - Contains only alphanumeric characters, underscores, and hyphens
/// - Does not exceed PostgreSQL's 63-character limit
fn validate_database_name(name: &str) -> Result<()> {
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(Error::config(format!(
            "Invalid database name '{name}': only alphanumeric, underscore, and hyphen allowed"
        )));
    }
    if name.len() > 63 {
        return Err(Error::config(
            "Database name exceeds PostgreSQL's 63-character limit".to_string(),
        ));
    }
    if name.is_empty() {
        return Err(Error::config("Database name cannot be empty".to_string()));
    }
    Ok(())
}

/// Configuration for the outbox processor
#[derive(Debug, Clone)]
pub struct OutboxProcessorConfig {
    pub postgres: PostgresConfig,
    pub qdrant: QdrantConfig,
    pub database_poll_interval_ms: u64,
    pub entries_per_poll: i64,
    pub max_retries: i32,
    pub max_embedding_dim: usize,
}

impl OutboxProcessorConfig {
    /// Load configuration from environment variables
    ///
    /// Reads the following environment variables with their defaults:
    /// - `POSTGRES_HOST` (default: "localhost") - PostgreSQL server hostname
    /// - `POSTGRES_PORT` (default: 5432) - PostgreSQL server port
    /// - `POSTGRES_DATABASE` (default: "codesearch") - PostgreSQL database name
    /// - `POSTGRES_USER` (default: "codesearch") - PostgreSQL username
    /// - `POSTGRES_PASSWORD` (default: "codesearch") - PostgreSQL password
    /// - `MAX_ENTITIES_PER_DB_OPERATION` (default: 1000) - Maximum entities per database operation
    /// - `QDRANT_HOST` (default: "localhost") - Qdrant server hostname
    /// - `QDRANT_PORT` (default: 6334) - Qdrant gRPC port
    /// - `QDRANT_REST_PORT` (default: 6333) - Qdrant REST API port
    /// - `DATABASE_POLL_INTERVAL_MS` (default: 1000) - Outbox polling interval in milliseconds
    /// - `ENTRIES_PER_POLL` (default: 100) - Number of outbox entries to fetch per poll
    /// - `MAX_RETRIES` (default: 3) - Maximum retry attempts for failed operations
    /// - `MAX_EMBEDDING_DIM` (default: 100000) - Maximum embedding dimension size
    ///
    /// # Validation
    ///
    /// Validates hostnames and database names to prevent injection attacks.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails for any hostname or database name.
    pub fn load_from_env() -> Result<Self> {
        // Load values from environment
        let postgres_host =
            std::env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string());
        let postgres_database =
            std::env::var("POSTGRES_DATABASE").unwrap_or_else(|_| "codesearch".to_string());
        let qdrant_host = std::env::var("QDRANT_HOST").unwrap_or_else(|_| "localhost".to_string());

        // Validate inputs
        validate_hostname(&postgres_host)?;
        validate_hostname(&qdrant_host)?;
        validate_database_name(&postgres_database)?;

        let postgres = PostgresConfig {
            host: postgres_host,
            port: std::env::var("POSTGRES_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(5432),
            database: postgres_database,
            user: std::env::var("POSTGRES_USER").unwrap_or_else(|_| "codesearch".to_string()),
            password: std::env::var("POSTGRES_PASSWORD")
                .unwrap_or_else(|_| "codesearch".to_string()),
            max_entities_per_db_operation: std::env::var("MAX_ENTITIES_PER_DB_OPERATION")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1000),
        };

        let qdrant = QdrantConfig {
            host: qdrant_host,
            port: std::env::var("QDRANT_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(6334),
            rest_port: std::env::var("QDRANT_REST_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(6333),
        };

        Ok(Self {
            postgres,
            qdrant,
            database_poll_interval_ms: std::env::var("DATABASE_POLL_INTERVAL_MS")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(1000),
            entries_per_poll: std::env::var("ENTRIES_PER_POLL")
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
        })
    }
}
