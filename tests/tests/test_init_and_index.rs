//! End-to-end tests for the complete codesearch pipeline
//!
//! These tests validate the full workflow: index → search
//! using isolated Qdrant containers with temporary storage.
//! The index command automatically initializes storage if needed.
//!
//! ## Running Tests
//!
//! The outbox_processor binary is automatically built on first test run if needed.
//! For best results when running tests in parallel:
//!
//! ```bash
//! cargo build --bin outbox_processor  # Optional: pre-build to avoid first-run delay
//! cargo test --workspace              # Tests can now run in parallel safely
//! ```
//!
//! Run with: cargo test --test e2e_tests
//! Verbose: CODESEARCH_TEST_LOG=debug cargo test --test e2e_tests

use anyhow::{Context, Result};
use codesearch_core::config::StorageConfig;
use codesearch_e2e_tests::common::*;
use codesearch_embeddings::{EmbeddingProvider, MockEmbeddingProvider};
use codesearch_storage::{create_collection_manager, create_storage_client};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use uuid::Uuid;

/// Create a test config file for the given repository and test instances
fn create_test_config(
    repo_path: &Path,
    qdrant: &TestQdrant,
    postgres: &TestPostgres,
    db_name: &str,
    collection_name: Option<&str>,
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
        collection_name.unwrap_or(""),
        postgres.port(),
        db_name
    );

    let config_path = repo_path.join("codesearch.toml");
    std::fs::write(&config_path, config_content)?;
    Ok(config_path)
}

/// Run the codesearch CLI with the given arguments
fn run_cli(repo_path: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new(codesearch_binary())
        .current_dir(repo_path)
        .args(args)
        .env("RUST_LOG", "info")
        .output()
        .context("Failed to run codesearch CLI")
}

