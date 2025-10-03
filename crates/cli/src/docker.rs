//! Docker dependency management for codesearch

use anyhow::{anyhow, Context, Result};
use codesearch_core::config::StorageConfig;
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

/// Stop containerized dependencies
pub fn stop_dependencies(compose_file: Option<&str>) -> Result<()> {
    let (cmd, mut args) = get_compose_command();

    if let Some(file) = compose_file {
        args.push("-f");
        args.push(file);
    }

    args.extend(["down"]);

    info!("Stopping containerized dependencies...");

    let output = Command::new(cmd)
        .args(&args)
        .output()
        .context("Failed to execute docker compose")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("Failed to stop dependencies cleanly: {}", stderr);
    }

    Ok(())
}

/// Check if Qdrant container is running
pub fn is_qdrant_running() -> Result<bool> {
    let output = Command::new("docker")
        .args([
            "ps",
            "--filter",
            "name=codesearch-qdrant",
            "--format",
            "{{.Names}}",
        ])
        .output()
        .context("Failed to check container status")?;

    if !output.status.success() {
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.contains("codesearch-qdrant"))
}

/// Check if vLLM container is running
pub fn is_vllm_running() -> Result<bool> {
    let output = Command::new("docker")
        .args([
            "ps",
            "--filter",
            "name=codesearch-vllm",
            "--format",
            "{{.Names}}",
        ])
        .output()
        .context("Failed to check container status")?;

    if !output.status.success() {
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.contains("codesearch-vllm"))
}

/// Check if Postgres container is running
pub fn is_postgres_running() -> Result<bool> {
    let output = Command::new("docker")
        .args([
            "ps",
            "--filter",
            "name=codesearch-postgres",
            "--format",
            "{{.Names}}",
        ])
        .output()
        .context("Failed to check container status")?;

    if !output.status.success() {
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.contains("codesearch-postgres"))
}

/// Check if Outbox Processor container is running
pub fn is_outbox_processor_running() -> Result<bool> {
    let output = Command::new("docker")
        .args([
            "ps",
            "--filter",
            "name=codesearch-outbox-processor",
            "--format",
            "{{.Names}}",
        ])
        .output()
        .context("Failed to check container status")?;

    if !output.status.success() {
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.contains("codesearch-outbox-processor"))
}

/// Check Qdrant health status
pub async fn check_qdrant_health(config: &StorageConfig) -> Result<bool> {
    // Qdrant doesn't have a /health endpoint, but the root endpoint returns version info
    let url = format!("http://{}:{}/", config.qdrant_host, config.qdrant_rest_port);

    match reqwest::get(&url).await {
        Ok(response) => Ok(response.status().is_success()),
        Err(_) => Ok(false),
    }
}

/// Check vLLM health status
pub async fn check_vllm_health(api_base_url: &str) -> Result<bool> {
    // vLLM has a /health endpoint
    let url = format!("{}/health", api_base_url.trim_end_matches("/v1"));

    match reqwest::get(&url).await {
        Ok(response) => Ok(response.status().is_success()),
        Err(_) => Ok(false),
    }
}

