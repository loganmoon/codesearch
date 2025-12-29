//! Specification-based graph validation test infrastructure
//!
//! This module provides a test harness for validating that the code graph
//! extraction pipeline correctly identifies entities and relationships
//! from Rust source code.

mod assertions;
mod neo4j_queries;
mod schema;

// Re-export only what tests need
pub use schema::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
};

use super::containers::{
    create_test_database, drop_test_database, get_shared_neo4j, get_shared_postgres,
    get_shared_qdrant, wait_for_graph_ready, TestNeo4j, TestPostgres, TestQdrant,
};
use anyhow::{bail, Context, Result};
use assertions::{assert_entities_match, assert_relationships_match};
use codesearch_core::config::OutboxConfig;
use codesearch_embeddings::{EmbeddingManager, MockEmbeddingProvider};
use codesearch_indexer::create_indexer;
use codesearch_storage::QdrantConfig;
use neo4j_queries::{get_all_entities, get_all_relationships};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tracing::info;

/// RAII guard that ensures test database cleanup even on test failure
struct TestDatabaseGuard {
    postgres: Arc<TestPostgres>,
    db_name: String,
    cleaned_up: bool,
}

impl TestDatabaseGuard {
    fn new(postgres: Arc<TestPostgres>, db_name: String) -> Self {
        Self {
            postgres,
            db_name,
            cleaned_up: false,
        }
    }

    /// Mark as cleaned up (call this after successful explicit cleanup)
    fn mark_cleaned_up(&mut self) {
        self.cleaned_up = true;
    }
}

impl Drop for TestDatabaseGuard {
    fn drop(&mut self) {
        if !self.cleaned_up {
            // Use blocking cleanup since we're in a Drop implementation
            let postgres = self.postgres.clone();
            let db_name = self.db_name.clone();
            eprintln!(
                "TestDatabaseGuard: Cleaning up test database {} on drop",
                db_name
            );
            // Schedule cleanup on a new runtime since we can't use async in Drop
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().ok();
                if let Some(rt) = rt {
                    rt.block_on(async {
                        let _ = drop_test_database(&postgres, &db_name).await;
                    });
                }
            });
        }
    }
}

/// Run a specification validation test for a given fixture
///
/// This function:
/// 1. Creates a temporary repository with the fixture's source files
/// 2. Runs the indexer and outbox processor directly
/// 3. Waits for graph resolution to complete
/// 4. Queries Neo4j for actual entities and relationships
/// 5. Compares against the expected spec
pub async fn run_spec_validation(fixture: &Fixture) -> Result<()> {
    eprintln!("\n=== Running spec validation: {} ===\n", fixture.name);

    // Get shared test infrastructure
    let postgres = get_shared_postgres().await?;
    let qdrant = get_shared_qdrant().await?;
    let neo4j = get_shared_neo4j().await?;

    // Create isolated test database with RAII cleanup guard
    let db_name = create_test_database(&postgres).await?;
    let mut cleanup_guard = TestDatabaseGuard::new(postgres.clone(), db_name.clone());
    eprintln!("Created test database: {}", db_name);

    // Create temporary repository with fixture files
    let temp_dir = create_test_repository(fixture)?;
    let repo_path = temp_dir.path();
    eprintln!("Created test repository at: {}", repo_path.display());

    // Run indexer and outbox processor directly (instead of CLI subprocess)
    run_indexer_directly(repo_path, &qdrant, &postgres, &neo4j, &db_name).await?;

    eprintln!("Indexing completed, waiting for graph_ready flag...");
    wait_for_graph_ready(&postgres, &db_name, Duration::from_secs(30)).await?;

    eprintln!("Graph sync completed, querying results...");

    // Get repository_id from the database
    let repository_id = get_repository_id(&postgres, &db_name).await?;
    eprintln!("Repository ID: {}", repository_id);

    // Query actual entities and relationships
    let actual_entities = get_all_entities(&neo4j, &repository_id).await?;
    let actual_relationships = get_all_relationships(&neo4j, &repository_id).await?;

    eprintln!(
        "Found {} entities, {} relationships",
        actual_entities.len(),
        actual_relationships.len()
    );

    // Debug: print actual entities
    eprintln!("\nActual entities:");
    for e in &actual_entities {
        eprintln!("  {} {}", e.entity_type, e.qualified_name);
    }

    // Debug: print actual relationships
    eprintln!("\nActual relationships:");
    for r in &actual_relationships {
        eprintln!(
            "  {} {} -> {}",
            r.rel_type, r.from_qualified_name, r.to_qualified_name
        );
    }

    // Assert entities match
    assert_entities_match(fixture.entities, &actual_entities)
        .context("Entity validation failed")?;

    // Assert relationships match
    assert_relationships_match(fixture.relationships, &actual_relationships)
        .context("Relationship validation failed")?;

    eprintln!("\n=== {} PASSED ===\n", fixture.name);

    // Explicit cleanup and mark guard as cleaned up
    drop_test_database(&postgres, &db_name).await?;
    cleanup_guard.mark_cleaned_up();

    Ok(())
}

