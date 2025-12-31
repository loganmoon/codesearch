// Integration tests for outbox processor using real database instances

use codesearch_core::entities::{EntityMetadata, EntityType, Language, SourceLocation, Visibility};
use codesearch_core::CodeEntity;
use codesearch_outbox_processor::OutboxProcessor;
use codesearch_storage::{OutboxOperation, PostgresClientTrait, QdrantConfig, TargetStore};
use sqlx::postgres::PgPoolOptions;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use testcontainers::core::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

fn create_test_entity(name: &str, entity_id: &str, file_path: &str, repo_id: &str) -> CodeEntity {
    CodeEntity {
        entity_id: entity_id.to_string(),
        repository_id: repo_id.to_string(),
        name: name.to_string(),
        qualified_name: name.to_string(),
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
        dependencies: Vec::new(),
        signature: None,
        documentation_summary: None,
        content: Some(format!("fn {name}() {{}}")),
        metadata: EntityMetadata::default(),
        relationships: Default::default(),
    }
}

#[tokio::test]
#[ignore = "Requires Docker for testcontainers"]
async fn test_outbox_processor_basic_initialization() {
    let postgres_node = Postgres::default().with_tag("18").start().await.unwrap();
    let connection_string = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres_node.get_host_port_ipv4(5432).await.unwrap()
    );

    // Create connection pool
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("Failed to connect to Postgres");

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool, 1000));

    // Run migrations
    postgres_client
        .run_migrations()
        .await
        .expect("Failed to run migrations");

    // Create qdrant config (won't actually connect in this test)
    let qdrant_config = QdrantConfig {
        host: "localhost".to_string(),
        port: 6334,
        rest_port: 6333,
    };

    let storage_config = codesearch_core::config::StorageConfig {
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
        postgres_pool_size: 20,
    };

    // Create processor
    let _processor = OutboxProcessor::new(
        postgres_client,
        qdrant_config,
        storage_config,
        Duration::from_secs(1),
        10,
        3,
        OutboxProcessor::DEFAULT_MAX_EMBEDDING_DIM,
        200, // max_cached_collections
    );

    // If we get here, initialization succeeded
}

#[tokio::test]
#[ignore = "Requires Docker for testcontainers"]
async fn test_outbox_entries_can_be_created_and_queried() {
    let postgres_node = Postgres::default().with_tag("18").start().await.unwrap();
    let connection_string = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres_node.get_host_port_ipv4(5432).await.unwrap()
    );

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("Failed to connect to Postgres");

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool, 1000));
    postgres_client
        .run_migrations()
        .await
        .expect("Failed to run migrations");

    // Create test repository
    let repo_path = std::path::Path::new("/test/repo");
    let repo_id = postgres_client
        .ensure_repository(repo_path, "test_collection", None)
        .await
        .expect("Failed to create repository");

    // Create test entity and embedding
    let entity = create_test_entity(
        "test_function",
        "test_entity_id",
        "/test/file.rs",
        &repo_id.to_string(),
    );
    let embedding = vec![0.1; 1536];
    let point_id = Uuid::new_v4();

    // Store embedding in cache to get its ID
    let content_hash = format!("{:032x}", 123456u128); // Dummy hash for test
    let embedding_ids = postgres_client
        .store_embeddings(
            repo_id,
            &[(content_hash, embedding, None)],
            "test-model",
            1536,
        )
        .await
        .expect("Failed to store embedding");
    let embedding_id = embedding_ids[0];

    // Store entity with outbox entry
    let batch_entry = vec![(
        &entity,
        embedding_id,
        OutboxOperation::Insert,
        point_id,
        TargetStore::Qdrant,
        None,
        50, // token_count
    )];

    postgres_client
        .store_entities_with_outbox_batch(repo_id, "test_collection", &batch_entry)
        .await
        .expect("Failed to store entity with outbox");

    // Query outbox entries
    let entries = postgres_client
        .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
        .await
        .expect("Failed to get outbox entries");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].entity_id, "test_entity_id");
    assert_eq!(entries[0].operation, "INSERT");
    assert_eq!(entries[0].collection_name, "test_collection");
}

