//! Container management for E2E tests

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Once, OnceLock, Weak};
use std::time::Duration;
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use testcontainers_modules::postgres::Postgres;
use tokio::sync::Mutex as TokioMutex;
use uuid::Uuid;

/// Global shared Qdrant instance (drops when last Arc is dropped)
static SHARED_QDRANT: OnceLock<TokioMutex<Weak<TestQdrant>>> = OnceLock::new();

/// Global shared Postgres instance (drops when last Arc is dropped)
static SHARED_POSTGRES: OnceLock<TokioMutex<Weak<TestPostgres>>> = OnceLock::new();

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

/// Test Postgres container using testcontainers-rs
pub struct TestPostgres {
    container: ContainerAsync<Postgres>,
    port: u16,
}

impl TestPostgres {
    /// Start a new Postgres instance
    pub async fn start() -> Result<Self> {
        let container = Postgres::default()
            .with_user("codesearch")
            .with_password("codesearch")
            .with_db_name("codesearch")
            .start()
            .await
            .context("Failed to start Postgres container")?;

        let port = container
            .get_host_port_ipv4(5432)
            .await
            .context("Failed to get Postgres port")?;

        Ok(Self { container, port })
    }

    /// Get the Postgres port
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get connection string for a specific database
    ///
    /// Uses configured credentials: codesearch/codesearch
    pub fn connection_string(&self, db_name: &str) -> String {
        format!(
            "postgresql://codesearch:codesearch@localhost:{}/{db_name}",
            self.port
        )
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

/// Get or create the shared Qdrant instance
///
/// Returns an Arc to a global shared Qdrant container that is created once
/// and reused across all tests. Tests maintain isolation by using unique collection names.
pub async fn get_shared_qdrant() -> Result<Arc<TestQdrant>> {
    let lock = SHARED_QDRANT.get_or_init(|| TokioMutex::new(Weak::new()));
    let mut guard = lock.lock().await;

    if let Some(qdrant) = guard.upgrade() {
        // Reuse existing container
        Ok(qdrant)
    } else {
        // Create new container
        eprintln!("🚀 Starting shared Qdrant instance for all tests...");
        let qdrant = Arc::new(TestQdrant::start().await?);
        *guard = Arc::downgrade(&qdrant);
        Ok(qdrant)
    }
}

/// Get or create the shared Postgres instance
///
/// Returns an Arc to a global shared Postgres container that is created once
/// and reused across all tests. Tests maintain isolation by creating unique databases.
pub async fn get_shared_postgres() -> Result<Arc<TestPostgres>> {
    let lock = SHARED_POSTGRES.get_or_init(|| TokioMutex::new(Weak::new()));
    let mut guard = lock.lock().await;

    if let Some(postgres) = guard.upgrade() {
        // Reuse existing container
        Ok(postgres)
    } else {
        // Create new container
        eprintln!("🚀 Starting shared Postgres instance for all tests...");
        let postgres = Arc::new(TestPostgres::start().await?);
        *guard = Arc::downgrade(&postgres);
        Ok(postgres)
    }
}

/// Create an isolated test database in the shared Postgres instance
///
/// Each test gets its own database to maintain isolation while sharing the container.
/// The database name includes a UUID to ensure uniqueness.
pub async fn create_test_database(postgres: &Arc<TestPostgres>) -> Result<String> {
    let db_name = format!("test_db_{}", Uuid::new_v4().simple());
    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/codesearch",
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
pub async fn drop_test_database(postgres: &Arc<TestPostgres>, db_name: &str) -> Result<()> {
    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/codesearch",
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
pub async fn drop_test_collection(qdrant: &Arc<TestQdrant>, collection_name: &str) -> Result<()> {
    let url = format!("{}/collections/{collection_name}", qdrant.rest_url());
    let _ = reqwest::Client::new().delete(&url).send().await?;
    Ok(())
}

/// Test Qdrant container using testcontainers-rs
pub struct TestQdrant {
    container: ContainerAsync<GenericImage>,
    rest_port: u16,
    grpc_port: u16,
}

impl TestQdrant {
    /// Start a new Qdrant instance
    pub async fn start() -> Result<Self> {
        let container = GenericImage::new("qdrant/qdrant", "latest-unprivileged")
            .with_exposed_port(ContainerPort::Tcp(6333))
            .with_exposed_port(ContainerPort::Tcp(6334))
            .with_wait_for(WaitFor::message_on_stdout("Qdrant gRPC listening"))
            .with_startup_timeout(Duration::from_secs(60))
            .start()
            .await
            .context("Failed to start Qdrant container")?;

        let rest_port = container
            .get_host_port_ipv4(6333)
            .await
            .context("Failed to get Qdrant REST port")?;

        let grpc_port = container
            .get_host_port_ipv4(6334)
            .await
            .context("Failed to get Qdrant gRPC port")?;

        Ok(Self {
            container,
            rest_port,
            grpc_port,
        })
    }

    /// Get the gRPC port
    pub fn port(&self) -> u16 {
        self.grpc_port
    }

    /// Get the REST API port
    pub fn rest_port(&self) -> u16 {
        self.rest_port
    }

    /// Get the REST API URL
    pub fn rest_url(&self) -> String {
        format!("http://localhost:{}", self.rest_port)
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
        postgres: &Arc<TestPostgres>,
        qdrant: &Arc<TestQdrant>,
        db_name: &str,
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
            .arg(format!("POSTGRES_DATABASE={db_name}"))
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

        Ok(Self {
            container_id,
            container_name,
        })
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
    postgres: &Arc<TestPostgres>,
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
    postgres: &Arc<TestPostgres>,
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
        Duration::from_millis(10),  // First few checks: very fast
        Duration::from_millis(50),  // Next checks: fast
        Duration::from_millis(100), // Later checks: normal
        Duration::from_millis(200), // Final checks: slower
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

        let current_interval = poll_intervals[interval_idx];
        tokio::time::sleep(current_interval).await;

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
    postgres: &Arc<TestPostgres>,
    qdrant: &Arc<TestQdrant>,
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
    postgres: &Arc<TestPostgres>,
    qdrant: &Arc<TestQdrant>,
    db_name: &str,
    collection_name: &str,
) -> Result<TestOutboxProcessor> {
    let processor = TestOutboxProcessor::start(postgres, qdrant, db_name, collection_name)?;

    // Wait for outbox to be empty (5 second timeout optimized for tests)
    // with processor logs on failure
    wait_for_outbox_empty_with_processor(
        postgres,
        db_name,
        Duration::from_secs(5),
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
    async fn test_postgres_starts_and_is_healthy() -> Result<()> {
        let postgres = TestPostgres::start().await?;

        // Verify we can connect to Postgres with configured credentials
        let connection_string = format!(
            "postgresql://codesearch:codesearch@localhost:{}/codesearch",
            postgres.port()
        );
        let pool = sqlx::PgPool::connect(&connection_string).await?;
        sqlx::query("SELECT 1").execute(&pool).await?;

        Ok(())
    }
}
