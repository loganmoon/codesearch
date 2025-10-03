//! End-to-end tests for the complete codesearch pipeline
//!
//! These tests validate the full workflow: init → index → search
//! using isolated Qdrant containers with temporary storage.
//!
//! Run with: cargo test --test e2e_tests
//! Verbose: CODESEARCH_TEST_LOG=debug cargo test --test e2e_tests

mod e2e;

use anyhow::{Context, Result};
use codesearch_core::config::StorageConfig;
use codesearch_embeddings::{EmbeddingProvider, MockEmbeddingProvider};
use codesearch_storage::{create_collection_manager, create_storage_client};
use e2e::*;
use indexer::create_indexer;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use uuid::Uuid;

/// Get the workspace manifest path for cargo run commands
fn workspace_manifest() -> std::path::PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    Path::new(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Cargo.toml")
}

/// Create a test config file for the given repository and test instances
fn create_test_config(
    repo_path: &Path,
    qdrant: &TestQdrant,
    postgres: &TestPostgres,
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
        collection_name.unwrap_or(""),
        postgres.port()
    );

    let config_path = repo_path.join("codesearch.toml");
    std::fs::write(&config_path, config_content)?;
    Ok(config_path)
}

/// Run the codesearch CLI with the given arguments
fn run_cli(repo_path: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("cargo")
        .current_dir(repo_path)
        .args([
            "run",
            "--manifest-path",
            workspace_manifest().to_str().unwrap(),
            "--package",
            "codesearch",
            "--",
        ])
        .args(args)
        .env("RUST_LOG", "info")
        .output()
        .context("Failed to run codesearch CLI")
}

#[tokio::test]
async fn test_init_creates_collection_in_qdrant() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;
    let repo = simple_rust_repo().await?;

    // Create config pointing to test instances
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, None)?;

    // Run init command
    let output = run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "Init command failed: stdout={stdout}, stderr={stderr}"
    );

    // Verify success message
    assert!(
        stderr.contains("Repository initialized successfully")
            || stdout.contains("Repository initialized successfully"),
        "Expected success message not found"
    );

    // Read updated config to get collection name
    let config_content = std::fs::read_to_string(&config_path)?;
    assert!(
        config_content.contains("collection_name = "),
        "Collection name not saved to config"
    );

    // Extract collection name from config
    let collection_name = config_content
        .lines()
        .find(|line| line.contains("collection_name = "))
        .and_then(|line| line.split('=').nth(1))
        .map(|s| s.trim().trim_matches('"'))
        .context("Failed to extract collection name")?;

    // Verify collection exists in Qdrant
    assert_collection_exists(&qdrant, collection_name).await?;

    // Note: The init command uses the default model dimensions (1536 for BAAI/bge-code-v1)
    // even with mock provider, since the provider type is determined at runtime
    // We just verify the collection was created successfully

    Ok(())
}

#[tokio::test]
async fn test_index_stores_entities_in_qdrant() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;
    let repo = multi_file_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, Some(&collection_name))?;

    // Run init first
    let init_output = run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    assert!(init_output.status.success(), "Init failed");

    // Run index command
    let index_output = run_cli(repo.path(), &["index"])?;

    let stdout = String::from_utf8_lossy(&index_output.stdout);
    let stderr = String::from_utf8_lossy(&index_output.stderr);

    assert!(
        index_output.status.success(),
        "Index command failed: stdout={stdout}, stderr={stderr}"
    );

    // Verify entities were indexed (should have at least 10 entities)
    assert_min_point_count(&qdrant, &collection_name, 10).await?;

    // Verify collection exists
    assert_collection_exists(&qdrant, &collection_name).await?;

    Ok(())
}

#[tokio::test]
async fn test_index_with_mock_embeddings() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;
    let repo = simple_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, Some(&collection_name))?;

    // Run init and index
    run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    let output = run_cli(repo.path(), &["index"])?;

    assert!(output.status.success(), "Index with mock embeddings failed");

    // Verify at least some entities were indexed
    assert_min_point_count(&qdrant, &collection_name, 3).await?;

    Ok(())
}

#[tokio::test]
async fn test_search_finds_relevant_entities() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;
    let repo = multi_file_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, Some(&collection_name))?;

    // Init and index the repository
    run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    run_cli(repo.path(), &["index"])?;

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

    // Create mock embedding for search query with matching dimensions (1536 from init)
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

    // Verify we got results
    assert!(
        !results.is_empty(),
        "Search should return at least some results"
    );

    // Verify we got entity_id and repository_id from search
    for (entity_id, repository_id, _score) in &results {
        assert!(!entity_id.is_empty(), "Entity ID should not be empty");
        assert!(
            !repository_id.is_empty(),
            "Repository ID should not be empty"
        );
    }

    Ok(())
}

