//! Container management for E2E tests

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use uuid::Uuid;

/// Test Postgres container with temporary storage
pub struct TestPostgres {
    container_id: String,
    container_name: String,
    port: u16,
}

impl TestPostgres {
    /// Start a new Postgres instance
    pub async fn start() -> Result<Self> {
        let container_name = format!("postgres-test-{}", Uuid::new_v4());

        // Find available port dynamically
        let port = portpicker::pick_unused_port()
            .ok_or_else(|| anyhow::anyhow!("No available port for Postgres"))?;

        // Start Postgres container
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                &container_name,
                "-p",
                &format!("{port}:5432"),
                "-e",
                "POSTGRES_DB=codesearch",
                "-e",
                "POSTGRES_USER=codesearch",
                "-e",
                "POSTGRES_PASSWORD=codesearch",
                "postgres:17",
            ])
            .output()
            .context("Failed to start Postgres container")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "Failed to start Postgres container: {stderr}"
            ));
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let instance = Self {
            container_id: container_id.clone(),
            container_name,
            port,
        };

        if let Err(e) = instance.wait_for_health().await {
            let logs = instance.get_container_logs();
            instance.cleanup();
            return Err(anyhow::anyhow!(
                "Postgres container failed to become healthy: {e}\nLogs: {logs}"
            ));
        }

        Ok(instance)
    }

    /// Get the Postgres port
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Wait for Postgres to become healthy
    async fn wait_for_health(&self) -> Result<()> {
        let max_attempts = 30;
        let delay = Duration::from_millis(100);

        for attempt in 1..=max_attempts {
            // Check if container is still running
            let status = Command::new("docker")
                .args(["inspect", "-f", "{{.State.Running}}", &self.container_id])
                .output()
                .context("Failed to check container status")?;

            let is_running = String::from_utf8_lossy(&status.stdout)
                .trim()
                .eq_ignore_ascii_case("true");

            if !is_running {
                return Err(anyhow::anyhow!("Container stopped unexpectedly"));
            }

            // Try to connect to Postgres
            let connection_string = format!(
                "postgresql://codesearch:codesearch@localhost:{}/codesearch",
                self.port
            );

            if let Ok(pool) = sqlx::PgPool::connect(&connection_string).await {
                if sqlx::query("SELECT 1").execute(&pool).await.is_ok() {
                    return Ok(());
                }
            }

            if attempt < max_attempts {
                tokio::time::sleep(delay).await;
            }
        }

        Err(anyhow::anyhow!(
            "Postgres did not become healthy after {} attempts",
            max_attempts
        ))
    }

    /// Get container logs for debugging
    fn get_container_logs(&self) -> String {
        let output = Command::new("docker")
            .args(["logs", "--tail", "50", &self.container_id])
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                format!("STDOUT:\n{stdout}\n\nSTDERR:\n{stderr}")
            }
            Err(e) => format!("Failed to get logs: {e}"),
        }
    }

    /// Stop and clean up the Postgres instance
    fn cleanup(&self) {
        // Stop container
        let _ = Command::new("docker")
            .args(["stop", &self.container_name])
            .output();

        // Remove container
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.container_name])
            .output();
    }
}

impl Drop for TestPostgres {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Pool of test Postgres containers for concurrent testing
pub struct TestPostgresPool {
    containers: Vec<TestPostgres>,
}

impl TestPostgresPool {
    /// Create a new pool with the specified number of containers
    pub async fn new(size: usize) -> Result<Self> {
        let mut containers = Vec::with_capacity(size);

        for i in 0..size {
            match TestPostgres::start().await {
                Ok(container) => containers.push(container),
                Err(e) => {
                    drop(containers);
                    return Err(anyhow::anyhow!(
                        "Failed to start container {i} in pool: {e}"
                    ));
                }
            }
        }

        Ok(Self { containers })
    }

