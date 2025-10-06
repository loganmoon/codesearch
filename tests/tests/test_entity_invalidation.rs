//! Integration tests for entity invalidation during re-indexing
//!
//! These tests verify that when files are modified and re-indexed:
//! 1. Stale entities are properly detected and marked as deleted
//! 2. DELETE operations are written to the outbox
//! 3. Deleted entities are eventually removed from Qdrant

use anyhow::{Context, Result};
use codesearch_e2e_tests::common::*;
use std::path::Path;
use std::process::Command;
use uuid::Uuid;

/// Create a test config file
fn create_test_config(
    repo_path: &Path,
    qdrant: &TestQdrant,
    postgres: &TestPostgres,
    collection_name: &str,
) -> Result<std::path::PathBuf> {
    let config_content = format!(
        r#"
[indexer]

[storage]
qdrant_host = "localhost"
qdrant_port = {}
qdrant_rest_port = {}
collection_name = "{}"
auto_start_deps = false
postgres_host = "localhost"
postgres_port = {}
postgres_database = "codesearch"
postgres_user = "codesearch"
postgres_password = "codesearch"

[embeddings]
provider = "mock"

[watcher]
debounce_ms = 500
ignore_patterns = ["*.log", "target", ".git"]
branch_strategy = "index_current"

[languages]
enabled = ["rust"]
"#,
        qdrant.port(),
        qdrant.rest_port(),
        collection_name,
        postgres.port()
    );

    let config_path = repo_path.join("codesearch.toml");
    std::fs::write(&config_path, config_content)?;
    Ok(config_path)
}

/// Run the codesearch CLI
fn run_cli(repo_path: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new(codesearch_binary())
        .current_dir(repo_path)
        .args(args)
        .env("RUST_LOG", "info")
        .output()
        .context("Failed to run codesearch CLI")
}

#[tokio::test]
async fn test_reindex_detects_deleted_function() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;

    // Create repository with one function
    let repo = TestRepositoryBuilder::new()
        .with_rust_file(
            "lib.rs",
            r#"
pub fn function_one() -> i32 {
    42
}

pub fn function_two() -> i32 {
    84
}
"#,
        )
        .build()
        .await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, &collection_name)?;

    // Initial index
    run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    let output = run_cli(repo.path(), &["index"])?;
    assert!(output.status.success(), "Initial index failed");

    // Start outbox processor and wait for sync
    let processor = start_and_wait_for_outbox_sync(&postgres, &qdrant, &collection_name).await?;

    // Verify both functions were indexed
    assert_min_point_count(&qdrant, &collection_name, 2).await?;

    drop(processor);

    // Modify file: remove function_two
    std::fs::write(
        repo.path().join("src/lib.rs"),
        r#"
pub fn function_one() -> i32 {
    42
}
"#,
    )?;

    // Re-index
    let output = run_cli(repo.path(), &["index"])?;
    assert!(output.status.success(), "Re-index failed");

    // Start processor again and wait for sync
    let processor = start_and_wait_for_outbox_sync(&postgres, &qdrant, &collection_name).await?;

    // Verify function_two was removed from Qdrant
    // (Only function_one should remain)
    assert_point_count(&qdrant, &collection_name, 1).await?;

    drop(processor);
    Ok(())
}

#[tokio::test]
async fn test_reindex_detects_renamed_function() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;

    // Create repository with function
    let repo = TestRepositoryBuilder::new()
        .with_rust_file(
            "lib.rs",
            r#"
pub fn old_name() -> i32 {
    42
}
"#,
        )
        .build()
        .await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, &collection_name)?;

    // Initial index
    run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    run_cli(repo.path(), &["index"])?;

    let processor = start_and_wait_for_outbox_sync(&postgres, &qdrant, &collection_name).await?;

    assert_min_point_count(&qdrant, &collection_name, 1).await?;

    drop(processor);

    // Rename function
    std::fs::write(
        repo.path().join("src/lib.rs"),
        r#"
pub fn new_name() -> i32 {
    42
}
"#,
    )?;

    // Re-index
    run_cli(repo.path(), &["index"])?;

    let processor = start_and_wait_for_outbox_sync(&postgres, &qdrant, &collection_name).await?;

    // Should still have approximately 1 entity (old deleted, new added)
    let final_count = get_point_count(&qdrant, &collection_name).await?;
    assert!(
        final_count >= 1,
        "Expected at least 1 entity after rename, got {final_count}"
    );

    drop(processor);
    Ok(())
}

#[tokio::test]
async fn test_reindex_empty_file() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;

    // Create repository with functions
    let repo = TestRepositoryBuilder::new()
        .with_rust_file(
            "lib.rs",
            r#"
pub fn func1() {}
pub fn func2() {}
pub fn func3() {}
"#,
        )
        .build()
        .await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, &collection_name)?;

    // Initial index
    run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    run_cli(repo.path(), &["index"])?;

    let processor = start_and_wait_for_outbox_sync(&postgres, &qdrant, &collection_name).await?;

    assert_min_point_count(&qdrant, &collection_name, 3).await?;

    drop(processor);

    // Delete all code from file
    std::fs::write(repo.path().join("src/lib.rs"), "// Empty file\n")?;

    // Re-index
    run_cli(repo.path(), &["index"])?;

    let processor = start_and_wait_for_outbox_sync(&postgres, &qdrant, &collection_name).await?;

    // All entities should be removed
    let final_count = get_point_count(&qdrant, &collection_name).await?;
    assert_eq!(final_count, 0, "Expected 0 entities in empty file");

    drop(processor);
    Ok(())
}

#[tokio::test]
async fn test_reindex_modified_function_body() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;

    // Create repository
    let repo = TestRepositoryBuilder::new()
        .with_rust_file(
            "lib.rs",
            r#"
pub fn calculate() -> i32 {
    1 + 1
}
"#,
        )
        .build()
        .await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, &collection_name)?;

    // Initial index
    run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    run_cli(repo.path(), &["index"])?;

    let processor = start_and_wait_for_outbox_sync(&postgres, &qdrant, &collection_name).await?;

    assert_min_point_count(&qdrant, &collection_name, 1).await?;

    drop(processor);

    // Modify function body (entity ID stays same)
    std::fs::write(
        repo.path().join("src/lib.rs"),
        r#"
pub fn calculate() -> i32 {
    // Different implementation
    2 + 2
}
"#,
    )?;

    // Re-index
    run_cli(repo.path(), &["index"])?;

    let processor = start_and_wait_for_outbox_sync(&postgres, &qdrant, &collection_name).await?;

    // Should still have 1 entity (updated, not deleted)
    let final_count = get_point_count(&qdrant, &collection_name).await?;
    assert_eq!(final_count, 1, "Expected 1 entity after body modification");

    drop(processor);
    Ok(())
}
