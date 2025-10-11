//! Shared infrastructure management for multi-repository support

use anyhow::{anyhow, Context, Result};
use codesearch_core::config::StorageConfig;
use fs2::FileExt;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::info;

use crate::docker;

/// Embedded docker-compose.yml for shared infrastructure
const INFRASTRUCTURE_COMPOSE: &str = include_str!("../../../infrastructure/docker-compose.yml");

/// Get the infrastructure directory path
fn get_infrastructure_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Unable to determine home directory"))?;
    Ok(home.join(".codesearch").join("infrastructure"))
}

/// Get the lock file path
fn get_lock_file_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Unable to determine home directory"))?;
    Ok(home.join(".codesearch").join(".infrastructure.lock"))
}

/// Check if shared infrastructure is running
pub fn is_shared_infrastructure_running() -> Result<bool> {
    let postgres_running = docker::is_postgres_running()?;
    let qdrant_running = docker::is_qdrant_running()?;
    let outbox_running = docker::is_outbox_processor_running()?;
    let vllm_running = docker::is_vllm_running()?;

    Ok(postgres_running && qdrant_running && outbox_running && vllm_running)
}

/// Ensure shared infrastructure directory and compose file exist
fn ensure_infrastructure_files() -> Result<PathBuf> {
    let infra_dir = get_infrastructure_dir()?;

    // Create directory if it doesn't exist
    if !infra_dir.exists() {
        info!(
            "Creating infrastructure directory at {}",
            infra_dir.display()
        );
        fs::create_dir_all(&infra_dir).context("Failed to create infrastructure directory")?;
    }

    // Write docker-compose.yml if it doesn't exist or is outdated
    let compose_path = infra_dir.join("docker-compose.yml");
    let should_write = if compose_path.exists() {
        // Check if content matches embedded version
        let existing = fs::read_to_string(&compose_path)
            .context("Failed to read existing docker-compose.yml")?;
        existing != INFRASTRUCTURE_COMPOSE
    } else {
        true
    };

    if should_write {
        info!("Writing docker-compose.yml to {}", compose_path.display());
        fs::write(&compose_path, INFRASTRUCTURE_COMPOSE)
            .context("Failed to write docker-compose.yml")?;
    }

    Ok(infra_dir)
}

/// Start shared infrastructure using docker compose
fn start_infrastructure(infra_dir: &Path) -> Result<()> {
    let compose_file = infra_dir.join("docker-compose.yml");

    if !compose_file.exists() {
        return Err(anyhow!(
            "docker-compose.yml not found at {}",
            compose_file.display()
        ));
    }

    info!("Starting shared infrastructure...");

    // Determine docker compose command
    let (cmd, mut args) = if Command::new("docker")
        .args(["compose", "version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
    {
        ("docker", vec!["compose"])
    } else {
        ("docker-compose", vec![])
    };

    let compose_file_str = compose_file
        .to_str()
        .ok_or_else(|| anyhow!("Invalid path for docker-compose.yml"))?;

    args.extend([
        "-f",
        compose_file_str,
        "up",
        "-d",
        "postgres",
        "qdrant",
        "vllm-embeddings",
        "outbox-processor",
    ]);

    let output = Command::new(cmd)
        .args(&args)
        .output()
        .context("Failed to execute docker compose")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to start infrastructure:\n{stderr}"));
    }

    info!("Infrastructure containers started");
    Ok(())
}

/// Wait for all shared infrastructure services to become healthy
async fn wait_for_all_services(config: &StorageConfig) -> Result<()> {
    info!("Waiting for infrastructure services to become healthy...");

    // Wait for Postgres
    docker::wait_for_postgres(config, Duration::from_secs(30)).await?;

    // Wait for Qdrant
    docker::wait_for_qdrant(config, Duration::from_secs(60)).await?;

    // Wait for vLLM
    let api_url = "http://localhost:8000/v1";
    docker::wait_for_vllm(api_url, Duration::from_secs(60)).await?;

    // Outbox processor doesn't have health endpoint - just wait a bit
    info!("Waiting for outbox processor to start...");
    sleep(Duration::from_secs(2)).await;

    info!("All infrastructure services are healthy");
    Ok(())
}

/// Ensure shared infrastructure is running, starting it if necessary
///
/// This function uses file locking to prevent concurrent initialization.
/// It will block until the lock is acquired or timeout is reached.
pub async fn ensure_shared_infrastructure(config: &StorageConfig) -> Result<()> {
    // Check if already running
    if is_shared_infrastructure_running()? {
        info!("Shared infrastructure is already running");
        return Ok(());
    }

    info!("Shared infrastructure not detected, initializing...");

    // Acquire lock
    let lock_path = get_lock_file_path()?;
    let lock_dir = lock_path
        .parent()
        .ok_or_else(|| anyhow!("Invalid lock file path"))?;

    // Create .codesearch directory if it doesn't exist
    if !lock_dir.exists() {
        fs::create_dir_all(lock_dir).context("Failed to create .codesearch directory")?;
    }

    info!("Acquiring infrastructure initialization lock...");
    let lock_file = File::create(&lock_path).context("Failed to create lock file")?;

    // Try to acquire exclusive lock with timeout
    let lock_start = Instant::now();
    let lock_timeout = Duration::from_secs(60);

    while lock_start.elapsed() < lock_timeout {
        match lock_file.try_lock_exclusive() {
            Ok(()) => {
                info!("Lock acquired, proceeding with initialization");
                break;
            }
            Err(_) => {
                if lock_start.elapsed().as_secs() % 10 == 0 {
                    info!("Waiting for another process to finish infrastructure initialization...");
                }
                sleep(Duration::from_millis(500)).await;
            }
        }
    }

    if lock_start.elapsed() >= lock_timeout {
        return Err(anyhow!(
            "Timeout waiting for infrastructure initialization lock. \
             If no other process is running, remove the lock file at: {}",
            lock_path.display()
        ));
    }

    // Double-check if infrastructure was started by another process
    if is_shared_infrastructure_running()? {
        info!("Infrastructure was started by another process");
        fs2::FileExt::unlock(&lock_file).context("Failed to unlock infrastructure lock")?;
        return Ok(());
    }

    // Ensure infrastructure files exist
    let infra_dir = ensure_infrastructure_files()?;

    // Start infrastructure
    start_infrastructure(&infra_dir)?;

    // Wait for services to be healthy
    wait_for_all_services(config).await?;

    // Release lock
    fs2::FileExt::unlock(&lock_file).context("Failed to unlock infrastructure lock")?;

    info!("Shared infrastructure initialization complete");
    Ok(())
}