    /// Get a reference to a container by index
    pub fn get(&self, index: usize) -> Option<&TestPostgres> {
        self.containers.get(index)
    }
}

/// Test Qdrant container with temporary storage and enhanced cleanup
pub struct TestQdrant {
    container_id: String,
    container_name: String,
    temp_dir: PathBuf,
    port: u16,
    rest_port: u16,
}

impl TestQdrant {
    /// Start a new Qdrant instance with temporary storage
    ///
    /// Uses health check polling to ensure the container is ready before returning.
    pub async fn start() -> Result<Self> {
        let container_name = format!("qdrant-test-{}", Uuid::new_v4());
        let temp_dir_name = format!("/tmp/qdrant-test-{}", Uuid::new_v4());
        let temp_dir = PathBuf::from(&temp_dir_name);

        // Create temp directory
        std::fs::create_dir_all(&temp_dir)
            .with_context(|| format!("Failed to create temp directory: {temp_dir_name}"))?;

        // Find available ports dynamically to avoid conflicts
        let port = portpicker::pick_unused_port()
            .ok_or_else(|| anyhow::anyhow!("No available port for Qdrant"))?;
        let rest_port = portpicker::pick_unused_port()
            .ok_or_else(|| anyhow::anyhow!("No available port for Qdrant REST"))?;

        // Start Qdrant container with temporary storage using unprivileged image
        // This runs as a non-root user, so cleanup won't require sudo
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                &container_name,
                "-p",
                &format!("{port}:6334"),
                "-p",
                &format!("{rest_port}:6333"),
                "-v",
                &format!("{temp_dir_name}:/qdrant/storage"),
                "qdrant/qdrant:latest-unprivileged",
            ])
            .output()
            .context("Failed to start Qdrant container")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Clean up temp directory if container failed to start
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(anyhow::anyhow!(
                "Failed to start Qdrant container: {stderr}"
            ));
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Poll for health instead of fixed sleep
        let instance = Self {
            container_id: container_id.clone(),
            container_name,
            temp_dir,
            port,
            rest_port,
        };

        if let Err(e) = instance.wait_for_health().await {
            // Container failed to become healthy, capture logs
            let logs = instance.get_container_logs();
            instance.cleanup();
            return Err(anyhow::anyhow!(
                "Qdrant container failed to become healthy: {e}\nLogs: {logs}"
            ));
        }

        Ok(instance)
    }

    /// Get the gRPC port
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the REST API port
    pub fn rest_port(&self) -> u16 {
        self.rest_port
    }

    /// Get the REST API URL
    pub fn rest_url(&self) -> String {
        format!("http://localhost:{}", self.rest_port)
    }

    /// Wait for Qdrant to become healthy
    async fn wait_for_health(&self) -> Result<()> {
        let max_attempts = 30;
        let delay = Duration::from_millis(100);

        for attempt in 1..=max_attempts {
            // Check if container is still running
            let status = Command::new("docker")
                .args(["inspect", "-f", "{{.State.Running}}", &self.container_id])
                .output()
                .context("Failed to check container status")?;

            let is_running = String::from_utf8_lossy(&status.stdout)
                .trim()
                .eq_ignore_ascii_case("true");

            if !is_running {
                return Err(anyhow::anyhow!("Container stopped unexpectedly"));
            }

            // Try to connect to health endpoint
            let health_url = format!("{}/healthz", self.rest_url());
            if let Ok(response) = reqwest::get(&health_url).await {
                if response.status().is_success() {
                    return Ok(());
                }
            }

            if attempt < max_attempts {
                tokio::time::sleep(delay).await;
            }
        }

        Err(anyhow::anyhow!(
            "Qdrant did not become healthy after {} attempts",
            max_attempts
        ))
    }

    /// Get container logs for debugging
    fn get_container_logs(&self) -> String {
        let output = Command::new("docker")
            .args(["logs", "--tail", "50", &self.container_id])
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                format!("STDOUT:\n{stdout}\n\nSTDERR:\n{stderr}")
            }
            Err(e) => format!("Failed to get logs: {e}"),
        }
    }

    /// Stop and clean up the Qdrant instance
    fn cleanup(&self) {
        // Stop container (ignore errors, container might already be stopped)
        let _ = Command::new("docker")
            .args(["stop", &self.container_name])
            .output();

        // Remove container
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.container_name])
            .output();

        // Remove temp directory (may need sudo for Docker-created files)
        if self.temp_dir.exists() {
            // Try normal removal first
            if std::fs::remove_dir_all(&self.temp_dir).is_err() {
                // If that fails, try with sudo (for Docker-created files)
                let _ = Command::new("sudo")
                    .args(["rm", "-rf", &self.temp_dir.to_string_lossy().as_ref()])
                    .output();
            }
        }
    }
}