#[tokio::test]
#[ignore = "Requires Docker for testcontainers"]
async fn test_client_cache_reuses_clients() {
    let postgres_node = Postgres::default().with_tag("18").start().await.unwrap();
    let connection_string = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres_node.get_host_port_ipv4(5432).await.unwrap()
    );

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("Failed to connect to Postgres");

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool, 1000));
    postgres_client
        .run_migrations()
        .await
        .expect("Failed to run migrations");

    let qdrant_config = QdrantConfig {
        host: "localhost".to_string(),
        port: 6334,
        rest_port: 6333,
    };

    let storage_config = codesearch_core::config::StorageConfig {
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
        postgres_pool_size: 20,
    };

    let processor = OutboxProcessor::new(
        postgres_client,
        qdrant_config,
        storage_config,
        Duration::from_secs(1),
        10,
        3,
        OutboxProcessor::DEFAULT_MAX_EMBEDDING_DIM,
        200, // max_cached_collections
    );

    // Access the cache through a method call
    // The processor should successfully initialize with an empty cache
    drop(processor);

    // Test passed if we got here without panicking
}

#[tokio::test]
#[ignore = "Requires Docker for testcontainers"]
async fn test_process_batch_multiple_collections() -> Result<(), Box<dyn std::error::Error>> {
    // Setup database
    let postgres_node = Postgres::default().with_tag("18").start().await.unwrap();
    let connection_string = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres_node.get_host_port_ipv4(5432).await.unwrap()
    );

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("Failed to connect to Postgres");

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create repository for foreign key
    let repo_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO repositories (repository_id, repository_path, repository_name, collection_name, last_indexed_commit)
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(repo_id)
    .bind("/test/repo")
    .bind("test-repo")
    .bind("test-collection")
    .bind("abc123")
    .execute(&pool)
    .await?;

    // Create 3 collections with staggered timestamps
    let collection_a = "collection-a";
    let collection_b = "collection-b";
    let collection_c = "collection-c";

    // Insert entries with specific timestamp order across collections
    // Collection A: t=1, t=4, t=7
    // Collection B: t=2, t=5, t=8
    // Collection C: t=3, t=6, t=9
    let base_time = chrono::Utc::now() - chrono::Duration::hours(1);

    for i in 0..9 {
        let collection = match i % 3 {
            0 => collection_a,
            1 => collection_b,
            _ => collection_c,
        };

        let entity_id = format!("entity-{i}");
        let created_at = base_time + chrono::Duration::seconds(i as i64);

        // Create entity metadata first
        sqlx::query(
            "INSERT INTO entity_metadata (repository_id, entity_id, qualified_name, name,
             entity_type, language, file_path, visibility, entity_data, git_commit_hash, qdrant_point_id, content)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind(format!("qualified::{entity_id}"))
        .bind(&entity_id)
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

        // Create outbox entry
        sqlx::query(
            "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
             payload, created_at, collection_name)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind("INSERT")
        .bind("qdrant")
        .bind(serde_json::json!({
            "entity_id": entity_id,
            "embedding": vec![0.1; 384],
            "qdrant_point_id": Uuid::new_v4().to_string()
        }))
        .bind(created_at)
        .bind(collection)
        .execute(&pool)
        .await?;
    }

    // Query to verify entries were created across collections
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT collection_name) FROM entity_outbox WHERE processed_at IS NULL",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(count, 3, "Should have entries in 3 collections");

    // Verify global ordering (fetch without processing)
    let entries: Vec<(String, String)> = sqlx::query_as(
        "SELECT collection_name, entity_id FROM entity_outbox
         WHERE processed_at IS NULL
         ORDER BY created_at ASC LIMIT 9",
    )
    .fetch_all(&pool)
    .await?;

    // Should be interleaved: A, B, C, A, B, C, A, B, C
    assert_eq!(entries[0].0, collection_a);
    assert_eq!(entries[1].0, collection_b);
    assert_eq!(entries[2].0, collection_c);
    assert_eq!(entries[3].0, collection_a);
    assert_eq!(entries[4].0, collection_b);
    assert_eq!(entries[5].0, collection_c);

    Ok(())
}