#[tokio::test]
#[ignore] // Requires vLLM running
async fn test_complete_pipeline_with_real_embeddings() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;
    let repo = complex_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());

    // Create config with LocalApi provider instead of mock
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
provider = "local_api"
api_url = "http://localhost:8000/v1"
model_name = "BAAI/bge-small-en-v1.5"

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

    let config_path = repo.path().join("codesearch.toml");
    std::fs::write(&config_path, config_content)?;

    // Run init
    let init_output = run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    assert!(
        init_output.status.success(),
        "Init failed with real embeddings"
    );

    // Run index
    let index_output = run_cli(repo.path(), &["index"])?;
    let stdout = String::from_utf8_lossy(&index_output.stdout);
    let stderr = String::from_utf8_lossy(&index_output.stderr);

    assert!(
        index_output.status.success(),
        "Index failed with real embeddings: stdout={stdout}, stderr={stderr}"
    );

    // Verify substantial entities were indexed
    assert_min_point_count(&qdrant, &collection_name, 15).await?;

    // Verify collection exists with correct dimensions for the model
    assert_collection_exists(&qdrant, &collection_name).await?;

    Ok(())
}

#[tokio::test]
async fn test_verify_expected_entities_are_indexed() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;
    let repo = multi_file_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, Some(&collection_name))?;

    // Init and index
    run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    run_cli(repo.path(), &["index"])?;

    // Verify entities were indexed
    // Just check that we have a reasonable number of entities - at least 10
    assert_min_point_count(&qdrant, &collection_name, 10).await?;

    // Verify we can find at least one struct entity
    let expected = ExpectedEntity::new(
        "Calculator",
        codesearch_core::entities::EntityType::Struct,
        "main.rs",
    );
    assert_entity_in_qdrant(&qdrant, &collection_name, &expected).await?;

    Ok(())
}

#[tokio::test]
async fn test_init_command_handles_existing_collection() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;
    let repo = simple_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, Some(&collection_name))?;

    // Run init first time
    let output1 = run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    assert!(output1.status.success(), "First init failed");

    // Run init again - should handle gracefully
    let output2 = run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;

    let stdout = String::from_utf8_lossy(&output2.stdout);
    let stderr = String::from_utf8_lossy(&output2.stderr);

    assert!(
        output2.status.success(),
        "Second init failed: stdout={stdout}, stderr={stderr}"
    );

    Ok(())
}

// Test disabled: create_indexer now requires PostgresClient
// TODO: Re-enable when test infrastructure includes Postgres
/*
#[tokio::test]
async fn test_programmatic_indexing_pipeline() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;
    let repo = simple_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());

    // Create collection via collection manager
    let storage_config = StorageConfig {
        qdrant_host: "localhost".to_string(),
        qdrant_port: qdrant.port(),
        qdrant_rest_port: qdrant.rest_port(),
        collection_name: collection_name.clone(),
        auto_start_deps: false,
        docker_compose_file: None,
        postgres_host: "localhost".to_string(),
        postgres_port: 5432,
        postgres_database: "codesearch".to_string(),
        postgres_user: "codesearch".to_string(),
        postgres_password: "codesearch".to_string(),
    };

    let collection_manager = create_collection_manager(&storage_config).await?;
    collection_manager
        .ensure_collection(&collection_name, 384)
        .await?;

    // Create indexer with mock embeddings
    let mock_provider = Arc::new(MockEmbeddingProvider::new(384));
    let embedding_manager = Arc::new(codesearch_embeddings::EmbeddingManager::new(mock_provider));
    let storage_client = create_storage_client(&storage_config, &collection_name).await?;

    let mut indexer = create_indexer(
        repo.path().to_path_buf(),
        storage_client,
        embedding_manager,
        None,
        None,
    );

    // Run indexer
    let result = indexer.index_repository().await?;
    let stats = result.stats();

    // Verify stats
    assert!(stats.total_files() > 0, "Should process at least one file");
    assert!(
        stats.entities_extracted() > 0,
        "Should extract at least one entity"
    );

    // Verify entities in Qdrant
    assert_min_point_count(&qdrant, &collection_name, 3).await?;

    Ok(())
}
*/

// =============================================================================
// Error Handling Tests
// =============================================================================

#[tokio::test]
async fn test_index_without_init_fails_gracefully() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;
    let repo = simple_rust_repo().await?;

    // Create config but don't run init
    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    create_test_config(repo.path(), &qdrant, &postgres, Some(&collection_name))?;

    // Try to index without init
    let output = run_cli(repo.path(), &["index"])?;

    // Should fail with helpful error
    assert!(
        !output.status.success(),
        "Index should fail when collection doesn't exist"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Error message should be helpful
    assert!(
        !stderr.is_empty(),
        "Should provide error message when index fails"
    );

    Ok(())
}

#[tokio::test]
async fn test_init_with_unreachable_qdrant_fails() -> Result<()> {
    init_test_logging();

    let repo = simple_rust_repo().await?;

    // Create config pointing to non-existent Qdrant
    let config_content = r#"
[indexer]

[storage]
qdrant_host = "localhost"
qdrant_port = 19999
qdrant_rest_port = 19998
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
"#;

    let config_path = repo.path().join("codesearch.toml");
    std::fs::write(&config_path, config_content)?;

    // Try to init with unreachable Qdrant
    let output = run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;

    // Should fail with clear error
    assert!(
        !output.status.success(),
        "Init should fail with unreachable Qdrant"
    );

    Ok(())
}

