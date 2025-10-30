//! Smoke tests for graph query functions
//!
//! These tests verify that graph query functions can be called and return results
//! without panicking. They require Neo4j and Postgres to be running.

use anyhow::Result;
use codesearch_core::config::StorageConfig;
use codesearch_server::graph_queries::*;
use codesearch_storage::{create_neo4j_client, PostgresClient, PostgresClientTrait};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;

#[allow(dead_code)]
fn create_test_config() -> StorageConfig {
    StorageConfig {
        qdrant_host: "localhost".to_string(),
        qdrant_port: 6334,
        qdrant_rest_port: 6333,
        auto_start_deps: false,
        docker_compose_file: None,
        postgres_host: "localhost".to_string(),
        postgres_port: 5432,
        postgres_database: "postgres".to_string(),
        postgres_user: "postgres".to_string(),
        postgres_password: "postgres".to_string(),
        neo4j_host: "localhost".to_string(),
        neo4j_http_port: 7474,
        neo4j_bolt_port: 7687,
        neo4j_user: "neo4j".to_string(),
        neo4j_password: "codesearch".to_string(),
        max_entities_per_db_operation: 1000,
    }
}

#[tokio::test]
#[ignore] // Requires Neo4j and Postgres to be running
async fn test_find_functions_in_module_smoke() -> Result<()> {
    let config = create_test_config();
    let neo4j = create_neo4j_client(&config).await?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&format!(
            "postgres://{}:{}@{}:{}/{}",
            config.postgres_user,
            config.postgres_password,
            config.postgres_host,
            config.postgres_port,
            config.postgres_database
        ))
        .await?;

    let postgres: Arc<dyn PostgresClientTrait> = Arc::new(PostgresClient::new(pool, 1000));
    postgres.run_migrations().await?;

    // Create test repository
    let repo_path = std::path::Path::new("/test/smoke");
    let repo_id = postgres
        .ensure_repository(repo_path, "test_collection", None)
        .await?;

    // Call function - should not panic even if no results
    let result = find_functions_in_module(&neo4j, &postgres, repo_id, "test::module").await;
    assert!(result.is_ok(), "Function should complete without error");

    Ok(())
}

#[tokio::test]
#[ignore] // Requires Neo4j and Postgres to be running
async fn test_find_trait_implementations_smoke() -> Result<()> {
    let config = create_test_config();
    let neo4j = create_neo4j_client(&config).await?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&format!(
            "postgres://{}:{}@{}:{}/{}",
            config.postgres_user,
            config.postgres_password,
            config.postgres_host,
            config.postgres_port,
            config.postgres_database
        ))
        .await?;

    let postgres: Arc<dyn PostgresClientTrait> = Arc::new(PostgresClient::new(pool, 1000));
    postgres.run_migrations().await?;

    let repo_path = std::path::Path::new("/test/smoke");
    let repo_id = postgres
        .ensure_repository(repo_path, "test_collection", None)
        .await?;

    // Call function - should not panic even if no results
    let result = find_trait_implementations(&neo4j, &postgres, repo_id, "TestTrait").await;
    assert!(result.is_ok(), "Function should complete without error");

    Ok(())
}

#[tokio::test]
#[ignore] // Requires Neo4j and Postgres to be running
async fn test_find_class_hierarchy_smoke() -> Result<()> {
    let config = create_test_config();
    let neo4j = create_neo4j_client(&config).await?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&format!(
            "postgres://{}:{}@{}:{}/{}",
            config.postgres_user,
            config.postgres_password,
            config.postgres_host,
            config.postgres_port,
            config.postgres_database
        ))
        .await?;

    let postgres: Arc<dyn PostgresClientTrait> = Arc::new(PostgresClient::new(pool, 1000));
    postgres.run_migrations().await?;

    let repo_path = std::path::Path::new("/test/smoke");
    let repo_id = postgres
        .ensure_repository(repo_path, "test_collection", None)
        .await?;

    // Call function - should not panic even if no results
    let result = find_class_hierarchy(&neo4j, &postgres, repo_id, "TestClass").await;
    assert!(result.is_ok(), "Function should complete without error");

    Ok(())
}

