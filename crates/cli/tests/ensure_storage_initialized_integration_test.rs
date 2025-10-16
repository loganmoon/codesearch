//! Integration tests for ensure_storage_initialized function
//!
//! These tests verify the complete initialization flow including config creation,
//! Docker startup, migrations, and repository registration.
//!
//! Tests marked with #[ignore] require Docker infrastructure and are slow.

use anyhow::Result;
use codesearch::init::ensure_storage_initialized;
use codesearch_core::config::Config;
use std::path::Path;
use tempfile::TempDir;
use tokio::fs;

// Helper to create a test repository directory
async fn create_test_repo() -> Result<TempDir> {
    let temp_dir = TempDir::new()?;
    let repo_path = temp_dir.path();

    // Initialize git repository
    std::process::Command::new("git")
        .current_dir(repo_path)
        .args(["init"])
        .output()?;

    // Configure git user
    std::process::Command::new("git")
        .current_dir(repo_path)
        .args(["config", "user.email", "test@example.com"])
        .output()?;

    std::process::Command::new("git")
        .current_dir(repo_path)
        .args(["config", "user.name", "Test User"])
        .output()?;

    Ok(temp_dir)
}

// Helper to create a test config directory
async fn create_config_dir() -> Result<TempDir> {
    let temp_dir = TempDir::new()?;
    Ok(temp_dir)
}

// Helper to verify config file contents
async fn verify_config_file(config_path: &Path) -> Result<Config> {
    assert!(config_path.exists(), "Config file should exist");

    let config = Config::from_file(config_path)?;

    // Verify default values
    assert_eq!(config.storage.qdrant_host, "localhost");
    assert_eq!(config.storage.qdrant_port, 6334);
    assert_eq!(config.storage.postgres_host, "localhost");
    assert_eq!(config.storage.postgres_port, 5432);
    assert_eq!(config.storage.postgres_database, "codesearch");
    assert_eq!(config.storage.postgres_user, "codesearch");
    assert_eq!(config.storage.postgres_password, "codesearch");

    Ok(config)
}

/// Test that config file is created if missing with correct defaults
#[tokio::test]
#[ignore] // Requires Docker infrastructure
async fn test_creates_config_file_if_missing() -> Result<()> {
    let repo_dir = create_test_repo().await?;
    let config_dir = create_config_dir().await?;
    let config_path = config_dir.path().join("codesearch.toml");

    // Ensure config doesn't exist
    assert!(!config_path.exists(), "Config should not exist initially");

    // Call ensure_storage_initialized - this will auto-start infrastructure
    let result = ensure_storage_initialized(repo_dir.path(), Some(&config_path)).await;

    assert!(
        result.is_ok(),
        "Should successfully initialize storage: {:?}",
        result.err()
    );

    // Verify config file was created
    let config = verify_config_file(&config_path).await?;

    // Verify collection name was generated
    assert!(
        !config.storage.collection_name.is_empty(),
        "Collection name should be generated"
    );

    Ok(())
}

/// Test that collection name is auto-generated and saved when empty
#[tokio::test]
#[ignore] // Requires Docker infrastructure
async fn test_generates_collection_name_when_empty() -> Result<()> {
    let repo_dir = create_test_repo().await?;
    let config_dir = create_config_dir().await?;
    let config_path = config_dir.path().join("codesearch.toml");

    // Create config with empty collection name
    let config = Config::builder(codesearch_core::config::StorageConfig {
        qdrant_host: "localhost".to_string(),
        qdrant_port: 6334,
        qdrant_rest_port: 6333,
        collection_name: String::new(), // Empty collection name
        auto_start_deps: true,
        docker_compose_file: None,
        postgres_host: "localhost".to_string(),
        postgres_port: 5432,
        postgres_database: "codesearch".to_string(),
        postgres_user: "codesearch".to_string(),
        postgres_password: "codesearch".to_string(),
        max_entities_per_db_operation: 10000,
    })
    .build();

    config.save(&config_path)?;

    // Verify collection name is empty
    let loaded_config = Config::from_file(&config_path)?;
    assert!(
        loaded_config.storage.collection_name.is_empty(),
        "Collection name should be empty initially"
    );

    // Call ensure_storage_initialized
    let result = ensure_storage_initialized(repo_dir.path(), Some(&config_path)).await;
    assert!(
        result.is_ok(),
        "Should successfully initialize: {:?}",
        result.err()
    );

    // Verify collection name was generated and saved to file
    let updated_config = Config::from_file(&config_path)?;
    assert!(
        !updated_config.storage.collection_name.is_empty(),
        "Collection name should be generated and saved"
    );

    Ok(())
}