#[tokio::test]
#[ignore]
async fn test_index_creates_collection_in_qdrant() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;

    let db_name = create_test_database(&postgres).await?;

    let repo = simple_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());

    // Create config pointing to test instances
    let _config_path = create_test_config(
        repo.path(),
        &qdrant,
        &postgres,
        &db_name,
        Some(&collection_name),
    )?;

    // Run index command - it will automatically initialize storage
    let output = run_cli(repo.path(), &["index"])?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "Index command failed: stdout={stdout}, stderr={stderr}"
    );

    // Verify collection was created
    assert_collection_exists(&qdrant, &collection_name).await?;

    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_index_stores_entities_in_qdrant() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;

    let db_name = create_test_database(&postgres).await?;

    let repo = multi_file_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let _config_path = create_test_config(
        repo.path(),
        &qdrant,
        &postgres,
        &db_name,
        Some(&collection_name),
    )?;

    // Run index - it will automatically initialize storage if needed
    let index_output = run_cli(repo.path(), &["index"])?;

    let stdout = String::from_utf8_lossy(&index_output.stdout);
    let stderr = String::from_utf8_lossy(&index_output.stderr);

    assert!(
        index_output.status.success(),
        "Index command failed: stdout={stdout}, stderr={stderr}"
    );

    let processor =
        start_and_wait_for_outbox_sync_with_db(&postgres, &qdrant, &db_name, &collection_name)
            .await?;

    assert_min_point_count(&qdrant, &collection_name, 10).await?;

    drop(processor); // Clean up processor

    assert_collection_exists(&qdrant, &collection_name).await?;

    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_index_with_mock_embeddings() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;

    let db_name = create_test_database(&postgres).await?;

    let repo = simple_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let _config_path = create_test_config(
        repo.path(),
        &qdrant,
        &postgres,
        &db_name,
        Some(&collection_name),
    )?;

    // Run index - it will automatically initialize storage if needed
    let output = run_cli(repo.path(), &["index"])?;

    assert!(output.status.success(), "Index with mock embeddings failed");

    let processor =
        start_and_wait_for_outbox_sync_with_db(&postgres, &qdrant, &db_name, &collection_name)
            .await?;

    assert_min_point_count(&qdrant, &collection_name, 3).await?;

    drop(processor); // Clean up processor

    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_search_finds_relevant_entities() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;

    let db_name = create_test_database(&postgres).await?;

    let repo = multi_file_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let _config_path = create_test_config(
        repo.path(),
        &qdrant,
        &postgres,
        &db_name,
        Some(&collection_name),
    )?;

    // Index the repository - it will automatically initialize storage if needed
    run_cli(repo.path(), &["index"])?;

    let processor =
        start_and_wait_for_outbox_sync_with_db(&postgres, &qdrant, &db_name, &collection_name)
            .await?;

    // Create storage client for programmatic search
    let storage_config = StorageConfig {
        qdrant_host: "localhost".to_string(),
        qdrant_port: qdrant.port(),
        qdrant_rest_port: qdrant.rest_port(),
        collection_name: collection_name.clone(),
        auto_start_deps: false,
        docker_compose_file: None,
        postgres_host: "localhost".to_string(),
        postgres_port: postgres.port(),
        postgres_database: "codesearch".to_string(),
        postgres_user: "codesearch".to_string(),
        postgres_password: "codesearch".to_string(),
    };

    let storage_client = create_storage_client(&storage_config, &collection_name).await?;

    // Create mock embedding for search query with matching dimensions (1536 for BAAI/bge-code-v1)
    let mock_provider = Arc::new(MockEmbeddingProvider::new(1536));
    let query_embedding = mock_provider
        .embed(vec!["Calculator add method".to_string()])
        .await?
        .into_iter()
        .next()
        .context("Failed to get query embedding")?
        .context("Embedding was None")?;

    // Perform search - now returns (entity_id, repository_id, score) tuples
    let results = storage_client
        .search_similar(query_embedding, 5, None)
        .await?;

    assert!(
        !results.is_empty(),
        "Search should return at least some results"
    );

    for (entity_id, repository_id, _score) in &results {
        assert!(!entity_id.is_empty(), "Entity ID should not be empty");
        assert!(
            !repository_id.is_empty(),
            "Repository ID should not be empty"
        );
    }

    drop(processor); // Clean up processor

    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

#[tokio::test]
#[ignore] // Requires docker compose up vllm-embeddings before running
async fn test_complete_pipeline_with_real_embeddings() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;

    let db_name = create_test_database(&postgres).await?;

    let repo = complex_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());

    // Create config with LocalApi provider using manual vLLM service
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
postgres_database = "{}"
postgres_user = "codesearch"
postgres_password = "codesearch"

[embeddings]
provider = "localapi"
api_url = "http://localhost:8000/v1"
model_name = "BAAI/bge-small-en-v1.5"

[watcher]
debounce_ms = 500
ignore_patterns = ["*.log", "target", ".git"]