#[tokio::test]
#[ignore] // Requires Neo4j and Postgres to be running
async fn test_find_function_callers_smoke() -> Result<()> {
    let config = create_test_config();
    let neo4j = create_neo4j_client(&config).await?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&format!(
            "postgres://{}:{}@{}:{}/{}",
            config.postgres_user,
            config.postgres_password,
            config.postgres_host,
            config.postgres_port,
            config.postgres_database
        ))
        .await?;

    let postgres: Arc<dyn PostgresClientTrait> = Arc::new(PostgresClient::new(pool, 1000));
    postgres.run_migrations().await?;

    let repo_path = std::path::Path::new("/test/smoke");
    let repo_id = postgres
        .ensure_repository(repo_path, "test_collection", None)
        .await?;

    // Call function - should not panic even if no results
    let result = find_function_callers(&neo4j, &postgres, repo_id, "test::function", 5).await;
    assert!(result.is_ok(), "Function should complete without error");

    Ok(())
}

#[tokio::test]
#[ignore] // Requires Neo4j and Postgres to be running
async fn test_find_unused_functions_smoke() -> Result<()> {
    let config = create_test_config();
    let neo4j = create_neo4j_client(&config).await?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&format!(
            "postgres://{}:{}@{}:{}/{}",
            config.postgres_user,
            config.postgres_password,
            config.postgres_host,
            config.postgres_port,
            config.postgres_database
        ))
        .await?;

    let postgres: Arc<dyn PostgresClientTrait> = Arc::new(PostgresClient::new(pool, 1000));
    postgres.run_migrations().await?;

    let repo_path = std::path::Path::new("/test/smoke");
    let repo_id = postgres
        .ensure_repository(repo_path, "test_collection", None)
        .await?;

    // Call function - should not panic even if no results
    let result = find_unused_functions(&neo4j, &postgres, repo_id).await;
    assert!(result.is_ok(), "Function should complete without error");

    Ok(())
}

#[tokio::test]
#[ignore] // Requires Neo4j and Postgres to be running
async fn test_find_module_dependencies_smoke() -> Result<()> {
    let config = create_test_config();
    let neo4j = create_neo4j_client(&config).await?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&format!(
            "postgres://{}:{}@{}:{}/{}",
            config.postgres_user,
            config.postgres_password,
            config.postgres_host,
            config.postgres_port,
            config.postgres_database
        ))
        .await?;

    let postgres: Arc<dyn PostgresClientTrait> = Arc::new(PostgresClient::new(pool, 1000));
    postgres.run_migrations().await?;

    let repo_path = std::path::Path::new("/test/smoke");
    let repo_id = postgres
        .ensure_repository(repo_path, "test_collection", None)
        .await?;

    // Call function - should not panic even if no results
    let result = find_module_dependencies(&neo4j, &postgres, repo_id, "test::module").await;
    assert!(result.is_ok(), "Function should complete without error");

    Ok(())
}

#[tokio::test]
#[ignore] // Requires Neo4j and Postgres to be running
async fn test_find_circular_dependencies_smoke() -> Result<()> {
    let config = create_test_config();
    let neo4j = create_neo4j_client(&config).await?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&format!(
            "postgres://{}:{}@{}:{}/{}",
            config.postgres_user,
            config.postgres_password,
            config.postgres_host,
            config.postgres_port,
            config.postgres_database
        ))
        .await?;

    let postgres: Arc<dyn PostgresClientTrait> = Arc::new(PostgresClient::new(pool, 1000));
    postgres.run_migrations().await?;

    let repo_path = std::path::Path::new("/test/smoke");
    let repo_id = postgres
        .ensure_repository(repo_path, "test_collection", None)
        .await?;

    // Call function - should not panic even if no results
    let result = find_circular_dependencies(&neo4j, &postgres, repo_id).await;
    assert!(result.is_ok(), "Function should complete without error");

    Ok(())
}