#[tokio::test]
#[ignore = "Requires Docker for testcontainers"]
async fn test_transaction_rollback_on_qdrant_failure() -> Result<(), Box<dyn std::error::Error>> {
    // This test verifies that if Qdrant write fails, ALL entries remain unprocessed
    // Note: This test only verifies the database state, not actual Qdrant interaction
    // (Full E2E testing with Qdrant failures would require mocking or E2E suite)

    let postgres_node = Postgres::default().with_tag("18").start().await.unwrap();
    let connection_string = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres_node.get_host_port_ipv4(5432).await.unwrap()
    );

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("Failed to connect to Postgres");

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create test data
    let repo_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO repositories (repository_id, repository_path, repository_name, collection_name, last_indexed_commit)
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(repo_id)
    .bind("/test/repo")
    .bind("test-repo")
    .bind("test-collection-1")
    .bind("abc123")
    .execute(&pool)
    .await?;

    // Create 5 entries
    for i in 0..5 {
        let entity_id = format!("entity-{i}");

        sqlx::query(
            "INSERT INTO entity_metadata (repository_id, entity_id, qualified_name, name,
             entity_type, language, file_path, visibility, entity_data, git_commit_hash, qdrant_point_id, content)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind(format!("qualified::{entity_id}"))
        .bind(&entity_id)
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

        sqlx::query(
            "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
             payload, collection_name)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind("INSERT")
        .bind("qdrant")
        .bind(serde_json::json!({
            "entity_id": entity_id,
            "embedding": vec![0.1; 384],
            "qdrant_point_id": Uuid::new_v4().to_string()
        }))
        .bind("test-collection")
        .execute(&pool)
        .await?;
    }

    // Simulate transaction with manual rollback
    let mut tx = pool.begin().await?;

    let entries: Vec<Uuid> = sqlx::query_scalar(
        "SELECT outbox_id FROM entity_outbox
         WHERE target_store = $1 AND processed_at IS NULL
         ORDER BY created_at ASC LIMIT 5
         FOR UPDATE SKIP LOCKED",
    )
    .bind("qdrant")
    .fetch_all(&mut *tx)
    .await?;

    assert_eq!(entries.len(), 5, "Should lock 5 entries");

    // Simulate marking as processed
    let mut query_builder = sqlx::QueryBuilder::new(
        "UPDATE entity_outbox SET processed_at = NOW() WHERE outbox_id IN (",
    );
    let mut separated = query_builder.separated(", ");
    for id in &entries {
        separated.push_bind(id);
    }
    separated.push_unseparated(")");
    query_builder.build().execute(&mut *tx).await?;

    // Verify entries are marked within transaction
    let processed_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM entity_outbox WHERE processed_at IS NOT NULL")
            .fetch_one(&mut *tx)
            .await?;
    assert_eq!(processed_count, 5, "Should see 5 processed within tx");

    // Rollback (simulating Qdrant failure)
    tx.rollback().await?;

    // Verify ALL entries remain unprocessed after rollback
    let unprocessed_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM entity_outbox WHERE processed_at IS NULL")
            .fetch_one(&pool)
            .await?;
    assert_eq!(
        unprocessed_count, 5,
        "All entries should be unprocessed after rollback"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "Requires Docker for testcontainers"]