[languages]
enabled = ["rust"]
"#,
        qdrant.port(),
        qdrant.rest_port(),
        collection_name,
        postgres.port(),
        db_name
    );

    let config_path = repo.path().join("codesearch.toml");
    std::fs::write(&config_path, config_content)?;

    // Run index - it will automatically initialize storage if needed
    let index_output = run_cli(repo.path(), &["index"])?;
    let stdout = String::from_utf8_lossy(&index_output.stdout);
    let stderr = String::from_utf8_lossy(&index_output.stderr);

    assert!(
        index_output.status.success(),
        "Index failed with real embeddings: stdout={stdout}, stderr={stderr}"
    );

    let processor =
        start_and_wait_for_outbox_sync_with_db(&postgres, &qdrant, &db_name, &collection_name)
            .await?;

    assert_min_point_count(&qdrant, &collection_name, 15).await?;

    drop(processor); // Clean up processor

    assert_collection_exists(&qdrant, &collection_name).await?;

    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_verify_expected_entities_are_indexed() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;

    let db_name = create_test_database(&postgres).await?;

    let repo = multi_file_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let _config_path = create_test_config(
        repo.path(),
        &qdrant,
        &postgres,
        &db_name,
        Some(&collection_name),
    )?;

    // Run index - it will automatically initialize storage if needed
    run_cli(repo.path(), &["index"])?;

    let processor =
        start_and_wait_for_outbox_sync_with_db(&postgres, &qdrant, &db_name, &collection_name)
            .await?;

    // Just check that we have a reasonable number of entities - at least 10
    assert_min_point_count(&qdrant, &collection_name, 10).await?;

    drop(processor); // Clean up processor

    let expected = ExpectedEntity::new(
        "Calculator",
        codesearch_core::entities::EntityType::Struct,
        "main.rs",
    );
    assert_entity_in_qdrant(&qdrant, &collection_name, &expected).await?;

    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_index_command_handles_existing_collection() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;

    let db_name = create_test_database(&postgres).await?;

    let repo = simple_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let _config_path = create_test_config(
        repo.path(),
        &qdrant,
        &postgres,
        &db_name,
        Some(&collection_name),
    )?;

    // Run index first time - will automatically initialize storage
    let output1 = run_cli(repo.path(), &["index"])?;
    assert!(output1.status.success(), "First index failed");

    // Run index again - should handle gracefully (storage already initialized)
    let output2 = run_cli(repo.path(), &["index"])?;

    let stdout = String::from_utf8_lossy(&output2.stdout);
    let stderr = String::from_utf8_lossy(&output2.stderr);

    assert!(
        output2.status.success(),
        "Second index failed: stdout={stdout}, stderr={stderr}"
    );

    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_index_auto_initializes_when_collection_missing() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;

    let db_name = create_test_database(&postgres).await?;

    let repo = simple_rust_repo().await?;

    // Create config - collection doesn't exist yet
    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    create_test_config(
        repo.path(),
        &qdrant,
        &postgres,
        &db_name,
        Some(&collection_name),
    )?;

    // Run index - should auto-initialize storage
    let output = run_cli(repo.path(), &["index"])?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "Index should succeed and auto-initialize storage: stdout={stdout}, stderr={stderr}"
    );

    // Verify collection was created
    assert_collection_exists(&qdrant, &collection_name).await?;

    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_index_with_unreachable_qdrant_fails() -> Result<()> {
    init_test_logging();

    let repo = simple_rust_repo().await?;

    // Create config pointing to non-existent Qdrant
    let config_content = r#"
[indexer]

[storage]
qdrant_host = "localhost"
qdrant_port = 19999
qdrant_rest_port = 19998
collection_name = "test_collection"
auto_start_deps = false

[embeddings]
provider = "mock"

[watcher]
debounce_ms = 500
ignore_patterns = ["*.log", "target", ".git"]

[languages]
enabled = ["rust"]
"#;

    let config_path = repo.path().join("codesearch.toml");
    std::fs::write(&config_path, config_content)?;

    let output = run_cli(repo.path(), &["index"])?;

    assert!(
        !output.status.success(),
        "Index should fail with unreachable Qdrant"
    );

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_index_with_invalid_files_continues() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;

    let db_name = create_test_database(&postgres).await?;

    // Create repo with valid and invalid Rust files
    let repo = TestRepositoryBuilder::new()
        .with_rust_file(
            "valid.rs",
            r#"
fn valid_function() -> i32 {
    42
}
"#,
        )
        .with_rust_file(
            "invalid.rs",
            r#"
// This is invalid Rust syntax
fn broken( {
    let x =
}
"#,
        )
        .build()
        .await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let _config_path = create_test_config(
        repo.path(),
        &qdrant,
        &postgres,
        &db_name,
        Some(&collection_name),
    )?;

    // Run index - it will automatically initialize storage if needed
    let output = run_cli(repo.path(), &["index"])?;

    assert!(
        output.status.success(),
        "Index should succeed with partial failures"
    );

    let processor =
        start_and_wait_for_outbox_sync_with_db(&postgres, &qdrant, &db_name, &collection_name)
            .await?;

    let storage_config = StorageConfig {
        qdrant_host: "localhost".to_string(),
        qdrant_port: qdrant.port(),
        qdrant_rest_port: qdrant.rest_port(),
        collection_name: collection_name.clone(),
        auto_start_deps: false,
        docker_compose_file: None,
        postgres_host: "localhost".to_string(),
        postgres_port: postgres.port(),
        postgres_database: "codesearch".to_string(),
        postgres_user: "codesearch".to_string(),
        postgres_password: "codesearch".to_string(),
    };
    let collection_manager = create_collection_manager(&storage_config).await?;
    assert!(
        collection_manager
            .collection_exists(&collection_name)
            .await?,
        "Collection should exist"
    );

    drop(processor); // Clean up processor

    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_empty_repository_indexes_successfully() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;

    let db_name = create_test_database(&postgres).await?;

    let repo = TestRepositoryBuilder::new().build().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let _config_path = create_test_config(
        repo.path(),
        &qdrant,
        &postgres,
        &db_name,
        Some(&collection_name),
    )?;

    // Run index - it will automatically initialize storage if needed
    let index_output = run_cli(repo.path(), &["index"])?;

    assert!(
        index_output.status.success(),
        "Index should succeed on empty repository"
    );

    // Collection should exist but be empty
    assert_collection_exists(&qdrant, &collection_name).await?;
    assert_point_count(&qdrant, &collection_name, 0).await?;

    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_large_entity_is_skipped() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;

    let db_name = create_test_database(&postgres).await?;

    // Create file with very large entity (exceeds context window)
    let large_content = format!(
        r#"
fn large_function() {{
    {}
}}
"#,
        "    println!(\"line\");\n".repeat(10000) // Very large function
    );

    let repo = TestRepositoryBuilder::new()
        .with_rust_file("large.rs", &large_content)
        .build()
        .await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let _config_path = create_test_config(
        repo.path(),
        &qdrant,
        &postgres,
        &db_name,
        Some(&collection_name),
    )?;

    // Run index - it will automatically initialize storage if needed
    let output = run_cli(repo.path(), &["index"])?;

    assert!(
        output.status.success(),
        "Index should succeed even with oversized entities"
    );

    let processor =
        start_and_wait_for_outbox_sync_with_db(&postgres, &qdrant, &db_name, &collection_name)
            .await?;

    // Large entity should be skipped
    // (Collection may be empty or have other entities if any were extracted)
    assert_collection_exists(&qdrant, &collection_name).await?;

    drop(processor); // Clean up processor

    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_duplicate_entity_ids_handled() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;

    let db_name = create_test_database(&postgres).await?;

    // Create repo with two files that have identically named functions
    let repo = TestRepositoryBuilder::new()
        .with_rust_file(
            "file1.rs",
            r#"
pub fn duplicate_name() -> i32 {
    1
}
"#,
        )
        .with_rust_file(
            "file2.rs",
            r#"
pub fn duplicate_name() -> i32 {
    2
}
"#,
        )
        .build()
        .await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let _config_path = create_test_config(
        repo.path(),
        &qdrant,
        &postgres,
        &db_name,
        Some(&collection_name),
    )?;

    // Run index - it will automatically initialize storage if needed
    let output = run_cli(repo.path(), &["index"])?;

    assert!(
        output.status.success(),
        "Index should handle duplicate entity names"
    );

    let processor =
        start_and_wait_for_outbox_sync_with_db(&postgres, &qdrant, &db_name, &collection_name)
            .await?;

    assert_min_point_count(&qdrant, &collection_name, 2).await?;

    drop(processor); // Clean up processor

    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

// =============================================================================
// Concurrent Execution Test
// =============================================================================
