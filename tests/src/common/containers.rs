//! Container management for E2E tests

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, Once};
use std::time::Duration;
use tokio::sync::OnceCell;
use uuid::Uuid;

/// Global registry of test resources for cleanup
static RESOURCE_REGISTRY: Mutex<Option<ResourceRegistry>> = Mutex::new(None);
static INIT_CLEANUP: Once = Once::new();

/// Global shared Qdrant instance for all tests (Phase 2 optimization)
static SHARED_QDRANT: OnceCell<TestQdrant> = OnceCell::const_new();

/// Global shared Postgres instance for all tests (Phase 2 optimization)
static SHARED_POSTGRES: OnceCell<TestPostgres> = OnceCell::const_new();

struct ResourceRegistry {
    container_names: Vec<String>,
    process_ids: Vec<u32>,
}

impl ResourceRegistry {
    fn new() -> Self {
        Self {
            container_names: Vec::new(),
            process_ids: Vec::new(),
        }
    }

    fn register_container(&mut self, name: String) {
        self.container_names.push(name);
    }

    fn register_process(&mut self, pid: u32) {
        self.process_ids.push(pid);
    }

    fn unregister_container(&mut self, name: &str) {
        self.container_names.retain(|n| n != name);
    }

    fn unregister_process(&mut self, pid: u32) {
        self.process_ids.retain(|p| *p != pid);
    }

    fn cleanup_all(&mut self) {
        // Kill all processes
        for pid in &self.process_ids {
            #[cfg(unix)]
            {
                use nix::sys::signal::{self, Signal};
                use nix::unistd::Pid;
                let _ = signal::kill(Pid::from_raw(*pid as i32), Signal::SIGKILL);
            }
        }

        // Stop all containers
        if !self.container_names.is_empty() {
            let _ = Command::new("docker")
                .arg("stop")
                .args(&self.container_names)
                .output();
            let _ = Command::new("docker")
                .arg("rm")
                .arg("-f")
                .args(&self.container_names)
                .output();
        }

        self.container_names.clear();
        self.process_ids.clear();
    }
}

fn init_cleanup_handler() {
    INIT_CLEANUP.call_once(|| {
        {
            let mut registry = RESOURCE_REGISTRY.lock().unwrap();
            *registry = Some(ResourceRegistry::new());
        }

        // Register cleanup on process exit
        let _ = std::panic::catch_unwind(|| {
            ctrlc::set_handler(move || {
                eprintln!("\n🧹 Caught Ctrl+C, cleaning up test resources...");
                if let Ok(mut registry) = RESOURCE_REGISTRY.lock() {
                    if let Some(reg) = registry.as_mut() {
                        reg.cleanup_all();
                    }
                }
                std::process::exit(130); // Standard exit code for SIGINT
            })
            .expect("Error setting Ctrl-C handler");
        });
    });
}

fn register_container(name: String) {
    init_cleanup_handler();
    if let Ok(mut registry) = RESOURCE_REGISTRY.lock() {
        if let Some(reg) = registry.as_mut() {
            reg.register_container(name);
        }
    }
}

fn unregister_container(name: &str) {
    if let Ok(mut registry) = RESOURCE_REGISTRY.lock() {
        if let Some(reg) = registry.as_mut() {
            reg.unregister_container(name);
        }
    }
}

fn register_process(pid: u32) {
    init_cleanup_handler();
    if let Ok(mut registry) = RESOURCE_REGISTRY.lock() {
        if let Some(reg) = registry.as_mut() {
            reg.register_process(pid);
        }
    }
}

fn unregister_process(pid: u32) {
    if let Ok(mut registry) = RESOURCE_REGISTRY.lock() {
        if let Some(reg) = registry.as_mut() {
            reg.unregister_process(pid);
        }
    }
}

/// Ensures the outbox_processor Docker image is built before tests run
/// This prevents race conditions when multiple tests try to build it concurrently
static BUILD_OUTBOX_IMAGE: Once = Once::new();