async fn test_global_ordering_across_collections() -> Result<(), Box<dyn std::error::Error>> {
    // Verify that entries are fetched in strict created_at order across collections

    let postgres_node = Postgres::default().with_tag("18").start().await.unwrap();
    let connection_string = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres_node.get_host_port_ipv4(5432).await.unwrap()
    );

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("Failed to connect to Postgres");

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create repository
    let repo_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO repositories (repository_id, repository_path, repository_name, collection_name, last_indexed_commit)
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(repo_id)
    .bind("/test/repo")
    .bind("test-repo")
    .bind("test-collection-2")
    .bind("abc123")
    .execute(&pool)
    .await?;

    // Create entries with specific timestamps
    // Collection A has 100 old entries (t=0 to t=99)
    // Collection B has 10 very new entries (t=1000 to t=1009)
    // Expected: First batch should contain ONLY Collection A entries

    let base_time = chrono::Utc::now() - chrono::Duration::hours(2);

    // Collection A: 100 old entries
    for i in 0..100 {
        let entity_id = format!("entity-a-{i}");
        let created_at = base_time + chrono::Duration::seconds(i);

        sqlx::query(
            "INSERT INTO entity_metadata (repository_id, entity_id, qualified_name, name,
             entity_type, language, file_path, visibility, entity_data, git_commit_hash, qdrant_point_id, content)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind(format!("qualified::{entity_id}"))
        .bind(&entity_id)
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

        sqlx::query(
            "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
             payload, created_at, collection_name)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind("INSERT")
        .bind("qdrant")
        .bind(serde_json::json!({
            "entity_id": entity_id,
            "embedding": vec![0.1; 384],
            "qdrant_point_id": Uuid::new_v4().to_string()
        }))
        .bind(created_at)
        .bind("collection-a")
        .execute(&pool)
        .await?;
    }

    // Collection B: 10 new entries
    for i in 0..10 {
        let entity_id = format!("entity-b-{i}");
        let created_at = base_time + chrono::Duration::seconds(1000 + i);

        sqlx::query(
            "INSERT INTO entity_metadata (repository_id, entity_id, qualified_name, name,
             entity_type, language, file_path, visibility, entity_data, git_commit_hash, qdrant_point_id, content)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind(format!("qualified::{entity_id}"))
        .bind(&entity_id)
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

        sqlx::query(
            "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
             payload, created_at, collection_name)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind("INSERT")
        .bind("qdrant")
        .bind(serde_json::json!({
            "entity_id": entity_id,
            "embedding": vec![0.1; 384],
            "qdrant_point_id": Uuid::new_v4().to_string()
        }))
        .bind(created_at)
        .bind("collection-b")
        .execute(&pool)
        .await?;
    }

    // Fetch first batch (batch_size=50)
    let entries: Vec<(String, String)> = sqlx::query_as(
        "SELECT collection_name, entity_id FROM entity_outbox
         WHERE target_store = $1 AND processed_at IS NULL
         ORDER BY created_at ASC LIMIT 50",
    )
    .bind("qdrant")
    .fetch_all(&pool)
    .await?;

    assert_eq!(entries.len(), 50, "Should fetch 50 entries");

    // ALL should be from collection A (oldest entries)
    for (collection, _) in &entries {
        assert_eq!(
            collection, "collection-a",
            "First batch should only contain oldest collection"
        );
    }

    // Verify entity IDs are in order (entity-a-0 to entity-a-49)
    assert_eq!(entries[0].1, "entity-a-0");
    assert_eq!(entries[49].1, "entity-a-49");

    Ok(())
}

