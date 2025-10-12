//! Docker dependency management for codesearch

use anyhow::{anyhow, Context, Result};
use codesearch_core::config::StorageConfig;
use sqlx::postgres::PgConnectOptions;
use std::process::Command;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{info, warn};

/// Generic helper to check if a container is running
fn is_container_running(container_name: &str) -> Result<bool> {
    let filter_arg = format!("name={container_name}");

    let output = Command::new("docker")
        .args(["ps", "--filter", &filter_arg, "--format", "{{.Names}}"])
        .output()
        .context("Failed to check container status")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Docker ps command failed: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.contains(container_name))
}

/// Check if Qdrant container is running
pub fn is_qdrant_running() -> Result<bool> {
    is_container_running("codesearch-qdrant")
}

/// Check if vLLM container is running
pub fn is_vllm_running() -> Result<bool> {
    is_container_running("codesearch-vllm")
}

/// Check if Postgres container is running
pub fn is_postgres_running() -> Result<bool> {
    is_container_running("codesearch-postgres")
}

/// Check if Outbox Processor container is running
pub fn is_outbox_processor_running() -> Result<bool> {
    is_container_running("codesearch-outbox-processor")
}

/// Check Qdrant health status
pub async fn check_qdrant_health(config: &StorageConfig) -> bool {
    // Qdrant doesn't have a /health endpoint, but the root endpoint returns version info
    let url = format!("http://{}:{}/", config.qdrant_host, config.qdrant_rest_port);

    match reqwest::get(&url).await {
        Ok(response) => {
            let is_success = response.status().is_success();
            if !is_success {
                warn!(
                    "Qdrant health check failed: HTTP {} at {}",
                    response.status(),
                    url
                );
            }
            is_success
        }
        Err(e) => {
            warn!("Qdrant health check failed: unable to connect to {url}: {e}");
            false
        }
    }
}

/// Check vLLM health status
pub async fn check_vllm_health(api_base_url: &str) -> bool {
    // vLLM has a /health endpoint
    let url = format!("{}/health", api_base_url.trim_end_matches("/v1"));

    match reqwest::get(&url).await {
        Ok(response) => {
            let is_success = response.status().is_success();
            if !is_success {
                warn!(
                    "vLLM health check failed: HTTP {} at {}",
                    response.status(),
                    url
                );
            }
            is_success
        }
        Err(e) => {
            warn!("vLLM health check failed: unable to connect to {url}: {e}");
            false
        }
    }
}

/// Wait for Qdrant to become healthy
pub async fn wait_for_qdrant(config: &StorageConfig, timeout: Duration) -> Result<()> {
    info!("Waiting for Qdrant to become healthy...");

    let start = Instant::now();

    while start.elapsed() < timeout {
        if check_qdrant_health(config).await {
            info!("Qdrant is healthy");
            return Ok(());
        }

        sleep(Duration::from_secs(1)).await;
    }

    Err(anyhow!(
        "Qdrant failed to become healthy within {} seconds. \
         Check logs with: docker logs codesearch-qdrant",
        timeout.as_secs()
    ))
}

/// Wait for vLLM to become healthy
pub async fn wait_for_vllm(api_base_url: &str, timeout: Duration) -> Result<()> {
    info!("Waiting for vLLM to become healthy...");

    let start = Instant::now();

    while start.elapsed() < timeout {
        if check_vllm_health(api_base_url).await {
            info!("vLLM is healthy");
            return Ok(());
        }

        sleep(Duration::from_secs(1)).await;
    }

    Err(anyhow!(
        "vLLM failed to become healthy within {} seconds. \
         Check logs with: docker logs codesearch-vllm",
        timeout.as_secs()
    ))
}

