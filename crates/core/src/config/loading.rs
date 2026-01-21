//! Configuration loading from files and environment variables

use crate::error::{Error, Result};
use config::{Config as ConfigLib, ConfigBuilder as LibConfigBuilder, Environment, File};
use std::path::Path;

use super::defaults::*;
use super::{global_config_path, Config};

/// Helper to set a config default with consistent error mapping
fn set_config_default<T: Into<config::Value>>(
    builder: LibConfigBuilder<config::builder::DefaultState>,
    key: &str,
    value: T,
) -> Result<LibConfigBuilder<config::builder::DefaultState>> {
    builder
        .set_default(key, value)
        .map_err(|e| Error::config(format!("Failed to set {key} default: {e}")))
}

impl Config {
    /// Loads configuration from a TOML file with environment variable overrides
    ///
    /// Environment variables are prefixed with `CODESEARCH_` and use double underscores
    /// for nested values. For example:
    /// - `CODESEARCH_EMBEDDINGS__PROVIDER=openai`
    pub fn from_file(path: &Path) -> Result<Self> {
        let builder = ConfigLib::builder();

        // Set outbox defaults explicitly (config crate doesn't apply serde defaults for missing sections)
        let builder = set_config_default(
            builder,
            "outbox.poll_interval_ms",
            default_outbox_poll_interval_ms() as i64,
        )?;
        let builder = set_config_default(
            builder,
            "outbox.entries_per_poll",
            default_outbox_entries_per_poll(),
        )?;
        let builder = set_config_default(
            builder,
            "outbox.max_retries",
            default_outbox_max_retries() as i64,
        )?;
        let builder = set_config_default(
            builder,
            "outbox.max_embedding_dim",
            default_outbox_max_embedding_dim() as i64,
        )?;
        let builder = set_config_default(
            builder,
            "outbox.max_cached_collections",
            default_outbox_max_cached_collections() as i64,
        )?;
        let builder = set_config_default(
            builder,
            "outbox.drain_timeout_secs",
            default_outbox_drain_timeout_secs() as i64,
        )?;

        // Reranking defaults
        let builder = set_config_default(builder, "reranking.enabled", default_enable_reranking())?;
        let builder =
            set_config_default(builder, "reranking.provider", default_reranking_provider())?;
        let builder = set_config_default(builder, "reranking.model", default_reranking_model())?;
        let builder = set_config_default(
            builder,
            "reranking.candidates",
            default_reranking_candidates() as i64,
        )?;
        let builder =
            set_config_default(builder, "reranking.top_k", default_reranking_top_k() as i64)?;
        let builder = set_config_default(
            builder,
            "reranking.timeout_secs",
            default_reranking_timeout_secs() as i64,
        )?;
        let mut builder = set_config_default(
            builder,
            "reranking.max_concurrent_requests",
            default_reranking_max_concurrent_requests() as i64,
        )?;

        // Add the config file if it exists
        if path.exists() {
            builder = builder.add_source(File::from(path));
        }

        // Add environment variables with CODESEARCH_ prefix
        builder = builder.add_source(
            Environment::with_prefix("CODESEARCH")
                .separator("__")
                .try_parsing(true),
        );

        // Support backward-compatible environment variables for storage
        if let Ok(host) = std::env::var("QDRANT_HOST") {
            builder = builder
                .set_override("storage.qdrant_host", host)
                .map_err(|e| Error::config(format!("Failed to set QDRANT_HOST: {e}")))?;
        }
        if let Ok(port) = std::env::var("QDRANT_PORT") {
            if let Ok(port_num) = port.parse::<u16>() {
                builder = builder
                    .set_override("storage.qdrant_port", port_num)
                    .map_err(|e| Error::config(format!("Failed to set QDRANT_PORT: {e}")))?;
            }
        }
        if let Ok(port) = std::env::var("QDRANT_REST_PORT") {
            if let Ok(port_num) = port.parse::<u16>() {
                builder = builder
                    .set_override("storage.qdrant_rest_port", port_num)
                    .map_err(|e| Error::config(format!("Failed to set QDRANT_REST_PORT: {e}")))?;
            }
        }

        // Support Postgres environment variables
        if let Ok(host) = std::env::var("POSTGRES_HOST") {
            builder = builder
                .set_override("storage.postgres_host", host)
                .map_err(|e| Error::config(format!("Failed to set POSTGRES_HOST: {e}")))?;
        }
        if let Ok(port) = std::env::var("POSTGRES_PORT") {
            if let Ok(port_num) = port.parse::<u16>() {
                builder = builder
                    .set_override("storage.postgres_port", port_num)
                    .map_err(|e| Error::config(format!("Failed to set POSTGRES_PORT: {e}")))?;
            }
        }
        if let Ok(db) = std::env::var("POSTGRES_DATABASE") {
            builder = builder
                .set_override("storage.postgres_database", db)
                .map_err(|e| Error::config(format!("Failed to set POSTGRES_DATABASE: {e}")))?;
        }
        if let Ok(user) = std::env::var("POSTGRES_USER") {
            builder = builder
                .set_override("storage.postgres_user", user)
                .map_err(|e| Error::config(format!("Failed to set POSTGRES_USER: {e}")))?;
        }
        if let Ok(password) = std::env::var("POSTGRES_PASSWORD") {
            builder = builder
                .set_override("storage.postgres_password", password)
                .map_err(|e| Error::config(format!("Failed to set POSTGRES_PASSWORD: {e}")))?;
        }

        // Neo4j configuration
        if let Ok(host) = std::env::var("NEO4J_HOST") {
            builder = builder
                .set_override("storage.neo4j_host", host)
                .map_err(|e| Error::config(format!("Failed to set NEO4J_HOST: {e}")))?;
        }
        if let Ok(port) = std::env::var("NEO4J_BOLT_PORT") {
            if let Ok(port_num) = port.parse::<u16>() {
                builder = builder
                    .set_override("storage.neo4j_bolt_port", port_num)
                    .map_err(|e| Error::config(format!("Failed to set NEO4J_BOLT_PORT: {e}")))?;
            }
        }
        if let Ok(password) = std::env::var("NEO4J_PASSWORD") {
            builder = builder
                .set_override("storage.neo4j_password", password)
                .map_err(|e| Error::config(format!("Failed to set NEO4J_PASSWORD: {e}")))?;
        }

        // Support indexer environment variables
        if let Ok(batch_size) = std::env::var("CODESEARCH_INDEXER__FILES_PER_DISCOVERY_BATCH") {
            if let Ok(size) = batch_size.parse::<i64>() {
                builder = builder
                    .set_override("indexer.files_per_discovery_batch", size)
                    .map_err(|e| {
                        Error::config(format!("Failed to set files_per_discovery_batch: {e}"))
                    })?;
            }
        }

        if let Ok(buffer_size) = std::env::var("CODESEARCH_INDEXER__PIPELINE_CHANNEL_CAPACITY") {
            if let Ok(size) = buffer_size.parse::<i64>() {
                builder = builder
                    .set_override("indexer.pipeline_channel_capacity", size)
                    .map_err(|e| {
                        Error::config(format!("Failed to set pipeline_channel_capacity: {e}"))
                    })?;
            }
        }

        if let Ok(entity_batch) = std::env::var("CODESEARCH_INDEXER__ENTITIES_PER_EMBEDDING_BATCH")
        {
            if let Ok(size) = entity_batch.parse::<i64>() {
                builder = builder
                    .set_override("indexer.entities_per_embedding_batch", size)
                    .map_err(|e| {
                        Error::config(format!("Failed to set entities_per_embedding_batch: {e}"))
                    })?;
            }
        }

        if let Ok(concurrency) =
            std::env::var("CODESEARCH_INDEXER__MAX_CONCURRENT_FILE_EXTRACTIONS")
        {
            if let Ok(val) = concurrency.parse::<i64>() {
                builder = builder
                    .set_override("indexer.max_concurrent_file_extractions", val)
                    .map_err(|e| {
                        Error::config(format!(
                            "Failed to set max_concurrent_file_extractions: {e}"
                        ))
                    })?;
            }
        }

        if let Ok(concurrency) =
            std::env::var("CODESEARCH_INDEXER__MAX_CONCURRENT_SNAPSHOT_UPDATES")
        {
            if let Ok(val) = concurrency.parse::<i64>() {
                builder = builder
                    .set_override("indexer.max_concurrent_snapshot_updates", val)
                    .map_err(|e| {
                        Error::config(format!(
                            "Failed to set max_concurrent_snapshot_updates: {e}"
                        ))
                    })?;
            }
        }

        let config = builder
            .build()
            .map_err(|e| Error::config(format!("Failed to build config: {e}")))?;

        config
            .try_deserialize()
            .map_err(|e| Error::config(format!("Failed to deserialize config: {e}")))
    }

    /// Creates a config from a TOML string (useful for testing)
    pub fn from_toml_str(content: &str) -> Result<Self> {
        toml::from_str(content).map_err(|e| Error::config(format!("Failed to parse TOML: {e}")))
    }

    /// Load configuration from a single file
    ///
    /// Precedence (lowest to highest):
    /// 1. Hardcoded defaults
    /// 2. Config file (~/.codesearch/config.toml or custom --config path)
    /// 3. Environment variables (CODESEARCH_*)
    pub fn load(config_path: Option<&Path>) -> Result<Self> {
        let path = match config_path {
            Some(p) => p.to_path_buf(),
            None => global_config_path()?,
        };
        Self::from_file(&path)
    }
}