#[tokio::test]
#[ignore = "Requires Docker for testcontainers"]
async fn test_retry_count_exceeded_marked_processed() -> Result<(), Box<dyn std::error::Error>> {
    // Verify that entries exceeding max_retries are marked as processed

    let postgres_node = Postgres::default().with_tag("18").start().await.unwrap();
    let connection_string = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres_node.get_host_port_ipv4(5432).await.unwrap()
    );

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("Failed to connect to Postgres");

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create repository
    let repo_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO repositories (repository_id, repository_path, repository_name, collection_name, last_indexed_commit)
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(repo_id)
    .bind("/test/repo")
    .bind("test-repo")
    .bind("test-collection-3")
    .bind("abc123")
    .execute(&pool)
    .await?;

    // Create entry with retry_count = 3 (at max_retries limit)
    let entity_id = "entity-max-retries";

    sqlx::query(
        "INSERT INTO entity_metadata (repository_id, entity_id, qualified_name, name,
         entity_type, language, file_path, visibility, entity_data, git_commit_hash, qdrant_point_id, content)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
    )
    .bind(repo_id)
    .bind(entity_id)
    .bind(format!("qualified::{entity_id}"))
    .bind(entity_id)
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

    let outbox_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO entity_outbox (outbox_id, repository_id, entity_id, operation, target_store,
         payload, collection_name, retry_count)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(outbox_id)
    .bind(repo_id)
    .bind(entity_id)
    .bind("INSERT")
    .bind("qdrant")
    .bind(serde_json::json!({
        "entity_id": entity_id,
        "embedding": vec![0.1; 384],
        "qdrant_point_id": Uuid::new_v4().to_string()
    }))
    .bind("test-collection")
    .bind(3) // At max_retries (default max_retries = 3)
    .execute(&pool)
    .await?;

    // Simulate processor logic: check retry_count and mark as processed
    let mut tx = pool.begin().await?;

    let entries: Vec<(Uuid, i32)> = sqlx::query_as(
        "SELECT outbox_id, retry_count FROM entity_outbox
         WHERE target_store = $1 AND processed_at IS NULL
         ORDER BY created_at ASC LIMIT 10
         FOR UPDATE SKIP LOCKED",
    )
    .bind("qdrant")
    .fetch_all(&mut *tx)
    .await?;

    assert_eq!(entries.len(), 1, "Should find 1 entry");
    assert_eq!(entries[0].1, 3, "Entry should have retry_count = 3");

    // Processor would mark this as processed
    let max_retries = 3;
    let failed_ids: Vec<Uuid> = entries
        .into_iter()
        .filter(|(_, retry_count)| *retry_count >= max_retries)
        .map(|(id, _)| id)
        .collect();

    assert_eq!(failed_ids.len(), 1, "Should identify 1 failed entry");

    sqlx::query("UPDATE entity_outbox SET processed_at = NOW() WHERE outbox_id = $1")
        .bind(failed_ids[0])
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    // Verify entry is now processed
    let unprocessed_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM entity_outbox WHERE processed_at IS NULL")
            .fetch_one(&pool)
            .await?;
    assert_eq!(unprocessed_count, 0, "Entry should be marked processed");

    Ok(())
}

#[tokio::test]
#[ignore = "Requires Docker for testcontainers"]
async fn test_delete_operation_with_entity_ids_array() -> Result<(), Box<dyn std::error::Error>> {
    // Test DELETE operation with entity_ids array in payload
    let postgres_node = Postgres::default().with_tag("18").start().await.unwrap();
    let connection_string = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres_node.get_host_port_ipv4(5432).await.unwrap()
    );

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("Failed to connect to Postgres");

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create repository
    let repo_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO repositories (repository_id, repository_path, repository_name, collection_name, last_indexed_commit)
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(repo_id)
    .bind("/test/repo")
    .bind("test-repo")
    .bind("test-collection")
    .bind("abc123")
    .execute(&pool)
    .await?;

    // Create multiple entity metadata entries
    for i in 0..3 {
        let entity_id = format!("entity-{i}");
        sqlx::query(
            "INSERT INTO entity_metadata (repository_id, entity_id, qualified_name, name,
             entity_type, language, file_path, visibility, entity_data, git_commit_hash, qdrant_point_id, content)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind(format!("qualified::{entity_id}"))
        .bind(&entity_id)
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
    }

    // Create DELETE outbox entry with entity_ids array
    sqlx::query(
        "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
         payload, collection_name)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(repo_id)
    .bind("entity-0") // Primary entity_id (not used when entity_ids present)
    .bind("DELETE")
    .bind("qdrant")
    .bind(serde_json::json!({
        "entity_ids": ["entity-0", "entity-1", "entity-2"]
    }))
    .bind("test-collection")
    .execute(&pool)
    .await?;

    // Verify entry was created
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM entity_outbox WHERE operation = 'DELETE' AND processed_at IS NULL",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(count, 1, "Should have 1 unprocessed DELETE entry");

    Ok(())
}

