//! Specification-based graph validation test infrastructure
//!
//! This module provides a test harness for validating that the code graph
//! extraction pipeline correctly identifies entities and relationships
//! from Rust source code.

pub mod assertions;
pub mod neo4j_queries;
pub mod schema;

use super::containers::{
    create_test_database, drop_test_database, get_shared_neo4j, get_shared_postgres,
    get_shared_qdrant, wait_for_graph_ready,
    TestPostgres,
};
use super::run_cli_with_full_infra;
use anyhow::{Context, Result};
use assertions::{assert_entities_match, assert_relationships_match};
use neo4j_queries::{get_all_entities, get_all_relationships};
use schema::Fixture;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

/// Run a specification validation test for a given fixture
///
/// This function:
/// 1. Creates a temporary repository with the fixture's source files
/// 2. Runs the full indexing pipeline via CLI
/// 3. Waits for graph resolution to complete
/// 4. Queries Neo4j for actual entities and relationships
/// 5. Compares against the expected spec
pub async fn run_spec_validation(fixture: &Fixture) -> Result<()> {
    eprintln!("\n=== Running spec validation: {} ===\n", fixture.name);

    // Get shared test infrastructure
    let postgres = get_shared_postgres().await?;
    let qdrant = get_shared_qdrant().await?;
    let neo4j = get_shared_neo4j().await?;

    // Create isolated test database
    let db_name = create_test_database(&postgres).await?;
    eprintln!("Created test database: {}", db_name);

    // Create temporary repository with fixture files
    let temp_dir = create_test_repository(fixture)?;
    let repo_path = temp_dir.path();
    eprintln!("Created test repository at: {}", repo_path.display());

    // Create codesearch.toml config file with mock embeddings
    create_config_file(repo_path, &qdrant, &postgres, &db_name)?;

    // Run the indexing CLI
    let output = run_cli_with_full_infra(repo_path, &["index"], &qdrant, &postgres, &neo4j, &db_name)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        eprintln!("CLI stdout:\n{}", stdout);
        eprintln!("CLI stderr:\n{}", stderr);
        anyhow::bail!("CLI index command failed: {:?}", output.status);
    }

    eprintln!("Indexing completed, waiting for graph_ready flag...");

    // The CLI runs the outbox processor with drain mode, so relationships should be resolved
    // when the CLI exits. We just need to wait for the graph_ready flag to be set.
    wait_for_graph_ready(&postgres, &db_name, Duration::from_secs(30)).await?;

    eprintln!("Graph sync completed, querying results...");

    // Get repository_id from the database
    let repository_id = get_repository_id(&postgres, &db_name).await?;
    eprintln!("Repository ID: {}", repository_id);

    // Query actual entities and relationships
    let actual_entities = get_all_entities(&neo4j, &repository_id).await?;
    let actual_relationships = get_all_relationships(&neo4j, &repository_id).await?;

    eprintln!("Found {} entities, {} relationships", actual_entities.len(), actual_relationships.len());

    // Debug: print actual entities
    eprintln!("\nActual entities:");
    for e in &actual_entities {
        eprintln!("  {} {}", e.entity_type, e.qualified_name);
    }

    // Debug: print actual relationships
    eprintln!("\nActual relationships:");
    for r in &actual_relationships {
        eprintln!("  {} {} -> {}", r.rel_type, r.from_qualified_name, r.to_qualified_name);
    }

    // Assert entities match
    assert_entities_match(fixture.entities, &actual_entities)
        .context("Entity validation failed")?;

    // Assert relationships match
    assert_relationships_match(fixture.relationships, &actual_relationships)
        .context("Relationship validation failed")?;

    eprintln!("\n=== {} PASSED ===\n", fixture.name);

    // Cleanup
    drop_test_database(&postgres, &db_name).await?;

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
            ProjectType::SingleCrate | ProjectType::BinaryCrate => {
                r#"[package]
name = "test_crate"
version = "0.1.0"
edition = "2021"
"#
                .to_string()
            }
            ProjectType::Workspace => {
                r#"[workspace]
members = ["crates/*"]
resolver = "2"
"#
                .to_string()
            }
            ProjectType::Custom => {
                r#"[package]
name = "test_crate"
version = "0.1.0"
edition = "2021"
"#
                .to_string()
            }
        },
    };
    fs::write(repo_path.join("Cargo.toml"), cargo_toml)?;

    // Initialize git repository (required for indexing)
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo_path)
        .output()
        .context("Failed to init git repo")?;

    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo_path)
        .output()
        .context("Failed to set git email")?;

    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_path)
        .output()
        .context("Failed to set git name")?;

    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo_path)
        .output()
        .context("Failed to git add")?;

    std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(repo_path)
        .output()
        .context("Failed to git commit")?;

    Ok(temp_dir)
}

/// Create a codesearch.toml config file with mock embeddings
fn create_config_file(
    repo_path: &Path,
    qdrant: &super::containers::TestQdrant,
    postgres: &super::containers::TestPostgres,
    db_name: &str,
) -> Result<()> {
    let config = format!(
        r#"[indexer]

[storage]
qdrant_host = "localhost"
qdrant_port = {}
qdrant_rest_port = {}
auto_start_deps = false
postgres_host = "localhost"
postgres_port = {}
postgres_database = "{}"
postgres_user = "codesearch"
postgres_password = "codesearch"

[embeddings]
provider = "mock"

[watcher]
debounce_ms = 500
ignore_patterns = ["*.log", "target", ".git"]

[languages]
enabled = ["rust"]
"#,
        qdrant.port(),
        qdrant.rest_port(),
        postgres.port(),
        db_name
    );
    fs::write(repo_path.join("codesearch.toml"), config)?;
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