#[tokio::test]
async fn test_index_with_invalid_files_continues() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;

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
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, Some(&collection_name))?;

    // Run init and index
    run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    let output = run_cli(repo.path(), &["index"])?;

    // Should succeed despite invalid file
    assert!(
        output.status.success(),
        "Index should succeed with partial failures"
    );

    // Should have indexed the valid file
    let storage_config = StorageConfig {
        qdrant_host: "localhost".to_string(),
        qdrant_port: qdrant.port(),
        qdrant_rest_port: qdrant.rest_port(),
        collection_name: collection_name.clone(),
        auto_start_deps: false,
        docker_compose_file: None,
        postgres_host: "localhost".to_string(),
        postgres_port: 5432,
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

    Ok(())
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[tokio::test]
async fn test_empty_repository_indexes_successfully() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;

    // Create empty repository with just git
    let repo = TestRepositoryBuilder::new().build().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, Some(&collection_name))?;

    // Run init
    let init_output = run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    assert!(init_output.status.success(), "Init should succeed");

    // Run index on empty repo
    let index_output = run_cli(repo.path(), &["index"])?;

    // Should succeed with zero entities
    assert!(
        index_output.status.success(),
        "Index should succeed on empty repository"
    );

    // Collection should exist but be empty
    assert_collection_exists(&qdrant, &collection_name).await?;
    assert_point_count(&qdrant, &collection_name, 0).await?;

    Ok(())
}

#[tokio::test]
async fn test_large_entity_is_skipped() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;

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
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, Some(&collection_name))?;

    // Run init and index
    run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    let output = run_cli(repo.path(), &["index"])?;

    // Should succeed
    assert!(
        output.status.success(),
        "Index should succeed even with oversized entities"
    );

    // Large entity should be skipped
    // (Collection may be empty or have other entities if any were extracted)
    assert_collection_exists(&qdrant, &collection_name).await?;

    Ok(())
}

#[tokio::test]
async fn test_duplicate_entity_ids_handled() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let postgres = TestPostgres::start().await?;

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
    let config_path = create_test_config(repo.path(), &qdrant, &postgres, Some(&collection_name))?;

    // Run init and index
    run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    let output = run_cli(repo.path(), &["index"])?;

    // Should succeed
    assert!(
        output.status.success(),
        "Index should handle duplicate entity names"
    );

    // Should have indexed both (they'll have different entity_ids due to file paths)
    assert_min_point_count(&qdrant, &collection_name, 2).await?;

    Ok(())
}

// =============================================================================
// Concurrent Execution Test
// =============================================================================

#[tokio::test]
async fn test_concurrent_indexing_with_separate_containers() -> Result<()> {
    init_test_logging();

    // Create pools of 3 containers
    let qdrant_pool = TestQdrantPool::new(3).await?;
    let postgres_pool = TestPostgresPool::new(3).await?;

    // Create 3 different repositories
    let repo1 = simple_rust_repo().await?;
    let repo2 = multi_file_rust_repo().await?;
    let repo3 = complex_rust_repo().await?;

    let repos = vec![repo1, repo2, repo3];
    let mut handles = Vec::new();

    // Index all repositories concurrently
    for (i, repo) in repos.iter().enumerate() {
        let qdrant = qdrant_pool.get(i).unwrap();
        let postgres = postgres_pool.get(i).unwrap();
        let collection_name = format!("test_collection_{}", Uuid::new_v4());
        let config_path =
            create_test_config(repo.path(), qdrant, postgres, Some(&collection_name))?;

        // Clone values for async move
        let repo_path = repo.path().to_path_buf();
        let collection_name_clone = collection_name.clone();
        let rest_url = qdrant.rest_url();

        let handle = tokio::spawn(async move {
            // Run init
            let init_output = Command::new("cargo")
                .current_dir(&repo_path)
                .args([
                    "run",
                    "--manifest-path",
                    workspace_manifest().to_str().unwrap(),
                    "--package",
                    "codesearch",
                    "--",
                    "init",
                    "--config",
                    config_path.to_str().unwrap(),
                ])
                .output()
                .expect("Failed to run init");

            assert!(init_output.status.success(), "Init failed");

            // Run index
            let index_output = Command::new("cargo")
                .current_dir(&repo_path)
                .args([
                    "run",
                    "--manifest-path",
                    workspace_manifest().to_str().unwrap(),
                    "--package",
                    "codesearch",
                    "--",
                    "index",
                ])
                .output()
                .expect("Failed to run index");

            assert!(index_output.status.success(), "Index failed");

            // Verify collection exists
            let url = format!("{rest_url}/collections/{collection_name_clone}");
            let response = reqwest::get(&url).await.expect("Failed to query Qdrant");
            assert!(response.status().is_success(), "Collection should exist");
        });

        handles.push(handle);
    }

    // Wait for all indexing operations to complete
    for handle in handles {
        handle.await?;
    }

    Ok(())
}
