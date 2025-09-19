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

    args.extend(["up", "-d", "qdrant"]);

    info!("Starting containerized dependencies...");

    let output = Command::new(cmd)
        .args(&args)
        .output()
        .context("Failed to execute docker compose")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to start dependencies:\n{}", stderr));
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

/// Check Qdrant health status
pub async fn check_qdrant_health(config: &StorageConfig) -> Result<bool> {
    // Qdrant doesn't have a /health endpoint, but the root endpoint returns version info
    let url = format!("http://{}:{}/", config.qdrant_host, config.qdrant_rest_port);

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

/// Ensure dependencies are running, starting them if necessary
pub async fn ensure_dependencies_running(config: &StorageConfig) -> Result<()> {
    // First check if Qdrant is already healthy
    if check_qdrant_health(config).await? {
        info!("Qdrant is already running and healthy");
        return Ok(());
    }

    // Check if auto-start is enabled
    if !config.auto_start_deps {
        return Err(anyhow!(
            "Qdrant is not running. Start it manually with: docker compose up -d qdrant\n\
             Or enable auto_start_deps in your configuration"
        ));
    }

    // Check if container exists but is not running
    if !is_qdrant_running()? {
        info!("Qdrant container is not running, starting it...");
        start_dependencies(config.docker_compose_file.as_deref())?;
    }

    // Wait for health
    wait_for_qdrant(config, Duration::from_secs(60)).await?;

    Ok(())
}

/// Get status of dependencies
pub async fn get_dependencies_status(config: &StorageConfig) -> Result<DependencyStatus> {
    let docker_available = is_docker_available();
    let compose_available = is_docker_compose_available();
    let qdrant_running = is_qdrant_running().unwrap_or(false);
    let qdrant_healthy = if qdrant_running {
        check_qdrant_health(config).await.unwrap_or(false)
    } else {
        false
    };

    Ok(DependencyStatus {
        docker_available,
        compose_available,
        qdrant_running,
        qdrant_healthy,
    })
}

#[derive(Debug)]
pub struct DependencyStatus {
    pub docker_available: bool,
    pub compose_available: bool,
    pub qdrant_running: bool,
    pub qdrant_healthy: bool,
}

impl std::fmt::Display for DependencyStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Dependency Status:")?;
        writeln!(
            f,
            "  Docker:          {}",
            if self.docker_available {
                "✓ Available"
            } else {
                "✗ Not found"
            }
        )?;
        writeln!(
            f,
            "  Docker Compose:  {}",
            if self.compose_available {
                "✓ Available"
            } else {
                "✗ Not found"
            }
        )?;
        writeln!(
            f,
            "  Qdrant Container: {}",
            if self.qdrant_running {
                "✓ Running"
            } else {
                "✗ Not running"
            }
        )?;
        writeln!(
            f,
            "  Qdrant Health:    {}",
            if self.qdrant_healthy {
                "✓ Healthy"
            } else {
                "✗ Unhealthy"
            }
        )?;
        Ok(())
    }
}
