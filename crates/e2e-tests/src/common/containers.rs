//! Container management for E2E tests

use anyhow::{Context, Result};
use std::sync::{Arc, OnceLock, Weak};
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

/// Global shared Neo4j instance (drops when last Arc is dropped)
static SHARED_NEO4J: OnceLock<TokioMutex<Weak<TestNeo4j>>> = OnceLock::new();

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
            .with_tag("18")
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

/// Create an isolated test database in the shared Postgres instance and run migrations
///
/// Each test gets its own database to maintain isolation while sharing the container.
/// The database name includes a UUID to ensure uniqueness.
/// Migrations are run automatically after database creation.
pub async fn create_test_database(postgres: &Arc<TestPostgres>) -> Result<String> {
    let db_name = format!("test_db_{}", Uuid::new_v4().simple());

    // Connect to default database to create new test database
    let admin_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/codesearch",
        postgres.port()
    );

    let admin_pool = sqlx::PgPool::connect(&admin_url).await?;
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&admin_pool)
        .await?;
    admin_pool.close().await;

    // Connect to the new test database and run migrations
    let test_db_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/{db_name}",
        postgres.port()
    );
    let test_pool = sqlx::PgPool::connect(&test_db_url).await?;

    // Run migrations (path relative to crates/e2e-tests/Cargo.toml)
    sqlx::migrate!("../../migrations")
        .run(&test_pool)
        .await
        .context("Failed to run migrations on test database")?;

    test_pool.close().await;

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

/// Clean up test data from Neo4j by repository_id
///
/// Deletes all nodes with the given repository_id to clean up after a test
/// while keeping the container running for other tests.
pub async fn drop_test_neo4j_data(neo4j: &Arc<TestNeo4j>, repository_id: &str) -> Result<()> {
    use neo4rs::{query, Graph};

    let graph = Graph::new(neo4j.bolt_url(), "", "")
        .await
        .context("Failed to connect to Neo4j for cleanup")?;

    // Delete all nodes with this repository_id and their relationships
    graph
        .run(
            query("MATCH (n {repository_id: $repository_id}) DETACH DELETE n")
                .param("repository_id", repository_id),
        )
        .await
        .context("Failed to delete Neo4j nodes for repository")?;

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
        let container = GenericImage::new("qdrant/qdrant", "v1.16.0-unprivileged")
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

/// Test Neo4j container using testcontainers-rs
pub struct TestNeo4j {
    #[allow(dead_code)]
    container: ContainerAsync<GenericImage>,
    bolt_port: u16,
    http_port: u16,
}

impl TestNeo4j {
    /// Start a new Neo4j instance
    ///
    /// Uses Neo4j Community Edition with authentication disabled for testing.
    pub async fn start() -> Result<Self> {
        // Note: with_wait_for must come before with_env_var since the latter
        // converts GenericImage to ContainerRequest<GenericImage>
        let container = GenericImage::new("neo4j", "5-community")
            .with_exposed_port(ContainerPort::Tcp(7687)) // Bolt protocol
            .with_exposed_port(ContainerPort::Tcp(7474)) // HTTP API
            .with_wait_for(WaitFor::message_on_stdout("Started."))
            .with_env_var("NEO4J_AUTH", "none") // Disable auth for tests
            .with_startup_timeout(Duration::from_secs(90))
            .start()
            .await
            .context("Failed to start Neo4j container")?;

        let bolt_port = container
            .get_host_port_ipv4(7687)
            .await
            .context("Failed to get Neo4j Bolt port")?;

        let http_port = container
            .get_host_port_ipv4(7474)
            .await
            .context("Failed to get Neo4j HTTP port")?;

        Ok(Self {
            container,
            bolt_port,
            http_port,
        })
    }

    /// Get the Bolt protocol port
    pub fn bolt_port(&self) -> u16 {
        self.bolt_port
    }

    /// Get the HTTP API port
    pub fn http_port(&self) -> u16 {
        self.http_port
    }

    /// Get the Bolt connection URL
    pub fn bolt_url(&self) -> String {
        format!("bolt://localhost:{}", self.bolt_port)
    }

    /// Get the HTTP API URL
    pub fn http_url(&self) -> String {
        format!("http://localhost:{}", self.http_port)
    }
}

/// Get or create the shared Neo4j instance
///
/// Returns an Arc to a global shared Neo4j container that is created once
/// and reused across all tests. Tests maintain isolation by using unique database names.
pub async fn get_shared_neo4j() -> Result<Arc<TestNeo4j>> {
    let lock = SHARED_NEO4J.get_or_init(|| TokioMutex::new(Weak::new()));
    let mut guard = lock.lock().await;

    if let Some(neo4j) = guard.upgrade() {
        // Reuse existing container
        Ok(neo4j)
    } else {
        // Create new container
        eprintln!("Starting shared Neo4j instance for all tests...");
        let neo4j = Arc::new(TestNeo4j::start().await?);
        *guard = Arc::downgrade(&neo4j);
        Ok(neo4j)
    }
}

/// Wait for the outbox table to be empty (all entries processed)
///
/// Polls the outbox table with adaptive intervals until all unprocessed entries are gone
/// or the timeout is reached.
pub async fn wait_for_outbox_empty(
    postgres: &Arc<TestPostgres>,
    db_name: &str,
    timeout: Duration,
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

/// Wait for graph to be marked ready for a repository
///
/// After outbox processing completes, the graph needs relationship resolution.
/// This polls the `graph_ready` flag in the repositories table until it's true.
pub async fn wait_for_graph_ready(
    postgres: &Arc<TestPostgres>,
    db_name: &str,
    timeout: Duration,
) -> Result<()> {
    use sqlx::PgPool;

    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/{db_name}",
        postgres.port()
    );

    let pool = PgPool::connect(&connection_url)
        .await
        .context("Failed to connect to Postgres for graph_ready polling")?;

    let start = std::time::Instant::now();

    loop {
        // Check if any repository has graph_ready = false
        let not_ready_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM repositories WHERE graph_ready = false")
                .fetch_one(&pool)
                .await
                .context("Failed to query repositories table")?;

        if not_ready_count == 0 {
            pool.close().await;
            return Ok(());
        }

        if start.elapsed() >= timeout {
            pool.close().await;
            return Err(anyhow::anyhow!(
                "Timeout waiting for graph_ready. {not_ready_count} repositories still pending after {timeout:?}"
            ));
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires Docker
    async fn test_qdrant_starts_and_is_healthy() -> Result<()> {
        let qdrant = TestQdrant::start().await?;

        // Verify we can connect to REST API
        let health_url = format!("{}/healthz", qdrant.rest_url());
        let response = reqwest::get(&health_url).await?;
        assert!(response.status().is_success());

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires Docker
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

    #[tokio::test]
    #[ignore] // Requires Docker
    async fn test_neo4j_starts_and_is_healthy() -> Result<()> {
        let neo4j = TestNeo4j::start().await?;

        // Verify we can connect to Neo4j HTTP API
        let health_url = format!("{}/", neo4j.http_url());
        let response = reqwest::get(&health_url).await?;
        assert!(response.status().is_success());

        Ok(())
    }
}
