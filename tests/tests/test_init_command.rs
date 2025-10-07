//! Integration tests for the init command

use anyhow::{Context, Result};
use codesearch_e2e_tests::common::{codesearch_binary, containers::start_test_containers};
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
#[ignore]
async fn test_init_command_creates_collection() -> Result<()> {
    let (qdrant, postgres) = start_test_containers().await?;
    let test_repo = create_test_repo()?;

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

    let output = Command::new(codesearch_binary())
        .current_dir(test_repo.path())
        .args(["init", "--config", config_path.to_str().unwrap()])
        .env("RUST_LOG", "info")
        .output()
        .context("Failed to run init command")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    println!("stdout: {stdout}");
    println!("stderr: {stderr}");

    assert!(
        output.status.success(),
        "Init command failed: stdout={stdout}, stderr={stderr}"
    );

    assert!(
        stderr.contains("Repository initialized successfully")
            || stdout.contains("Repository initialized successfully"),
        "Expected success message not found"
    );

    let config = std::fs::read_to_string(&config_path)?;
    assert!(
        config.contains("collection_name = "),
        "Collection name not saved to config"
    );

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_init_command_handles_existing_collection() -> Result<()> {
    let (qdrant, postgres) = start_test_containers().await?;
    let test_repo = create_test_repo()?;
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

    let output1 = Command::new(codesearch_binary())
        .current_dir(test_repo.path())
        .args(["init", "--config", config_path.to_str().unwrap()])
        .output()
        .context("Failed to run first init command")?;

    assert!(output1.status.success(), "First init failed");

    let output2 = Command::new(codesearch_binary())
        .current_dir(test_repo.path())
        .args(["init", "--config", config_path.to_str().unwrap()])
        .env("RUST_LOG", "info")
        .output()
        .context("Failed to run second init command")?;

    let stdout = String::from_utf8_lossy(&output2.stdout);
    let stderr = String::from_utf8_lossy(&output2.stderr);

    assert!(
        output2.status.success(),
        "Second init command failed: stdout={stdout}, stderr={stderr}"
    );

    assert!(
        stderr.contains("Repository initialized successfully")
            || stdout.contains("Repository initialized successfully"),
        "Expected success message not found on second run"
    );

    Ok(())
}