/// Wait for Qdrant to become healthy
pub async fn wait_for_qdrant(config: &StorageConfig, timeout: Duration) -> Result<()> {
    info!("Waiting for Qdrant to become healthy...");

    let start = Instant::now();

    while start.elapsed() < timeout {
        if check_qdrant_health(config).await? {
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
        if check_vllm_health(api_base_url).await? {
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
pub async fn check_postgres_health(config: &StorageConfig) -> Result<bool> {
    let connection_string = format!(
        "postgresql://{}:{}@{}:{}/{}",
        config.postgres_user,
        config.postgres_password,
        config.postgres_host,
        config.postgres_port,
        config.postgres_database
    );

    match sqlx::PgPool::connect(&connection_string).await {
        Ok(pool) => match sqlx::query("SELECT 1").execute(&pool).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        },
        Err(_) => Ok(false),
    }
}

/// Wait for Postgres to become healthy
pub async fn wait_for_postgres(config: &StorageConfig, timeout: Duration) -> Result<()> {
    info!("Waiting for Postgres to become healthy...");

    let start = Instant::now();

    while start.elapsed() < timeout {
        if check_postgres_health(config).await? {
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

/// Ensure dependencies are running, starting them if necessary
pub async fn ensure_dependencies_running(
    config: &StorageConfig,
    api_base_url: Option<&str>,
) -> Result<()> {
    let qdrant_healthy = check_qdrant_health(config).await?;
    let postgres_healthy = check_postgres_health(config).await?;
    let outbox_running = is_outbox_processor_running().unwrap_or(false);
    let vllm_healthy = if let Some(url) = api_base_url {
        check_vllm_health(url).await?
    } else {
        true // Skip vLLM check if no API URL provided
    };

    // If all are healthy, we're done
    if qdrant_healthy && postgres_healthy && outbox_running && vllm_healthy {
        info!("All dependencies are already running and healthy");
        return Ok(());
    }

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

/// Get status of dependencies
pub async fn get_dependencies_status(
    config: &StorageConfig,
    api_base_url: Option<&str>,
) -> Result<DependencyStatus> {
    let docker_available = is_docker_available();
    let compose_available = is_docker_compose_available();
    let qdrant_running = is_qdrant_running().unwrap_or(false);
    let qdrant_healthy = if qdrant_running {
        check_qdrant_health(config).await.unwrap_or(false)
    } else {
        false
    };
    let postgres_running = is_postgres_running().unwrap_or(false);
    let postgres_healthy = if postgres_running {
        check_postgres_health(config).await.unwrap_or(false)
    } else {
        false
    };
    let outbox_running = is_outbox_processor_running().unwrap_or(false);
    let vllm_running = is_vllm_running().unwrap_or(false);
    let vllm_healthy = if vllm_running && api_base_url.is_some() {
        check_vllm_health(api_base_url.unwrap_or("http://localhost:8000/v1"))
            .await
            .unwrap_or(false)
    } else {
        false
    };

    Ok(DependencyStatus {
        docker_available,
        compose_available,
        qdrant_running,
        qdrant_healthy,
        postgres_running,
        postgres_healthy,
        outbox_running,
        vllm_running,
        vllm_healthy,
    })
}

#[derive(Debug)]
pub struct DependencyStatus {
    pub docker_available: bool,
    pub compose_available: bool,
    pub qdrant_running: bool,
    pub qdrant_healthy: bool,
    pub postgres_running: bool,
    pub postgres_healthy: bool,
    pub outbox_running: bool,
    pub vllm_running: bool,
    pub vllm_healthy: bool,
}

impl std::fmt::Display for DependencyStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Dependency Status:")?;
        writeln!(
            f,
            "  Docker:            {}",
            if self.docker_available {
                "✓ Available"
            } else {
                "✗ Not found"
            }
        )?;
        writeln!(
            f,
            "  Docker Compose:    {}",
            if self.compose_available {
                "✓ Available"
            } else {
                "✗ Not found"
            }
        )?;
        writeln!(
            f,
            "  Qdrant Container:  {}",
            if self.qdrant_running {
                "✓ Running"
            } else {
                "✗ Not running"
            }
        )?;
        writeln!(
            f,
            "  Qdrant Health:     {}",
            if self.qdrant_healthy {
                "✓ Healthy"
            } else {
                "✗ Unhealthy"
            }
        )?;
        writeln!(
            f,
            "  Postgres Container: {}",
            if self.postgres_running {
                "✓ Running"
            } else {
                "✗ Not running"
            }
        )?;
        writeln!(
            f,
            "  Postgres Health:    {}",
            if self.postgres_healthy {
                "✓ Healthy"
            } else {
                "✗ Unhealthy"
            }
        )?;
        writeln!(
            f,
            "  Outbox Processor:   {}",
            if self.outbox_running {
                "✓ Running"
            } else {
                "✗ Not running"
            }
        )?;
        writeln!(
            f,
            "  vLLM Container:     {}",
            if self.vllm_running {
                "✓ Running"
            } else {
                "✗ Not running"
            }
        )?;
        writeln!(
            f,
            "  vLLM Health:        {}",
            if self.vllm_healthy {
                "✓ Healthy"
            } else {
                "✗ Unhealthy"
            }
        )?;
        Ok(())
    }
}
