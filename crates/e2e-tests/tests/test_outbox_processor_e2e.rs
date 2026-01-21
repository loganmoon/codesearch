//! End-to-end tests for outbox processor with real Qdrant interaction
//!
//! These tests validate the outbox processor's behavior with actual Qdrant instances,
//! including failure scenarios, DELETE operations, and mixed batch processing.

use anyhow::Result;
use codesearch_core::entities::{
    EntityMetadata, EntityRelationshipData, EntityType, Language, SourceLocation, Visibility,
};
use codesearch_core::{CodeEntity, QualifiedName};
use codesearch_e2e_tests::common::*;
use codesearch_outbox_processor::OutboxProcessor;
use codesearch_storage::{
    create_collection_manager, OutboxOperation, PostgresClientTrait, QdrantConfig, TargetStore,
};
use sqlx::postgres::PgPoolOptions;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

fn create_test_entity(name: &str, entity_id: &str, file_path: &str, repo_id: &str) -> CodeEntity {
    CodeEntity {
        entity_id: entity_id.to_string(),
        repository_id: repo_id.to_string(),
        name: name.to_string(),
        qualified_name: QualifiedName::parse(name).expect("Invalid qualified name in test"),
        path_entity_identifier: None,
        entity_type: EntityType::Function,
        language: Language::Rust,
        file_path: PathBuf::from(file_path),
        location: SourceLocation {
            start_line: 1,
            end_line: 10,
            start_column: 0,
            end_column: 10,
        },
        visibility: Some(Visibility::Public),
        parent_scope: None,
        signature: None,
        documentation_summary: None,
        content: Some(format!("fn {name}() {{}}")),
        metadata: EntityMetadata::default(),
        relationships: EntityRelationshipData::default(),
    }
}

/// Create storage and qdrant configs for testing
fn create_test_configs(
    qdrant: &TestQdrant,
    postgres: &TestPostgres,
    neo4j: &TestNeo4j,
    db_name: &str,
) -> (QdrantConfig, codesearch_core::config::StorageConfig) {
    let qdrant_config = QdrantConfig {
        host: "localhost".to_string(),
        port: qdrant.port(),
        rest_port: qdrant.rest_port(),
    };

    let storage_config = codesearch_core::config::StorageConfig {
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
        neo4j_host: "localhost".to_string(),
        neo4j_http_port: neo4j.http_port(),
        neo4j_bolt_port: neo4j.bolt_port(),
        neo4j_user: "".to_string(), // Neo4j test container has auth disabled
        neo4j_password: "".to_string(),
        max_entities_per_db_operation: 10000,
        postgres_pool_size: 20,
    };

    (qdrant_config, storage_config)
}

/// Helper to create Qdrant collection using the collection manager
async fn create_qdrant_collection(
    qdrant: &TestQdrant,
    postgres: &TestPostgres,
    neo4j: &TestNeo4j,
    db_name: &str,
    collection_name: &str,
    vector_size: usize,
) -> Result<()> {
    let (_, storage_config) = create_test_configs(qdrant, postgres, neo4j, db_name);
    let collection_manager = create_collection_manager(&storage_config).await?;
    collection_manager
        .ensure_collection(collection_name, vector_size)
        .await?;
    Ok(())
}

