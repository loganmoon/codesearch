//! Integration tests for the init command

mod e2e;

use anyhow::{Context, Result};
use e2e::containers::{TestPostgres, TestQdrant};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use uuid::Uuid;

/// Create a test repository with git initialized
fn create_test_repo() -> Result<TempDir> {
    let temp_dir = TempDir::new().context("Failed to create temp dir")?;

    // Initialize git repo
    Command::new("git")
        .current_dir(temp_dir.path())
        .args(["init"])
        .output()
        .context("Failed to init git repo")?;

    // Create a simple Rust file for testing
    let src_dir = temp_dir.path().join("src");
    std::fs::create_dir_all(&src_dir)?;

    std::fs::write(
        src_dir.join("main.rs"),
        "fn main() {\n    println!(\"Hello, world!\");\n}\n",
    )?;

    Ok(temp_dir)
}

#[tokio::test]
async fn test_init_command_creates_collection() -> Result<()> {
    // Start test Qdrant and Postgres with temporary storage
    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;

    // Create test repository
    let test_repo = create_test_repo()?;

    // Create config file with test Qdrant and Postgres settings
    let config_content = format!(
        r#"
[indexer]

[storage]
qdrant_host = "localhost"
qdrant_port = {}
qdrant_rest_port = {}
collection_name = ""
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
        postgres.port()
    );

    let config_path = test_repo.path().join("codesearch.toml");
    std::fs::write(&config_path, config_content)?;

    // Run init command using cargo run with manifest path
    let manifest_path = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let workspace_manifest = Path::new(&manifest_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Cargo.toml");

    let output = Command::new("cargo")
        .current_dir(test_repo.path())
        .args([
            "run",
            "--manifest-path",
            workspace_manifest.to_str().unwrap(),
            "--package",
            "codesearch",
            "--bin",
            "codesearch",
            "--",
            "init",
            "--config",
            config_path.to_str().unwrap(),
        ])
        .env("RUST_LOG", "info")
        .output()
        .context("Failed to run init command")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    println!("stdout: {stdout}");
    println!("stderr: {stderr}");

    // Check that init succeeded
    assert!(
        output.status.success(),
        "Init command failed: stdout={stdout}, stderr={stderr}"
    );

    // Verify success message in output
    assert!(
        stderr.contains("Repository initialized successfully")
            || stdout.contains("Repository initialized successfully"),
        "Expected success message not found"
    );

    // Verify collection name was generated
    let config = std::fs::read_to_string(&config_path)?;
    assert!(
        config.contains("collection_name = "),
        "Collection name not saved to config"
    );

    Ok(())
}

#[tokio::test]
async fn test_init_command_handles_existing_collection() -> Result<()> {
    // Start test Qdrant and Postgres with temporary storage
    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;

    // Create test repository
    let test_repo = create_test_repo()?;

    // Create config with specific collection name
    let collection_name = format!("test_collection_{}", Uuid::new_v4());
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

    let config_path = test_repo.path().join("codesearch.toml");
    std::fs::write(&config_path, config_content)?;

    // Run init command first time using cargo run with manifest path
    let manifest_path = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let workspace_manifest = Path::new(&manifest_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Cargo.toml");

    let output1 = Command::new("cargo")
        .current_dir(test_repo.path())
        .args([
            "run",
            "--manifest-path",
            workspace_manifest.to_str().unwrap(),
            "--package",
            "codesearch",
            "--bin",
            "codesearch",
            "--",
            "init",
            "--config",
            config_path.to_str().unwrap(),
        ])
        .output()
        .context("Failed to run first init command")?;

    assert!(output1.status.success(), "First init failed");

    // Run init command again - should handle existing collection gracefully
    let output2 = Command::new("cargo")
        .current_dir(test_repo.path())
        .args([
            "run",
            "--manifest-path",
            workspace_manifest.to_str().unwrap(),
            "--package",
            "codesearch",
            "--bin",
            "codesearch",
            "--",
            "init",
            "--config",
            config_path.to_str().unwrap(),
        ])
        .env("RUST_LOG", "info")
        .output()
        .context("Failed to run second init command")?;

    let stdout = String::from_utf8_lossy(&output2.stdout);
    let stderr = String::from_utf8_lossy(&output2.stderr);

    // Second init should also succeed
    assert!(
        output2.status.success(),
        "Second init command failed: stdout={stdout}, stderr={stderr}"
    );

    // Should still show success message
    assert!(
        stderr.contains("Repository initialized successfully")
            || stdout.contains("Repository initialized successfully"),
        "Expected success message not found on second run"
    );

    Ok(())
}