/// Build the outbox_processor Docker image if it doesn't exist
///
/// This is called automatically by TestOutboxProcessor::start()
fn ensure_outbox_image_built() -> Result<()> {
    BUILD_OUTBOX_IMAGE.call_once(|| {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().expect("Failed to get current dir"));

        let workspace_root = manifest_dir
            .parent()
            .expect("Failed to find workspace root");

        let output = Command::new("docker")
            .args([
                "build",
                "-t",
                "codesearch-outbox:test",
                "-f",
                "Dockerfile.outbox-processor",
                ".",
            ])
            .current_dir(workspace_root)
            .output()
            .expect("Failed to build outbox-processor image");

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!("Failed to build outbox-processor Docker image: {stderr}");
        }
    });

    Ok(())
}

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
        // Bind to 127.0.0.1 only for security and to avoid port conflicts
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                &container_name,
                "-p",
                &format!("127.0.0.1:{port}:5432"),
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
            // Clean up container if it was created (even if it failed to start)
            let _ = Command::new("docker")
                .args(["rm", "-f", &container_name])
                .output();
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

        // Register for global cleanup
        register_container(instance.container_name.clone());

        Ok(instance)
    }

    /// Start a new Postgres instance with pre-allocated port
    ///
    /// This is used by TestPostgresPool to avoid port allocation race conditions.
    pub async fn start_with_port(port: u16) -> Result<Self> {
        let container_name = format!("postgres-test-{}", Uuid::new_v4());

        // Start Postgres container with pre-allocated port
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                &container_name,
                "-p",
                &format!("127.0.0.1:{port}:5432"),
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
            // Clean up container if it was created (even if it failed to start)
            let _ = Command::new("docker")
                .args(["rm", "-f", &container_name])
                .output();
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

        // Register for global cleanup
        register_container(instance.container_name.clone());

        Ok(instance)
    }

    /// Get the Postgres port
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Wait for Postgres to become healthy using exponential backoff
    async fn wait_for_health(&self) -> Result<()> {
        let max_attempts = 20;
        let initial_delay = Duration::from_millis(50);
        let max_delay = Duration::from_millis(500);

        let mut delay = initial_delay;

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
                // Exponential backoff: double the delay, but cap at max_delay
                delay = std::cmp::min(delay * 2, max_delay);
            }
        }

        Err(anyhow::anyhow!(
            "Postgres did not become healthy after {max_attempts} attempts"
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
        // Unregister from global cleanup
        unregister_container(&self.container_name);

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

/// Start test containers in parallel for faster setup
///
/// This starts both Qdrant and Postgres containers concurrently,
/// which is much faster than starting them sequentially.
pub async fn start_test_containers() -> Result<(TestQdrant, TestPostgres)> {
    let (qdrant_result, postgres_result) = tokio::join!(TestQdrant::start(), TestPostgres::start());

    Ok((qdrant_result?, postgres_result?))
}

/// Get or create the shared Qdrant instance (Phase 2 optimization)
///
/// Returns a reference to a global shared Qdrant container that is created once
/// and reused across all tests. Tests maintain isolation by using unique collection names.
pub async fn get_shared_qdrant() -> Result<&'static TestQdrant> {
    SHARED_QDRANT
        .get_or_try_init(|| async {
            eprintln!("🚀 Starting shared Qdrant instance for all tests...");
            TestQdrant::start().await
        })
        .await
}

/// Get or create the shared Postgres instance (Phase 2 optimization)
///
/// Returns a reference to a global shared Postgres container that is created once
/// and reused across all tests. Tests maintain isolation by creating unique databases.
pub async fn get_shared_postgres() -> Result<&'static TestPostgres> {
    SHARED_POSTGRES
        .get_or_try_init(|| async {
            eprintln!("🚀 Starting shared Postgres instance for all tests...");
            TestPostgres::start().await
        })
        .await
}

/// Create an isolated test database in the shared Postgres instance
///
/// Each test gets its own database to maintain isolation while sharing the container.
/// The database name includes a UUID to ensure uniqueness.
pub async fn create_test_database(postgres: &TestPostgres) -> Result<String> {
    let db_name = format!("test_db_{}", Uuid::new_v4().simple());
    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/postgres",
        postgres.port()
    );

    let pool = sqlx::PgPool::connect(&connection_url).await?;
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&pool)
        .await?;
    pool.close().await;

    Ok(db_name)
}