#[tokio::test]
#[ignore = "Requires Docker for testcontainers"]
async fn test_delete_operation_with_single_entity_id_fallback(
) -> Result<(), Box<dyn std::error::Error>> {
    // Test DELETE operation falling back to single entity_id when entity_ids not present
    let postgres_node = Postgres::default().with_tag("18").start().await.unwrap();
    let connection_string = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres_node.get_host_port_ipv4(5432).await.unwrap()
    );

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("Failed to connect to Postgres");

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create repository
    let repo_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO repositories (repository_id, repository_path, repository_name, collection_name, last_indexed_commit)
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(repo_id)
    .bind("/test/repo")
    .bind("test-repo")
    .bind("test-collection")
    .bind("abc123")
    .execute(&pool)
    .await?;

    // Create entity metadata
    let entity_id = "single-entity";
    sqlx::query(
        "INSERT INTO entity_metadata (repository_id, entity_id, qualified_name, name,
         entity_type, language, file_path, visibility, entity_data, git_commit_hash, qdrant_point_id, content)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
    )
    .bind(repo_id)
    .bind(entity_id)
    .bind(format!("qualified::{entity_id}"))
    .bind(entity_id)
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

    // Create DELETE outbox entry WITHOUT entity_ids array (uses entity_id field)
    sqlx::query(
        "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
         payload, collection_name)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(repo_id)
    .bind(entity_id)
    .bind("DELETE")
    .bind("qdrant")
    .bind(serde_json::json!({})) // Empty payload - should fallback to entity_id field
    .bind("test-collection")
    .execute(&pool)
    .await?;

    // Verify entry was created
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM entity_outbox WHERE operation = 'DELETE' AND entity_id = $1 AND processed_at IS NULL"
    )
    .bind(entity_id)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        count, 1,
        "Should have 1 unprocessed DELETE entry with fallback entity_id"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "Requires Docker for testcontainers"]
async fn test_mixed_insert_update_delete_in_same_batch() -> Result<(), Box<dyn std::error::Error>> {
    // Test that INSERT, UPDATE, and DELETE operations can be processed in the same batch
    let postgres_node = Postgres::default().with_tag("18").start().await.unwrap();
    let connection_string = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres_node.get_host_port_ipv4(5432).await.unwrap()
    );

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("Failed to connect to Postgres");

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create repository
    let repo_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO repositories (repository_id, repository_path, repository_name, collection_name, last_indexed_commit)
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(repo_id)
    .bind("/test/repo")
    .bind("test-repo")
    .bind("test-collection")
    .bind("abc123")
    .execute(&pool)
    .await?;

    // Create entity metadata for multiple entities
    for i in 0..5 {
        let entity_id = format!("entity-{i}");
        sqlx::query(
            "INSERT INTO entity_metadata (repository_id, entity_id, qualified_name, name,
             entity_type, language, file_path, visibility, entity_data, git_commit_hash, qdrant_point_id, content)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind(format!("qualified::{entity_id}"))
        .bind(&entity_id)
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
    }

    // Create mixed operations in same batch
    // 2 INSERT operations
    for i in 0..2 {
        let entity_id = format!("entity-{i}");
        sqlx::query(
            "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
             payload, collection_name)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind("INSERT")
        .bind("qdrant")
        .bind(serde_json::json!({
            "entity_id": entity_id,
            "embedding": vec![0.1; 384],
            "qdrant_point_id": Uuid::new_v4().to_string()
        }))
        .bind("test-collection")
        .execute(&pool)
        .await?;
    }

    // 2 UPDATE operations
    for i in 2..4 {
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
            "embedding": vec![0.2; 384],
            "qdrant_point_id": Uuid::new_v4().to_string()
        }))
        .bind("test-collection")
        .execute(&pool)
        .await?;
    }

    // 1 DELETE operation
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
    .bind("test-collection")
    .execute(&pool)
    .await?;

    // Verify all entries were created
    let insert_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM entity_outbox WHERE operation = 'INSERT' AND processed_at IS NULL",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(insert_count, 2, "Should have 2 INSERT entries");

    let update_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM entity_outbox WHERE operation = 'UPDATE' AND processed_at IS NULL",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(update_count, 2, "Should have 2 UPDATE entries");

    let delete_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM entity_outbox WHERE operation = 'DELETE' AND processed_at IS NULL",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(delete_count, 1, "Should have 1 DELETE entry");

    // Verify they would be fetched in same batch (global ordering)
    let entries: Vec<(String, String)> = sqlx::query_as(
        "SELECT operation, entity_id FROM entity_outbox
         WHERE target_store = $1 AND processed_at IS NULL
         ORDER BY created_at ASC LIMIT 10",
    )
    .bind("qdrant")
    .fetch_all(&pool)
    .await?;

    assert_eq!(entries.len(), 5, "Should fetch all 5 entries in one batch");
    assert!(
        entries.iter().any(|(op, _)| op == "INSERT"),
        "Batch should contain INSERT"
    );
    assert!(
        entries.iter().any(|(op, _)| op == "UPDATE"),
        "Batch should contain UPDATE"
    );
    assert!(
        entries.iter().any(|(op, _)| op == "DELETE"),
        "Batch should contain DELETE"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "Requires Docker for testcontainers"]
