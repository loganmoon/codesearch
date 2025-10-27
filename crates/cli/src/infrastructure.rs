//! Shared infrastructure management for multi-repository support

use anyhow::{anyhow, Context, Result};
use codesearch_core::config::StorageConfig;
use fs2::FileExt;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tracing::info;

use crate::docker;

/// RAII guard for file locking
///
/// Automatically releases the file lock when dropped, preventing lock leaks
/// even when errors occur during critical sections.
struct LockGuard {
    file: File,
}

impl LockGuard {
    /// Acquire an exclusive lock on the file, blocking until acquired
    fn try_lock_exclusive(file: File, timeout: Duration) -> Result<Self> {
        let start = Instant::now();

        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(Self { file }),
                Err(e) if start.elapsed() >= timeout => {
                    return Err(anyhow!("Timeout waiting for lock: {e}"));
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Best effort unlock - log but don't panic if it fails
        if let Err(e) = fs2::FileExt::unlock(&self.file) {
            tracing::warn!("Failed to unlock file during drop: {e}");
        }
    }
}

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
    let vllm_running = docker::is_vllm_running()?;
    let reranker_running = docker::is_vllm_reranker_running()?;

    Ok(postgres_running && qdrant_running && vllm_running && reranker_running)
}

/// Ensure shared infrastructure directory and compose file exist
async fn ensure_infrastructure_files() -> Result<PathBuf> {
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
        let existing = tokio::fs::read_to_string(&compose_path)
            .await
            .context("Failed to read existing docker-compose.yml")?;
        existing != INFRASTRUCTURE_COMPOSE
    } else {
        true
    };

    if should_write {
        info!("Writing docker-compose.yml to {}", compose_path.display());
        tokio::fs::write(&compose_path, INFRASTRUCTURE_COMPOSE)
            .await
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

    // Clean up any stopped containers before starting
    docker::cleanup_stopped_infrastructure_containers()?;

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

    args.extend(["-f", compose_file_str]);

    args.extend([
        "up",
        "-d",
        "postgres",
        "qdrant",
        "vllm-embeddings",
        "vllm-reranker",
    ]);

    let output = Command::new(cmd)
        .args(&args)
        .output()
        .context("Failed to execute docker compose")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Provide helpful context based on error content
        let help_msg = if stderr.contains("already in use") {
            "\n\nHint: Some containers may still be running. Try:\n  \
             docker ps -a --filter \"name=codesearch\"\n  \
             docker rm -f codesearch-postgres codesearch-qdrant codesearch-vllm-embeddings codesearch-vllm-reranker"
        } else if stderr.contains("Cannot connect to the Docker daemon") {
            "\n\nHint: Docker daemon is not running. Start Docker Desktop or run: sudo systemctl start docker"
        } else {
            "\n\nCheck container logs:\n  \
             docker logs codesearch-postgres\n  \
             docker logs codesearch-qdrant\n  \
             docker logs codesearch-vllm-embeddings\n  \
             docker logs codesearch-vllm-reranker"
        };

        return Err(anyhow!(
            "Failed to start infrastructure:\n{stderr}{help_msg}"
        ));
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

    // Wait for vLLM embeddings
    let api_url = "http://localhost:8000/v1";
    docker::wait_for_vllm(api_url, Duration::from_secs(60)).await?;

    // Wait for vLLM reranker
    let reranker_api_url = "http://localhost:8001";
    docker::wait_for_vllm(reranker_api_url, Duration::from_secs(60)).await?;

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

    // Open or create lock file and immediately acquire lock
    // This uses File::options to atomically open and prepare for locking,
    // eliminating the TOCTOU race between file creation and lock acquisition
    // We don't truncate because the lock file is just for coordination
    let lock_file = File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .context("Failed to open lock file")?;

    let lock_timeout = Duration::from_secs(60);
    let _lock_guard = LockGuard::try_lock_exclusive(lock_file, lock_timeout).context(format!(
        "Timeout waiting for infrastructure initialization lock. \
             If no other process is running, remove the lock file at: {}",
        lock_path.display()
    ))?;

    info!("Lock acquired, proceeding with initialization");

    // Double-check if infrastructure was started by another process
    if is_shared_infrastructure_running()? {
        info!("Infrastructure was started by another process");
        // Lock will be automatically released when _lock_guard drops
        return Ok(());
    }

    // Ensure infrastructure files exist
    let infra_dir = ensure_infrastructure_files().await?;

    // Start infrastructure
    start_infrastructure(&infra_dir)?;

    // Wait for services to be healthy
    wait_for_all_services(config).await?;

    // Lock will be automatically released when _lock_guard drops
    info!("Shared infrastructure initialization complete");
    Ok(())
}
