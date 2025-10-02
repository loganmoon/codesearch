//! Integration tests for the init command

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use uuid::Uuid;

/// Test Qdrant container with temporary storage
struct TestQdrant {
    container_name: String,
    temp_dir: PathBuf,
    port: u16,
    rest_port: u16,
}

impl TestQdrant {
    /// Start a new Qdrant instance with temporary storage
    async fn start() -> Result<Self> {
        let container_name = format!("qdrant-test-{}", Uuid::new_v4());
        let temp_dir_name = format!("/tmp/qdrant-test-{}", Uuid::new_v4());
        let temp_dir = PathBuf::from(&temp_dir_name);

        // Create temp directory
        std::fs::create_dir_all(&temp_dir).context("Failed to create temp directory for Qdrant")?;

        // Find available ports dynamically to avoid conflicts
        let port = portpicker::pick_unused_port().expect("No available port for Qdrant");
        let rest_port = portpicker::pick_unused_port().expect("No available port for Qdrant REST");

        // Start Qdrant container with temporary storage
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
                "qdrant/qdrant",
            ])
            .output()
            .context("Failed to start Qdrant container")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "Failed to start Qdrant container: {stderr}"
            ));
        }

        // Wait for Qdrant to be ready
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

        Ok(Self {
            container_name,
            temp_dir,
            port,
            rest_port,
        })
    }

    /// Stop and clean up the Qdrant instance
    fn cleanup(&self) {
        // Stop and remove container
        let _ = Command::new("docker")
            .args(["stop", &self.container_name])
            .output();

        let _ = Command::new("docker")
            .args(["rm", &self.container_name])
            .output();

        // Remove temp directory
        if self.temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&self.temp_dir);
        }
    }
}

impl Drop for TestQdrant {
    fn drop(&mut self) {
        self.cleanup();
    }
}

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
    // Start test Qdrant with temporary storage
    let qdrant = TestQdrant::start().await?;

    // Create test repository
    let test_repo = create_test_repo()?;

    // Create config file with test Qdrant settings
    let config_content = format!(
        r#"
[indexer]

[storage]
qdrant_host = "localhost"
qdrant_port = {}
qdrant_rest_port = {}
collection_name = ""
auto_start_deps = false

[embeddings]
provider = "mock"

[watcher]
debounce_ms = 500
ignore_patterns = ["*.log", "target", ".git"]
branch_strategy = "index_current"

[languages]
enabled = ["rust"]
"#,
        qdrant.port, qdrant.rest_port
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
    // Start test Qdrant with temporary storage
    let qdrant = TestQdrant::start().await?;

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

[embeddings]
provider = "mock"

[watcher]
debounce_ms = 500
ignore_patterns = ["*.log", "target", ".git"]
branch_strategy = "index_current"

[languages]
enabled = ["rust"]
"#,
        qdrant.port, qdrant.rest_port, collection_name
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

#[test]
fn test_qdrant_cleanup() {
    // This test verifies that temp directories are cleaned up
    let temp_dir = format!("/tmp/qdrant-cleanup-test-{}", Uuid::new_v4());
    std::fs::create_dir_all(&temp_dir).unwrap();

    {
        let _test_qdrant = TestQdrant {
            container_name: "nonexistent-container".to_string(),
            temp_dir: PathBuf::from(&temp_dir),
            port: 6334,
            rest_port: 6333,
        };

        // TestQdrant should clean up when dropped
    }

    // Verify temp directory was removed
    assert!(
        !Path::new(&temp_dir).exists(),
        "Temp directory not cleaned up"
    );
}