/// Create a temporary repository with the fixture's source files
fn create_test_repository(fixture: &Fixture) -> Result<TempDir> {
    use schema::ProjectType;

    let temp_dir = TempDir::new().context("Failed to create temp directory")?;
    let repo_path = temp_dir.path();

    // Determine base directory for source files based on project type
    let base_dir = match fixture.project_type {
        ProjectType::SingleCrate | ProjectType::BinaryCrate => {
            let src_dir = repo_path.join("src");
            fs::create_dir_all(&src_dir)?;
            src_dir
        }
        ProjectType::Workspace | ProjectType::Custom => repo_path.to_path_buf(),
    };

    // Write source files
    for (path, content) in fixture.files {
        let file_path = base_dir.join(path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&file_path, content)?;
    }

    // Create Cargo.toml (use custom or generate default based on project type)
    let cargo_toml = match fixture.cargo_toml {
        Some(custom) => custom.to_string(),
        None => match fixture.project_type {
            ProjectType::SingleCrate | ProjectType::BinaryCrate => r#"[package]
name = "test_crate"
version = "0.1.0"
edition = "2021"
"#
            .to_string(),
            ProjectType::Workspace => r#"[workspace]
members = ["crates/*"]
resolver = "2"
"#
            .to_string(),
            ProjectType::Custom => r#"[package]
name = "test_crate"
version = "0.1.0"
edition = "2021"
"#
            .to_string(),
        },
    };
    fs::write(repo_path.join("Cargo.toml"), cargo_toml)?;

    // Initialize git repository (required for indexing)
    run_git_command(repo_path, &["init"])?;
    run_git_command(repo_path, &["config", "user.email", "test@test.com"])?;
    run_git_command(repo_path, &["config", "user.name", "Test"])?;
    run_git_command(repo_path, &["add", "."])?;
    run_git_command(repo_path, &["commit", "-m", "Initial commit"])?;

    Ok(temp_dir)
}

/// Run a git command and verify it succeeds
fn run_git_command(repo_path: &Path, args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "git {} failed with status {}: {}",
            args.join(" "),
            output.status,
            stderr
        );
    }

    Ok(())
}