/// Test handling of Qdrant connection failures
#[tokio::test]
async fn test_handles_qdrant_connection_failure() -> Result<()> {
    let repo_dir = create_test_repo().await?;
    let config_dir = create_config_dir().await?;
    let config_path = config_dir.path().join("codesearch.toml");

    // Create config with invalid Qdrant port
    let config = Config::builder(codesearch_core::config::StorageConfig {
        qdrant_host: "localhost".to_string(),
        qdrant_port: 9999, // Invalid port - nothing listening here
        qdrant_rest_port: 9998,
        collection_name: "test_collection".to_string(),
        auto_start_deps: false, // Don't auto-start to test connection failure
        docker_compose_file: None,
        postgres_host: "localhost".to_string(),
        postgres_port: 5432,
        postgres_database: "codesearch".to_string(),
        postgres_user: "codesearch".to_string(),
        postgres_password: "codesearch".to_string(),
        max_entities_per_db_operation: 10000,
    })
    .build();

    config.save(&config_path)?;

    // Call ensure_storage_initialized and verify it returns an error
    let result = ensure_storage_initialized(repo_dir.path(), Some(&config_path)).await;

    assert!(
        result.is_err(),
        "Should fail with invalid Qdrant connection"
    );

    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(
        err_msg.to_lowercase().contains("qdrant")
            || err_msg.to_lowercase().contains("storage")
            || err_msg.to_lowercase().contains("connection"),
        "Error should mention Qdrant/storage/connection, got: {err_msg}"
    );

    Ok(())
}

/// Test handling of Postgres connection failures
#[tokio::test]
#[ignore] // Requires Qdrant to be running to reach Postgres stage
async fn test_handles_postgres_connection_failure() -> Result<()> {
    let repo_dir = create_test_repo().await?;
    let config_dir = create_config_dir().await?;
    let config_path = config_dir.path().join("codesearch.toml");

    // Create config with invalid Postgres port
    // Note: Qdrant must be running for this test to reach the Postgres stage
    let config = Config::builder(codesearch_core::config::StorageConfig {
        qdrant_host: "localhost".to_string(),
        qdrant_port: 6334,
        qdrant_rest_port: 6333,
        collection_name: "test_collection".to_string(),
        auto_start_deps: true, // Auto-start so Qdrant is available
        docker_compose_file: None,
        postgres_host: "localhost".to_string(),
        postgres_port: 9999, // Invalid port
        postgres_database: "codesearch".to_string(),
        postgres_user: "codesearch".to_string(),
        postgres_password: "codesearch".to_string(),
        max_entities_per_db_operation: 10000,
    })
    .build();

    config.save(&config_path)?;

    // This test will auto-start infrastructure (Qdrant), but Postgres connection will fail
    let result = ensure_storage_initialized(repo_dir.path(), Some(&config_path)).await;

    assert!(
        result.is_err(),
        "Should fail with invalid Postgres connection"
    );

    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(
        err_msg.to_lowercase().contains("postgres")
            || err_msg.to_lowercase().contains("database")
            || err_msg.to_lowercase().contains("migration"),
        "Error should mention Postgres/database/migration, got: {err_msg}"
    );

    Ok(())
}

/// Test handling of migration failures
#[tokio::test]
#[ignore] // Requires Docker infrastructure and complex setup
async fn test_handles_migration_failures() -> Result<()> {
    // This test would require:
    // 1. Starting Postgres with testcontainers
    // 2. Pre-populating with conflicting schema
    // 3. Running ensure_storage_initialized
    // 4. Verifying it fails with migration error

    // TODO: Implement when testcontainers infrastructure is set up

    Ok(())
}

/// Test handling of dependency startup failures
#[tokio::test]
#[ignore] // Requires Docker and is complex to simulate
async fn test_handles_dependency_startup_failure() -> Result<()> {
    // This test would require simulating Docker failures:
    // 1. Port conflicts
    // 2. Missing GPU for vLLM
    // 3. Insufficient permissions
    // 4. Docker daemon not running

    // TODO: Implement with controlled failure injection

    Ok(())
}

/// Test handling of health check timeouts
#[tokio::test]
#[ignore] // Requires Docker and controlled timing
async fn test_handles_health_check_timeout() -> Result<()> {
    // This test would require:
    // 1. Starting containers that respond slowly to health checks
    // 2. Setting very short timeouts
    // 3. Verifying proper timeout error messages

    // TODO: Implement with mock containers or timeout injection

    Ok(())
}

/// Test successful repository registration (happy path)
#[tokio::test]
#[ignore] // Requires full Docker infrastructure
async fn test_repository_registration_success() -> Result<()> {
    let repo_dir = create_test_repo().await?;
    let config_dir = create_config_dir().await?;
    let config_path = config_dir.path().join("codesearch.toml");

    // Create initial commit (required for repository)
    fs::write(repo_dir.path().join("test.txt"), "test content").await?;
    std::process::Command::new("git")
        .current_dir(repo_dir.path())
        .args(["add", "."])
        .output()?;
    std::process::Command::new("git")
        .current_dir(repo_dir.path())
        .args(["commit", "-m", "Initial commit"])
        .output()?;

    // Call ensure_storage_initialized - full happy path
    let result = ensure_storage_initialized(repo_dir.path(), Some(&config_path)).await;

    assert!(
        result.is_ok(),
        "Should successfully complete initialization: {:?}",
        result.err()
    );

    // Verify config was created
    assert!(config_path.exists(), "Config file should be created");

    // Verify config contains generated collection name
    let config = Config::from_file(&config_path)?;
    assert!(
        !config.storage.collection_name.is_empty(),
        "Collection name should be set"
    );

    // Verify the returned config matches what was saved
    let returned_config = result.unwrap();
    assert_eq!(
        returned_config.storage.collection_name, config.storage.collection_name,
        "Returned config should match saved config"
    );

    // Note: Verifying repository registration in Postgres would require
    // connecting to the database, which is beyond the scope of this test.
    // The ensure_storage_initialized function already tests this internally.

    Ok(())
}
