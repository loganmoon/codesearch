//! Docker dependency management for codesearch

use anyhow::{anyhow, Context, Result};
use codesearch_core::config::StorageConfig;
use sqlx::postgres::PgConnectOptions;
use std::process::Command;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{info, warn};

/// Check if Docker is installed and available
pub fn is_docker_available() -> bool {
    Command::new("docker")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Check if Docker Compose is installed and available
pub fn is_docker_compose_available() -> bool {
    // Try docker compose (v2) first
    if Command::new("docker")
        .args(["compose", "version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
    {
        return true;
    }

    // Fall back to docker-compose (v1)
    Command::new("docker-compose")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Get the Docker Compose command (v2 or v1)
fn get_compose_command() -> (&'static str, Vec<&'static str>) {
    // Prefer docker compose (v2)
    if Command::new("docker")
        .args(["compose", "version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
    {
        ("docker", vec!["compose"])
    } else {
        ("docker-compose", vec![])
    }
}

/// Start containerized dependencies
pub fn start_dependencies(compose_file: Option<&str>) -> Result<()> {
    if !is_docker_available() {
        return Err(anyhow!(
            "Docker is not installed. Please install Docker from https://docs.docker.com/get-docker/"
        ));
    }

    if !is_docker_compose_available() {
        return Err(anyhow!(
            "Docker Compose is not installed. Please install Docker Compose from https://docs.docker.com/compose/install/"
        ));
    }

    let (cmd, mut args) = get_compose_command();

    // Add compose file if specified
    if let Some(file) = compose_file {
        args.push("-f");
        args.push(file);
    }

    args.extend([
        "up",
        "-d",
        "qdrant",
        "postgres",
        "outbox-processor",
        "vllm-embeddings",
    ]);

    info!("Starting containerized dependencies...");

    let output = Command::new(cmd)
        .args(&args)
        .output()
        .context("Failed to execute docker compose")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to start dependencies:\n{stderr}"));
    }

    info!("Dependencies started successfully");
    Ok(())
}

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
pub async fn wait_for_postgres(config: &StorageConfig, timeout: Duration) -> Result<()> {
    info!("Waiting for Postgres to become healthy...");

    let start = Instant::now();

    while start.elapsed() < timeout {
        if check_postgres_health(config).await {
            info!("Postgres is healthy");
            return Ok(());
        }

        sleep(Duration::from_secs(1)).await;
    }

    Err(anyhow!(
        "Postgres failed to become healthy within {} seconds. \
         Check logs with: docker logs codesearch-postgres",
        timeout.as_secs()
    ))
}

/// Check if we're in shared infrastructure mode
///
/// Returns true if shared infrastructure containers exist (regardless of their state)
pub fn is_shared_infrastructure_mode() -> Result<bool> {
    // Check if any of the expected shared infrastructure containers exist
    // We use "docker ps -a" to check for existence (running or stopped)
    let containers = [
        "codesearch-postgres",
        "codesearch-qdrant",
        "codesearch-vllm",
        "codesearch-outbox-processor",
    ];

    for container in &containers {
        let output = Command::new("docker")
            .args([
                "ps",
                "-a",
                "--filter",
                &format!("name={container}"),
                "--format",
                "{{.Names}}",
            ])
            .output()
            .context("Failed to check for shared infrastructure containers")?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains(container) {
                // At least one shared infrastructure container exists
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Ensure dependencies are running, starting them if necessary
pub async fn ensure_dependencies_running(
    config: &StorageConfig,
    api_base_url: Option<&str>,
) -> Result<()> {
    // Check if we're in shared infrastructure mode
    let shared_mode = is_shared_infrastructure_mode()?;

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

    // In shared infrastructure mode, don't start per-repo docker compose
    if shared_mode {
        info!("Shared infrastructure mode detected - skipping per-repository docker compose");

        // Just verify health or fail
        if !qdrant_healthy || !postgres_healthy || !outbox_running || !vllm_healthy {
            let mut msg = String::new();
            msg.push_str(
                "Shared infrastructure containers exist but some services are not healthy:\n",
            );
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
            msg.push_str("\nTry restarting shared infrastructure:\n");
            msg.push_str("  cd ~/.codesearch/infrastructure && docker compose restart");
            return Err(anyhow!(msg));
        }

        return Ok(());
    }

    // Not in shared mode - use regular per-repo docker compose logic
    // Check if auto-start is enabled
    if !config.auto_start_deps {
        let mut msg = String::new();
        if !qdrant_healthy {
            msg.push_str("Qdrant is not running. ");
        }
        if !postgres_healthy {
            msg.push_str("Postgres is not running. ");
        }
        if !outbox_running {
            msg.push_str("Outbox Processor is not running. ");
        }
        if !vllm_healthy {
            msg.push_str("vLLM is not running. ");
        }
        msg.push_str("Start them manually with: docker compose up -d\n");
        msg.push_str("Or enable auto_start_deps in your configuration");
        return Err(anyhow!(msg));
    }

    // Check if containers exist but are not running
    if !is_qdrant_running()?
        || !is_postgres_running()?
        || !outbox_running
        || (api_base_url.is_some() && !is_vllm_running()?)
    {
        info!("Starting containerized dependencies...");
        start_dependencies(config.docker_compose_file.as_deref())?;
    }

    // Wait for health
    if !qdrant_healthy {
        wait_for_qdrant(config, Duration::from_secs(60)).await?;
    }
    if !postgres_healthy {
        wait_for_postgres(config, Duration::from_secs(30)).await?;
    }
    // Outbox processor doesn't have a health endpoint - just wait a bit for it to start
    if !outbox_running {
        info!("Waiting for outbox processor to start...");
        sleep(Duration::from_secs(2)).await;
    }
    if let Some(url) = api_base_url {
        if !vllm_healthy {
            wait_for_vllm(url, Duration::from_secs(60)).await?;
        }
    }

    Ok(())
}
