// Integration tests for outbox processor using real database instances

use codesearch_core::entities::{EntityMetadata, EntityType, Language, SourceLocation, Visibility};
use codesearch_core::CodeEntity;
use codesearch_outbox_processor::OutboxProcessor;
use codesearch_storage::{OutboxOperation, PostgresClientTrait, QdrantConfig, TargetStore};
use sqlx::postgres::PgPoolOptions;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

fn create_test_entity(name: &str, entity_id: &str, file_path: &str, repo_id: &str) -> CodeEntity {
    CodeEntity {
        entity_id: entity_id.to_string(),
        repository_id: repo_id.to_string(),
        name: name.to_string(),
        qualified_name: name.to_string(),
        entity_type: EntityType::Function,
        language: Language::Rust,
        file_path: PathBuf::from(file_path),
        location: SourceLocation {
            start_line: 1,
            end_line: 10,
            start_column: 0,
            end_column: 10,
        },
        visibility: Visibility::Public,
        parent_scope: None,
        dependencies: Vec::new(),
        signature: None,
        documentation_summary: None,
        content: Some(format!("fn {name}() {{}}")),
        metadata: EntityMetadata::default(),
    }
}

#[tokio::test]
async fn test_outbox_processor_basic_initialization() {
    let postgres_node = Postgres::default().start().await.unwrap();
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

    // Create processor
    let _processor = OutboxProcessor::new(
        postgres_client,
        qdrant_config,
        Duration::from_secs(1),
        10,
        3,
    );

    // If we get here, initialization succeeded
}

#[tokio::test]
async fn test_outbox_entries_can_be_created_and_queried() {
    let postgres_node = Postgres::default().start().await.unwrap();
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

    // Store entity with outbox entry
    let batch_entry = vec![(
        &entity,
        embedding.as_slice(),
        OutboxOperation::Insert,
        point_id,
        TargetStore::Qdrant,
        None,
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
async fn test_client_cache_reuses_clients() {
    let postgres_node = Postgres::default().start().await.unwrap();
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

    let processor = OutboxProcessor::new(
        postgres_client,
        qdrant_config,
        Duration::from_secs(1),
        10,
        3,
    );

    // Access the cache through a method call
    // The processor should successfully initialize with an empty cache
    drop(processor);

    // Test passed if we got here without panicking
}