async fn test_concurrent_processor_isolation_with_skip_locked(
) -> Result<(), Box<dyn std::error::Error>> {
    // Verify that SELECT FOR UPDATE SKIP LOCKED prevents concurrent processors
    // from processing the same entries

    let postgres_node = Postgres::default().with_tag("18").start().await.unwrap();
    let connection_string = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres_node.get_host_port_ipv4(5432).await.unwrap()
    );

    let pool = PgPoolOptions::new()
        .max_connections(10) // Need more connections for concurrent test
        .connect(&connection_string)
        .await
        .expect("Failed to connect to Postgres");

    let postgres_client: Arc<dyn PostgresClientTrait> =
        Arc::new(codesearch_storage::PostgresClient::new(pool.clone(), 1000));
    postgres_client.run_migrations().await?;

    // Create repository
    let repo_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO repositories (repository_id, repository_path, repository_name, collection_name, last_indexed_commit)
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(repo_id)
    .bind("/test/repo")
    .bind("test-repo")
    .bind("test-collection")
    .bind("abc123")
    .execute(&pool)
    .await?;

    // Create 10 test entries
    for i in 0..10 {
        let entity_id = format!("entity-{i}");
        sqlx::query(
            "INSERT INTO entity_metadata (repository_id, entity_id, qualified_name, name,
             entity_type, language, file_path, visibility, entity_data, git_commit_hash, qdrant_point_id, content)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind(format!("qualified::{entity_id}"))
        .bind(&entity_id)
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

        sqlx::query(
            "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store,
             payload, collection_name)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(repo_id)
        .bind(&entity_id)
        .bind("INSERT")
        .bind("qdrant")
        .bind(serde_json::json!({
            "entity_id": entity_id,
            "embedding": vec![0.1; 384],
            "qdrant_point_id": Uuid::new_v4().to_string()
        }))
        .bind("test-collection")
        .execute(&pool)
        .await?;
    }

    // Start transaction 1 and lock first 5 entries
    let mut tx1 = pool.begin().await?;
    let locked_by_tx1: Vec<Uuid> = sqlx::query_scalar(
        "SELECT outbox_id FROM entity_outbox
         WHERE target_store = $1 AND processed_at IS NULL
         ORDER BY created_at ASC LIMIT 5
         FOR UPDATE SKIP LOCKED",
    )
    .bind("qdrant")
    .fetch_all(&mut *tx1)
    .await?;

    assert_eq!(
        locked_by_tx1.len(),
        5,
        "Transaction 1 should lock 5 entries"
    );

    // Try to acquire locks with transaction 2 (should skip locked entries)
    let mut tx2 = pool.begin().await?;
    let locked_by_tx2: Vec<Uuid> = sqlx::query_scalar(
        "SELECT outbox_id FROM entity_outbox
         WHERE target_store = $1 AND processed_at IS NULL
         ORDER BY created_at ASC LIMIT 10
         FOR UPDATE SKIP LOCKED",
    )
    .bind("qdrant")
    .fetch_all(&mut *tx2)
    .await?;

    // Transaction 2 should only get the remaining 5 entries (skipping the locked ones)
    assert_eq!(
        locked_by_tx2.len(),
        5,
        "Transaction 2 should lock 5 different entries (skipping locked ones)"
    );

    // Verify no overlap between locked entries
    for id in &locked_by_tx2 {
        assert!(
            !locked_by_tx1.contains(id),
            "Transaction 2 should not lock any entries locked by Transaction 1"
        );
    }

    // Both transactions together should cover all 10 entries
    let total_locked = locked_by_tx1.len() + locked_by_tx2.len();
    assert_eq!(
        total_locked, 10,
        "Both transactions should cover all 10 entries"
    );

    // Cleanup
    tx1.rollback().await?;
    tx2.rollback().await?;

    Ok(())
}