/// Check Postgres health status
///
/// # Security Considerations
///
/// This function creates a new database connection for each health check, passing
/// the password from the configuration. While this does expose credentials in memory,
/// it is acceptable for local health checks because:
/// - The password is already present in the `StorageConfig` structure in memory
/// - This is a local development tool, not a production service
/// - Health checks are infrequent and short-lived
///
/// For production deployments, consider using connection pooling or secret management
/// systems instead of storing passwords in configuration.
pub async fn check_postgres_health(config: &StorageConfig) -> bool {
    let connect_options = PgConnectOptions::new()
        .host(&config.postgres_host)
        .port(config.postgres_port)
        .username(&config.postgres_user)
        .password(&config.postgres_password)
        .database("postgres");

    match sqlx::PgPool::connect_with(connect_options).await {
        Ok(pool) => match sqlx::query("SELECT 1").execute(&pool).await {
            Ok(_) => true,
            Err(e) => {
                warn!(
                    "Postgres health check query failed at {}:{}/postgres: {e}",
                    config.postgres_host, config.postgres_port
                );
                false
            }
        },
        Err(e) => {
            warn!(
                "Postgres health check connection failed at {}:{}/postgres: {e}",
                config.postgres_host, config.postgres_port
            );
            false
        }
    }
}

/// Wait for Postgres to become healthy
///
/// This function reuses a single connection across multiple health checks to avoid
/// the overhead of creating 30+ connections during startup.
pub async fn wait_for_postgres(config: &StorageConfig, timeout: Duration) -> Result<()> {
    info!("Waiting for Postgres to become healthy...");

    let start = Instant::now();
    let connect_options = PgConnectOptions::new()
        .host(&config.postgres_host)
        .port(config.postgres_port)
        .username(&config.postgres_user)
        .password(&config.postgres_password)
        .database("postgres");

    // Try to establish connection, retrying on failures
    let mut pool = None;
    while start.elapsed() < timeout {
        match sqlx::PgPool::connect_with(connect_options.clone()).await {
            Ok(p) => {
                pool = Some(p);
                break;
            }
            Err(e) => {
                warn!(
                    "Postgres connection attempt failed at {}:{}/postgres: {e}",
                    config.postgres_host, config.postgres_port
                );
                sleep(Duration::from_secs(1)).await;
            }
        }
    }

    let pool = pool.ok_or_else(|| {
        anyhow!(
            "Postgres failed to accept connections within {} seconds. \
             Check logs with: docker logs codesearch-postgres",
            timeout.as_secs()
        )
    })?;

    // Connection established, now verify it's healthy with queries
    while start.elapsed() < timeout {
        match sqlx::query("SELECT 1").execute(&pool).await {
            Ok(_) => {
                info!("Postgres is healthy");
                return Ok(());
            }
            Err(e) => {
                warn!(
                    "Postgres health check query failed at {}:{}/postgres: {e}",
                    config.postgres_host, config.postgres_port
                );
                sleep(Duration::from_secs(1)).await;
            }
        }
    }

    Err(anyhow!(
        "Postgres failed to become healthy within {} seconds. \
         Check logs with: docker logs codesearch-postgres",
        timeout.as_secs()
    ))
}

/// Ensure dependencies are running
///
/// All codesearch installations use shared infrastructure. This function verifies
/// that all required services are healthy, providing helpful error messages if not.
pub async fn ensure_dependencies_running(
    config: &StorageConfig,
    api_base_url: Option<&str>,
) -> Result<()> {
    let qdrant_healthy = check_qdrant_health(config).await;
    let postgres_healthy = check_postgres_health(config).await;
    let outbox_running = is_outbox_processor_running()?;
    let vllm_healthy = if let Some(url) = api_base_url {
        check_vllm_health(url).await
    } else {
        true // Skip vLLM check if no API URL provided
    };

    // If all are healthy, we're done
    if qdrant_healthy && postgres_healthy && outbox_running && vllm_healthy {
        info!("All dependencies are already running and healthy");
        return Ok(());
    }

    // Some services are not healthy - provide helpful error message
    let mut msg = String::new();
    msg.push_str("Some required services are not healthy:\n");
    if !postgres_healthy {
        msg.push_str("  - PostgreSQL is not responding\n");
    }
    if !qdrant_healthy {
        msg.push_str("  - Qdrant is not responding\n");
    }
    if !outbox_running {
        msg.push_str("  - Outbox Processor is not running\n");
    }
    if !vllm_healthy {
        msg.push_str("  - vLLM is not responding\n");
    }
    msg.push_str(
        "\nShared infrastructure should start automatically on first `codesearch index`.\n",
    );
    msg.push_str(
        "If services are stopped, try: cd ~/.codesearch/infrastructure && docker compose restart",
    );

    Err(anyhow!(msg))
}