/// Run outbox processor in-process until the outbox is empty
///
/// This runs the processor directly (not in Docker) and processes batches
/// until no pending entries remain.
async fn run_outbox_processor_until_empty(
    postgres_client: Arc<dyn PostgresClientTrait>,
    qdrant: &TestQdrant,
    postgres: &TestPostgres,
    neo4j: &TestNeo4j,
    db_name: &str,
    timeout: Duration,
) -> Result<()> {
    let (qdrant_config, storage_config) = create_test_configs(qdrant, postgres, neo4j, db_name);

    let processor = OutboxProcessor::new(
        Arc::clone(&postgres_client),
        qdrant_config,
        storage_config,
        Duration::from_millis(50), // Fast poll for tests
        100,                       // batch_size
        3,                         // max_retries
        OutboxProcessor::DEFAULT_MAX_EMBEDDING_DIM,
        200, // max_cached_collections
    );

    let start = std::time::Instant::now();

    loop {
        // Check timeout
        if start.elapsed() > timeout {
            let pending = postgres_client.count_pending_outbox_entries().await?;
            anyhow::bail!(
                "Timeout waiting for outbox to empty. {} entries still pending",
                pending
            );
        }

        // Process a batch
        let had_work = processor.process_batch().await?;

        // Check if outbox is empty
        let pending = postgres_client.count_pending_outbox_entries().await?;
        if pending == 0 {
            return Ok(());
        }

        // If no work was done, wait a bit before next poll
        if !had_work {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_e2e_delete_operations_sync_to_qdrant() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;
    let neo4j = get_shared_neo4j().await?;
    let db_name = create_test_database(&postgres).await?;
    let collection_name = format!("test_delete_{}", Uuid::new_v4());

    // Setup: Connect to database
    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/{db_name}",
        postgres.port()
    );
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_url)
        .await?;

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create repository
    let repo_path = std::path::Path::new("/test/repo");
    let repo_id = postgres_client
        .ensure_repository(repo_path, &collection_name, None)
        .await?;

    // Initialize storage (create collection)
    create_qdrant_collection(&qdrant, &postgres, &neo4j, &db_name, &collection_name, 384).await?;

    // Step 1: INSERT 3 entities to Qdrant
    let entities: Vec<CodeEntity> = (0..3)
        .map(|i| {
            create_test_entity(
                &format!("func_{i}"),
                &format!("entity-{i}"),
                "/test/file.rs",
                &repo_id.to_string(),
            )
        })
        .collect();

    // Store entities with outbox
    for entity in &entities {
        let embedding = vec![0.1; 384];
        // Store embedding to get its ID (sparse embedding is optional, test with None)
        let content_hash = format!("{:032x}", Uuid::new_v4().as_u128());
        let embedding_ids = postgres_client
            .store_embeddings(
                repo_id,
                &[(content_hash, embedding, None)], // No sparse embedding
                "test-model",
                384,
            )
            .await?;
        let embedding_id = embedding_ids[0];

        let batch = vec![(
            entity,
            embedding_id,
            OutboxOperation::Insert,
            Uuid::new_v4(),
            TargetStore::Qdrant,
            None, // git_commit
            50,   // token_count
        )];
        postgres_client
            .store_entities_with_outbox_batch(repo_id, &collection_name, &batch)
            .await?;
    }

    // Run outbox processor to sync INSERTs
    run_outbox_processor_until_empty(
        Arc::clone(&postgres_client),
        &qdrant,
        &postgres,
        &neo4j,
        &db_name,
        Duration::from_secs(30),
    )
    .await?;

    // Verify 3 points exist in Qdrant
    assert_min_point_count(&qdrant, &collection_name, 3).await?;

    // Step 2: DELETE 2 entities via outbox
    let delete_ids = vec!["entity-0".to_string(), "entity-1".to_string()];

    // Create DELETE outbox entries
    sqlx::query(
        "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
         payload, collection_name)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(repo_id)
    .bind("entity-0")
    .bind("DELETE")
    .bind("qdrant")
    .bind(serde_json::json!({
        "entity_ids": delete_ids
    }))
    .bind(&collection_name)
    .execute(&pool)
    .await?;

    // Run processor again to sync DELETEs
    run_outbox_processor_until_empty(
        Arc::clone(&postgres_client),
        &qdrant,
        &postgres,
        &neo4j,
        &db_name,
        Duration::from_secs(30),
    )
    .await?;

    // Verify only 1 point remains in Qdrant
    assert_point_count(&qdrant, &collection_name, 1).await?;

    // Cleanup
    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_e2e_mixed_operations_in_single_batch() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;
    let neo4j = get_shared_neo4j().await?;
    let db_name = create_test_database(&postgres).await?;
    let collection_name = format!("test_mixed_{}", Uuid::new_v4());

    // Setup: Connect to database
    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/{db_name}",
        postgres.port()
    );
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_url)
        .await?;

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create repository
    let repo_path = std::path::Path::new("/test/repo");
    let repo_id = postgres_client
        .ensure_repository(repo_path, &collection_name, None)
        .await?;

    // Initialize storage
    create_qdrant_collection(&qdrant, &postgres, &neo4j, &db_name, &collection_name, 384).await?;

    // Create 5 entities for testing
    for i in 0..5 {
        let entity = create_test_entity(
            &format!("func_{i}"),
            &format!("entity-{i}"),
            "/test/file.rs",
            &repo_id.to_string(),
        );
        let embedding = vec![0.1; 384];
        // Store embedding to get its ID (sparse embedding is optional, test with None)
        let content_hash = format!("{:032x}", Uuid::new_v4().as_u128());
        let embedding_ids = postgres_client
            .store_embeddings(
                repo_id,
                &[(content_hash, embedding, None)], // No sparse embedding
                "test-model",
                384,
            )
            .await?;
        let embedding_id = embedding_ids[0];

        let batch = vec![(
            &entity,
            embedding_id,
            OutboxOperation::Insert,
            Uuid::new_v4(),
            TargetStore::Qdrant,
            None, // git_commit
            50,   // token_count
        )];
        postgres_client
            .store_entities_with_outbox_batch(repo_id, &collection_name, &batch)
            .await?;
    }

    // Process INSERTs
    run_outbox_processor_until_empty(
        Arc::clone(&postgres_client),
        &qdrant,
        &postgres,
        &neo4j,
        &db_name,
        Duration::from_secs(30),
    )
    .await?;

    assert_min_point_count(&qdrant, &collection_name, 5).await?;

    // Now create mixed batch: 2 UPDATEs + 1 DELETE
    // UPDATE entity-0 and entity-1
    for i in 0..2 {
        let entity_id = format!("entity-{i}");
        sqlx::query(
            "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
             payload, collection_name)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind("UPDATE")
        .bind("qdrant")
        .bind(serde_json::json!({
            "entity_id": entity_id,
            "embedding": vec![0.5; 384],
            "qdrant_point_id": Uuid::new_v4().to_string()
        }))
        .bind(&collection_name)
        .execute(&pool)
        .await?;
    }

    // DELETE entity-4
    sqlx::query(
        "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
         payload, collection_name)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(repo_id)
    .bind("entity-4")
    .bind("DELETE")
    .bind("qdrant")
    .bind(serde_json::json!({
        "entity_ids": ["entity-4"]
    }))
    .bind(&collection_name)
    .execute(&pool)
    .await?;

    // Process mixed batch
    run_outbox_processor_until_empty(
        Arc::clone(&postgres_client),
        &qdrant,
        &postgres,
        &neo4j,
        &db_name,
        Duration::from_secs(30),
    )
    .await?;

    // Should have 4 points now (5 - 1 deleted)
    assert_point_count(&qdrant, &collection_name, 4).await?;

    // Cleanup
    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_e2e_invalid_delete_payload_recorded_as_failure() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;
    let neo4j = get_shared_neo4j().await?;
    let db_name = create_test_database(&postgres).await?;
    let collection_name = format!("test_invalid_delete_{}", Uuid::new_v4());

    // Setup: Connect to database
    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/{db_name}",
        postgres.port()
    );
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_url)
        .await?;

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create repository
    let repo_path = std::path::Path::new("/test/repo");
    let repo_id = postgres_client
        .ensure_repository(repo_path, &collection_name, None)
        .await?;

    // Initialize storage
    create_qdrant_collection(&qdrant, &postgres, &neo4j, &db_name, &collection_name, 384).await?;

    // Create entity metadata
    sqlx::query(
        "INSERT INTO entity_metadata (repository_id, entity_id, qualified_name, name,
         entity_type, language, file_path, visibility, entity_data, git_commit_hash, qdrant_point_id, content)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(repo_id)
    .bind("invalid-entity")
    .bind("qualified::invalid-entity")
    .bind("invalid-entity")
    .bind("function")
    .bind("rust")
    .bind("/test/file.rs")
    .bind("public")
    .bind(serde_json::json!({}))
    .bind("abc123")
    .bind(Uuid::new_v4())
    .bind(None::<String>)
    .execute(&pool)
    .await?;

    // Create DELETE with invalid payload (entity_ids is not an array)
    sqlx::query(
        "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
         payload, collection_name)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(repo_id)
    .bind("invalid-entity")
    .bind("DELETE")
    .bind("qdrant")
    .bind(serde_json::json!({
        "entity_ids": "not-an-array" // Invalid!
    }))
    .bind(&collection_name)
    .execute(&pool)
    .await?;

    // Create and run processor for enough batches to exhaust retries (max_retries = 3)
    let (qdrant_config, storage_config) = create_test_configs(&qdrant, &postgres, &neo4j, &db_name);
    let processor = OutboxProcessor::new(
        Arc::clone(&postgres_client),
        qdrant_config,
        storage_config,
        Duration::from_millis(50),
        10,
        3, // max_retries
        OutboxProcessor::DEFAULT_MAX_EMBEDDING_DIM,
        200,
    );

    // Run batches until entry is processed (either successfully or after max retries)
    for _ in 0..10 {
        let _ = processor.process_batch().await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Check if entry has been processed
        let unprocessed_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM entity_outbox WHERE processed_at IS NULL")
                .fetch_one(&pool)
                .await?;
        if unprocessed_count == 0 {
            break;
        }
    }

    // Check that entry was eventually marked as processed (after max retries)
    let processed_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM entity_outbox WHERE processed_at IS NOT NULL")
            .fetch_one(&pool)
            .await?;

    // Entry should be marked processed after exhausting retries (to avoid poison pills)
    assert_eq!(
        processed_count, 1,
        "Entry with invalid DELETE payload should be marked processed after max retries"
    );

    // Check that retry_count reached max and error was recorded
    let entry: Option<(i32, Option<String>)> = sqlx::query_as(
        "SELECT retry_count, last_error FROM entity_outbox WHERE entity_id = 'invalid-entity'",
    )
    .fetch_optional(&pool)
    .await?;

    let (retry_count, last_error) = entry.expect("Entry should exist");
    assert!(
        retry_count >= 3,
        "Entry should have been retried at least 3 times, got {retry_count}"
    );
    assert!(
        last_error.is_some(),
        "Entry should have error message recorded"
    );
    let error_msg = last_error.unwrap();
    assert!(
        error_msg.contains("Invalid DELETE payload"),
        "Error should mention invalid DELETE payload, got: {error_msg}"
    );

    // Cleanup
    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