impl Drop for TestQdrant {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Pool of test Qdrant containers for concurrent testing
pub struct TestQdrantPool {
    containers: Vec<TestQdrant>,
}

impl TestQdrantPool {
    /// Create a new pool with the specified number of containers
    pub async fn new(size: usize) -> Result<Self> {
        let mut containers = Vec::with_capacity(size);

        for i in 0..size {
            match TestQdrant::start().await {
                Ok(container) => containers.push(container),
                Err(e) => {
                    // Clean up any containers we've already created
                    drop(containers);
                    return Err(anyhow::anyhow!(
                        "Failed to start container {i} in pool: {e}"
                    ));
                }
            }
        }

        Ok(Self { containers })
    }

    /// Get a reference to a container by index
    pub fn get(&self, index: usize) -> Option<&TestQdrant> {
        self.containers.get(index)
    }

    /// Get the number of containers in the pool
    pub fn len(&self) -> usize {
        self.containers.len()
    }

    /// Check if the pool is empty
    pub fn is_empty(&self) -> bool {
        self.containers.is_empty()
    }

    /// Iterate over all containers
    pub fn iter(&self) -> impl Iterator<Item = &TestQdrant> {
        self.containers.iter()
    }
}

impl Drop for TestQdrantPool {
    fn drop(&mut self) {
        // Cleanup happens automatically via Drop on each TestQdrant
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_qdrant_starts_and_is_healthy() -> Result<()> {
        let qdrant = TestQdrant::start().await?;

        // Verify we can connect to REST API
        let health_url = format!("{}/healthz", qdrant.rest_url());
        let response = reqwest::get(&health_url).await?;
        assert!(response.status().is_success());

        Ok(())
    }

    #[tokio::test]
    async fn test_qdrant_cleanup_removes_temp_dir() -> Result<()> {
        let temp_dir = {
            let qdrant = TestQdrant::start().await?;
            qdrant.temp_dir.clone()
        };

        // Poll for cleanup with timeout (Docker cleanup can be slow)
        let mut cleaned_up = false;
        for _ in 0..20 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            if !temp_dir.exists() {
                cleaned_up = true;
                break;
            }
        }

        // After dropping, temp directory should be cleaned up
        assert!(
            cleaned_up,
            "Temp directory still exists after cleanup: {}",
            temp_dir.display()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_qdrant_pool_creates_multiple_containers() -> Result<()> {
        let pool = TestQdrantPool::new(3).await?;

        assert_eq!(pool.len(), 3);

        // Verify all containers are healthy
        for container in pool.iter() {
            let health_url = format!("{}/healthz", container.rest_url());
            let response = reqwest::get(&health_url).await?;
            assert!(response.status().is_success());
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_postgres_starts_and_is_healthy() -> Result<()> {
        let postgres = TestPostgres::start().await?;

        // Verify we can connect to Postgres
        let connection_string = format!(
            "postgresql://codesearch:codesearch@localhost:{}/codesearch",
            postgres.port()
        );
        let pool = sqlx::PgPool::connect(&connection_string).await?;
        sqlx::query("SELECT 1").execute(&pool).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_postgres_cleanup_stops_container() -> Result<()> {
        let container_name = {
            let postgres = TestPostgres::start().await?;
            postgres.container_name.clone()
        };

        // Poll for cleanup with timeout
        let mut cleaned_up = false;
        for _ in 0..20 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            let output = Command::new("docker")
                .args([
                    "ps",
                    "-a",
                    "--filter",
                    &format!("name={container_name}"),
                    "--format",
                    "{{.Names}}",
                ])
                .output()?;

            if String::from_utf8_lossy(&output.stdout).trim().is_empty() {
                cleaned_up = true;
                break;
            }
        }

        assert!(
            cleaned_up,
            "Postgres container still exists after cleanup: {container_name}"
        );

        Ok(())
    }
}