/// Drop a test database from the shared Postgres instance
///
/// Terminates all connections to the database before dropping it to avoid errors.
pub async fn drop_test_database(postgres: &TestPostgres, db_name: &str) -> Result<()> {
    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/postgres",
        postgres.port()
    );

    let pool = sqlx::PgPool::connect(&connection_url).await?;

    // Terminate all connections to the database first
    sqlx::query(&format!(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{db_name}'"
    ))
    .execute(&pool)
    .await?;

    // Now drop the database
    sqlx::query(&format!("DROP DATABASE IF EXISTS {db_name}"))
        .execute(&pool)
        .await?;
    pool.close().await;

    Ok(())
}

/// Clean up a test collection from Qdrant
///
/// Deletes the collection to clean up after a test while keeping the container running.
pub async fn drop_test_collection(qdrant: &TestQdrant, collection_name: &str) -> Result<()> {
    let url = format!("{}/collections/{collection_name}", qdrant.rest_url());
    let _ = reqwest::Client::new().delete(&url).send().await?;
    Ok(())
}

/// Pool of test Postgres containers for concurrent testing
pub struct TestPostgresPool {
    containers: Vec<TestPostgres>,
}

impl TestPostgresPool {
    /// Create a new pool with the specified number of containers
    ///
    /// Pre-allocates all ports before starting any containers to avoid race conditions.
    pub async fn new(size: usize) -> Result<Self> {
        // Pre-allocate all ports at once to avoid race conditions
        let mut ports = Vec::with_capacity(size);
        for i in 0..size {
            let port = portpicker::pick_unused_port()
                .ok_or_else(|| anyhow::anyhow!("No available port for Postgres container {i}"))?;
            ports.push(port);
        }

        // Now start containers with pre-allocated ports
        let mut containers = Vec::with_capacity(size);
        for (i, port) in ports.into_iter().enumerate() {
            match TestPostgres::start_with_port(port).await {
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
        // Bind to 127.0.0.1 only for security and to avoid port conflicts
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                &container_name,
                "-p",
                &format!("127.0.0.1:{port}:6334"),
                "-p",
                &format!("127.0.0.1:{rest_port}:6333"),
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
            // Clean up container if it was created (even if it failed to start)
            let _ = Command::new("docker")
                .args(["rm", "-f", &container_name])
                .output();
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

        // Register for global cleanup
        register_container(instance.container_name.clone());

        Ok(instance)
    }

    /// Start a new Qdrant instance with pre-allocated ports
    ///
    /// This is used by TestQdrantPool to avoid port allocation race conditions.
    /// Uses health check polling to ensure the container is ready before returning.
    pub async fn start_with_ports(port: u16, rest_port: u16) -> Result<Self> {
        let container_name = format!("qdrant-test-{}", Uuid::new_v4());
        let temp_dir_name = format!("/tmp/qdrant-test-{}", Uuid::new_v4());
        let temp_dir = PathBuf::from(&temp_dir_name);

        // Create temp directory
        std::fs::create_dir_all(&temp_dir)
            .with_context(|| format!("Failed to create temp directory: {temp_dir_name}"))?;

        // Start Qdrant container with pre-allocated ports
        // Bind to 127.0.0.1 only for security and to avoid port conflicts
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                &container_name,
                "-p",
                &format!("127.0.0.1:{port}:6334"),
                "-p",
                &format!("127.0.0.1:{rest_port}:6333"),
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
            // Clean up container if it was created (even if it failed to start)
            let _ = Command::new("docker")
                .args(["rm", "-f", &container_name])
                .output();
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

        // Register for global cleanup
        register_container(instance.container_name.clone());

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

    /// Wait for Qdrant to become healthy using exponential backoff
    async fn wait_for_health(&self) -> Result<()> {
        let max_attempts = 20;
        let initial_delay = Duration::from_millis(50);
        let max_delay = Duration::from_millis(500);

        let mut delay = initial_delay;

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
                // Exponential backoff: double the delay, but cap at max_delay
                delay = std::cmp::min(delay * 2, max_delay);
            }
        }

        Err(anyhow::anyhow!(
            "Qdrant did not become healthy after {max_attempts} attempts"
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
        // Unregister from global cleanup
        unregister_container(&self.container_name);

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
                    .args(["rm", "-rf", self.temp_dir.to_string_lossy().as_ref()])
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
    ///
    /// Pre-allocates all ports before starting any containers to avoid race conditions.
    pub async fn new(size: usize) -> Result<Self> {
        // Pre-allocate all ports at once to avoid race conditions
        let mut ports = Vec::with_capacity(size);
        for i in 0..size {
            let port = portpicker::pick_unused_port()
                .ok_or_else(|| anyhow::anyhow!("No available port for Qdrant container {i}"))?;
            let rest_port = portpicker::pick_unused_port().ok_or_else(|| {
                anyhow::anyhow!("No available REST port for Qdrant container {i}")
            })?;
            ports.push((port, rest_port));
        }

        // Now start containers with pre-allocated ports
        let mut containers = Vec::with_capacity(size);
        for (i, (port, rest_port)) in ports.into_iter().enumerate() {
            match TestQdrant::start_with_ports(port, rest_port).await {
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

/// Test Outbox Processor instance running as a container
pub struct TestOutboxProcessor {
    container_id: String,
    container_name: String,
}

impl TestOutboxProcessor {
    /// Start a new outbox processor instance
    ///
    /// Connects to the provided Postgres and Qdrant instances.
    ///
    /// This uses a Docker container built from Dockerfile.outbox-processor
    pub fn start(
        postgres: &TestPostgres,
        qdrant: &TestQdrant,
        collection_name: &str,
    ) -> Result<Self> {
        // Ensure the Docker image is built (thread-safe, happens only once)
        ensure_outbox_image_built()?;

        let container_name = format!("outbox-processor-test-{}", Uuid::new_v4());

        // On Linux, use --network host to access localhost services
        // On macOS/Windows, use host.docker.internal
        let mut cmd = Command::new("docker");
        cmd.args(["run", "-d", "--name", &container_name]);

        let host = if cfg!(target_os = "linux") {
            cmd.arg("--network").arg("host");
            "localhost"
        } else {
            cmd.arg("--add-host")
                .arg("host.docker.internal:host-gateway");
            "host.docker.internal"
        };

        cmd.arg("-e")
            .arg(format!("POSTGRES_HOST={host}"))
            .arg("-e")
            .arg(format!("POSTGRES_PORT={}", postgres.port()))
            .arg("-e")
            .arg("POSTGRES_DATABASE=codesearch")
            .arg("-e")
            .arg("POSTGRES_USER=codesearch")
            .arg("-e")
            .arg("POSTGRES_PASSWORD=codesearch")
            .arg("-e")
            .arg(format!("QDRANT_HOST={host}"))
            .arg("-e")
            .arg(format!("QDRANT_PORT={}", qdrant.port()))
            .arg("-e")
            .arg(format!("QDRANT_REST_PORT={}", qdrant.rest_port()))
            .arg("-e")
            .arg(format!("QDRANT_COLLECTION={collection_name}"))
            .arg("-e")
            .arg("RUST_LOG=debug")
            .arg("codesearch-outbox:test");

        let output = cmd
            .output()
            .context("Failed to start outbox processor container")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Clean up container if it was created
            let _ = Command::new("docker")
                .args(["rm", "-f", &container_name])
                .output();
            return Err(anyhow::anyhow!(
                "Failed to start outbox processor container: {stderr}"
            ));
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let instance = Self {
            container_id,
            container_name: container_name.clone(),
        };

        // Register for global cleanup
        register_container(container_name);

        Ok(instance)
    }

    /// Get container logs for debugging
    pub fn get_logs(&self) -> String {
        let output = Command::new("docker")
            .args(["logs", "--tail", "100", &self.container_id])
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

    /// Stop the outbox processor
    fn cleanup(&self) {
        // Unregister from global cleanup
        unregister_container(&self.container_name);

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

/// Wait for the outbox table to be empty (all entries processed)
///
/// Polls the outbox table every 100ms until all unprocessed entries are gone
/// or the timeout is reached.
pub async fn wait_for_outbox_empty(
    postgres: &TestPostgres,
    db_name: &str,
    timeout: Duration,
) -> Result<()> {
    wait_for_outbox_empty_with_processor(postgres, db_name, timeout, None).await
}

/// Wait for the outbox table to be empty, with optional processor for debugging
///
/// Uses adaptive polling: starts with fast polling (50ms) and gradually backs off
/// to slower intervals (500ms max) to optimize for both fast response and low overhead.
async fn wait_for_outbox_empty_with_processor(
    postgres: &TestPostgres,
    db_name: &str,
    timeout: Duration,
    processor: Option<&TestOutboxProcessor>,
) -> Result<()> {
    use sqlx::PgPool;

    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/{db_name}",
        postgres.port()
    );

    let pool = PgPool::connect(&connection_url)
        .await
        .context("Failed to connect to Postgres for outbox polling")?;

    let start = std::time::Instant::now();

    // Adaptive polling intervals: check frequently at first, then back off
    let poll_intervals = [
        Duration::from_millis(50),  // First few checks: very fast
        Duration::from_millis(100), // Next checks: normal
        Duration::from_millis(200), // Later checks: slower
        Duration::from_millis(500), // Final checks: slowest
    ];

    let mut interval_idx = 0;

    loop {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM entity_outbox WHERE processed_at IS NULL")
                .fetch_one(&pool)
                .await
                .context("Failed to query outbox table")?;

        if count == 0 {
            pool.close().await;
            return Ok(());
        }

        if start.elapsed() >= timeout {
            pool.close().await;

            // Dump processor logs if available for debugging
            if let Some(proc) = processor {
                eprintln!("\n=== OUTBOX PROCESSOR LOGS ===");
                eprintln!("{}", proc.get_logs());
                eprintln!("=== END PROCESSOR LOGS ===\n");
            }

            return Err(anyhow::anyhow!(
                "Timeout waiting for outbox to be empty. {count} unprocessed entries remain after {timeout:?}"
            ));
        }

        // Adaptive interval: gradually increase delay
        let current_interval = poll_intervals[interval_idx];
        tokio::time::sleep(current_interval).await;

        // Gradually increase interval (backoff) up to the maximum
        if interval_idx < poll_intervals.len() - 1 {
            interval_idx += 1;
        }
    }
}

/// Start an outbox processor and wait for it to sync all pending entries
///
/// This is a convenience function that starts the processor and waits for
/// the outbox table to be empty with a 15-second timeout using adaptive polling.
/// No fixed sleep is needed - adaptive polling handles varying startup times efficiently.
pub async fn start_and_wait_for_outbox_sync(
    postgres: &TestPostgres,
    qdrant: &TestQdrant,
    collection_name: &str,
) -> Result<TestOutboxProcessor> {
    // Use default database name for backward compatibility
    start_and_wait_for_outbox_sync_with_db(postgres, qdrant, "codesearch", collection_name).await
}

/// Start an outbox processor and wait for it to sync all pending entries (with custom database)
///
/// This variant allows specifying a custom database name for database isolation.
/// Uses adaptive polling (50ms -> 500ms) to efficiently handle varying processor startup times.
pub async fn start_and_wait_for_outbox_sync_with_db(
    postgres: &TestPostgres,
    qdrant: &TestQdrant,
    db_name: &str,
    collection_name: &str,
) -> Result<TestOutboxProcessor> {
    let processor = TestOutboxProcessor::start(postgres, qdrant, collection_name)?;

    // No fixed sleep needed - adaptive polling starts checking immediately at 50ms intervals
    // and gradually backs off. This is much faster than a fixed 2s sleep when the processor
    // starts quickly, but still reliable when it takes longer.

    // Wait for outbox to be empty (15 second timeout, increased from 10s for safety)
    // with processor logs on failure
    wait_for_outbox_empty_with_processor(
        postgres,
        db_name,
        Duration::from_secs(15),
        Some(&processor),
    )
    .await?;

    Ok(processor)
}

impl Drop for TestOutboxProcessor {
    fn drop(&mut self) {
        self.cleanup();
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