/// Test that sparse embeddings are correctly stored and retrieved during outbox processing
#[tokio::test]
#[ignore]
async fn test_e2e_insert_with_sparse_embeddings() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;
    let neo4j = get_shared_neo4j().await?;
    let db_name = create_test_database(&postgres).await?;
    let collection_name = format!("test_sparse_{}", Uuid::new_v4());

    // Setup: Connect to database
    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/{db_name}",
        postgres.port()
    );
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_url)
        .await?;

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create repository
    let repo_path = std::path::Path::new("/test/repo");
    let repo_id = postgres_client
        .ensure_repository(repo_path, &collection_name, None)
        .await?;

    // Initialize storage (create collection)
    create_qdrant_collection(&qdrant, &postgres, &neo4j, &db_name, &collection_name, 384).await?;

    // Create entity with sparse embedding
    let entity = create_test_entity(
        "sparse_func",
        "sparse-entity-1",
        "/test/sparse.rs",
        &repo_id.to_string(),
    );

    let dense_embedding = vec![0.1; 384];
    // Create a sparse embedding with a few non-zero values
    // Format: Vec<(index, value)>
    let sparse_embedding: Vec<(u32, f32)> = vec![(5, 0.8), (42, 0.5), (100, 0.3), (255, 0.9)];

    // Store embedding with sparse component
    let content_hash = format!("{:032x}", Uuid::new_v4().as_u128());
    let embedding_ids = postgres_client
        .store_embeddings(
            repo_id,
            &[(content_hash, dense_embedding, Some(sparse_embedding))],
            "test-model",
            384,
        )
        .await?;
    let embedding_id = embedding_ids[0];

    // Store entity with outbox
    let batch = vec![(
        &entity,
        embedding_id,
        OutboxOperation::Insert,
        Uuid::new_v4(),
        TargetStore::Qdrant,
        None, // git_commit
        50,   // token_count
    )];
    postgres_client
        .store_entities_with_outbox_batch(repo_id, &collection_name, &batch)
        .await?;

    // Run outbox processor
    run_outbox_processor_until_empty(
        Arc::clone(&postgres_client),
        &qdrant,
        &postgres,
        &neo4j,
        &db_name,
        Duration::from_secs(30),
    )
    .await?;

    // Verify point was inserted
    assert_point_count(&qdrant, &collection_name, 1).await?;

    // Cleanup
    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_e2e_retry_exhaustion_marks_entry_processed() -> Result<()> {
    init_test_logging();

    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;
    let neo4j = get_shared_neo4j().await?;
    let db_name = create_test_database(&postgres).await?;
    let collection_name = format!("test_retry_{}", Uuid::new_v4());

    // Setup: Connect to database
    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/{db_name}",
        postgres.port()
    );
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_url)
        .await?;

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create repository
    let repo_path = std::path::Path::new("/test/repo");
    let repo_id = postgres_client
        .ensure_repository(repo_path, &collection_name, None)
        .await?;

    // Initialize storage
    create_qdrant_collection(&qdrant, &postgres, &neo4j, &db_name, &collection_name, 384).await?;

    // Create entity metadata
    sqlx::query(
        "INSERT INTO entity_metadata (repository_id, entity_id, qualified_name, name,
         entity_type, language, file_path, visibility, entity_data, git_commit_hash, qdrant_point_id, content)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(repo_id)
    .bind("max-retry-entity")
    .bind("qualified::max-retry-entity")
    .bind("max-retry-entity")
    .bind("function")
    .bind("rust")
    .bind("/test/file.rs")
    .bind("public")
    .bind(serde_json::json!({}))
    .bind("abc123")
    .bind(Uuid::new_v4())
    .bind(None::<String>)
    .execute(&pool)
    .await?;

    // Create outbox entry with retry_count already at max (3)
    sqlx::query(
        "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
         payload, collection_name, retry_count)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(repo_id)
    .bind("max-retry-entity")
    .bind("INSERT")
    .bind("qdrant")
    .bind(serde_json::json!({
        "entity_id": "max-retry-entity",
        "embedding": vec![0.1; 384],
        "qdrant_point_id": Uuid::new_v4().to_string()
    }))
    .bind(&collection_name)
    .bind(3) // Max retries already exhausted
    .execute(&pool)
    .await?;

    // Run outbox processor - entry should be marked as processed immediately
    run_outbox_processor_until_empty(
        Arc::clone(&postgres_client),
        &qdrant,
        &postgres,
        &neo4j,
        &db_name,
        Duration::from_secs(30),
    )
    .await?;

    // Verify entry was marked as processed
    let processed_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM entity_outbox WHERE processed_at IS NOT NULL")
            .fetch_one(&pool)
            .await?;
    assert_eq!(
        processed_count, 1,
        "Entry with exhausted retries should be marked processed"
    );

    // Verify Qdrant has no points (entry was not synced)
    assert_point_count(&qdrant, &collection_name, 0).await?;

    // Cleanup
    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;
    Ok(())
}