/// Run the indexer and outbox processor directly (instead of spawning CLI subprocess)
///
/// This approach ensures we're always testing the latest code in the workspace,
/// avoiding stale binary issues that can occur with subprocess execution.
async fn run_indexer_directly(
    repo_path: &Path,
    qdrant: &Arc<TestQdrant>,
    postgres: &Arc<TestPostgres>,
    neo4j: &Arc<TestNeo4j>,
    db_name: &str,
) -> Result<()> {
    use codesearch_core::StorageConfig;
    use codesearch_storage::create_postgres_client;

    // Create mock embedding manager
    let embedding_manager = Arc::new(EmbeddingManager::new(
        Arc::new(MockEmbeddingProvider::new(384)),
        "test-model-v1".to_string(),
    ));

    // Create storage config for postgres client
    let storage_config = StorageConfig {
        qdrant_host: "localhost".to_string(),
        qdrant_port: qdrant.port(),
        qdrant_rest_port: qdrant.rest_port(),
        auto_start_deps: false,
        docker_compose_file: None,
        postgres_host: "localhost".to_string(),
        postgres_port: postgres.port(),
        postgres_database: db_name.to_string(),
        postgres_user: "codesearch".to_string(),
        postgres_password: "codesearch".to_string(),
        postgres_pool_size: 5,
        max_entities_per_db_operation: 1000,
        neo4j_host: "localhost".to_string(),
        neo4j_bolt_port: neo4j.bolt_port(),
        neo4j_http_port: neo4j.http_port(),
        neo4j_user: "neo4j".to_string(),
        neo4j_password: String::new(),
    };

    // Create postgres client
    let postgres_client = create_postgres_client(&storage_config)
        .await
        .context("Failed to connect to Postgres")?;

    // Generate collection name and register the repository
    // ensure_repository generates a deterministic UUID from the path
    let collection_name = format!(
        "test_{}",
        repo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo")
    );

    // Insert repository record - this returns the deterministic UUID
    let repository_id = postgres_client
        .ensure_repository(repo_path, &collection_name, None)
        .await
        .context("Failed to register repository")?;

    info!(
        repository_id = %repository_id,
        collection_name = %collection_name,
        "Registered test repository"
    );

    // Create Qdrant config for outbox processor
    let qdrant_config = QdrantConfig {
        host: "localhost".to_string(),
        port: qdrant.port(),
        rest_port: qdrant.rest_port(),
    };

    // Create outbox processor drain channel
    let (outbox_drain_tx, outbox_drain_rx) = tokio::sync::oneshot::channel();

    // Spawn outbox processor as background task with drain mode
    let postgres_client_for_outbox = postgres_client.clone();
    let storage_config_for_outbox = storage_config.clone();
    let outbox_config = OutboxConfig::default();
    let outbox_handle = tokio::spawn(async move {
        codesearch_outbox_processor::start_outbox_processor_with_drain(
            postgres_client_for_outbox,
            &qdrant_config,
            storage_config_for_outbox,
            &outbox_config,
            outbox_drain_rx,
        )
        .await
    });

    info!("Outbox processor started (will drain after indexing completes)");

    // Create GitRepository
    let git_repo = codesearch_watcher::GitRepository::open(repo_path).ok();

    // Create and run indexer
    let indexer_config = codesearch_indexer::IndexerConfig::default();
    let mut indexer = create_indexer(
        repo_path.to_path_buf(),
        repository_id.to_string(),
        embedding_manager,
        None, // sparse_manager
        postgres_client,
        git_repo,
        indexer_config,
    )?;

    // Run indexing
    let result = indexer
        .index_repository()
        .await
        .context("Failed to index repository")?;

    info!(
        "Indexing completed: {} files, {} entities",
        result.stats().total_files(),
        result.stats().entities_extracted()
    );

    // Signal the outbox processor to drain remaining entries
    info!("Indexing complete. Signaling outbox processor to drain remaining entries...");
    let _ = outbox_drain_tx.send(());

    // Wait for outbox processor to finish draining
    match tokio::time::timeout(Duration::from_secs(60), outbox_handle).await {
        Ok(Ok(Ok(()))) => info!("Outbox processor drained and stopped successfully"),
        Ok(Ok(Err(e))) => bail!("Outbox processor failed: {e}"),
        Ok(Err(e)) => bail!("Outbox processor task panicked: {e}"),
        Err(_) => bail!("Outbox processor drain timed out after 60 seconds"),
    }

    Ok(())
}

/// Get the repository ID from the database
async fn get_repository_id(postgres: &Arc<TestPostgres>, db_name: &str) -> Result<String> {
    use sqlx::PgPool;

    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/{}",
        postgres.port(),
        db_name
    );

    let pool = PgPool::connect(&connection_url).await?;

    let repo_id: String =
        sqlx::query_scalar("SELECT repository_id::text FROM repositories LIMIT 1")
            .fetch_one(&pool)
            .await
            .context("Failed to get repository_id")?;

    pool.close().await;

    Ok(repo_id)
}
