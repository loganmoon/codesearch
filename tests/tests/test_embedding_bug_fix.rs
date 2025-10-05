//! Regression test for embedding bug fix
//!
//! This test validates that the outbox payload contains BOTH entity AND embedding,
//! preventing the bug where embeddings were lost during fallback processing.

use codesearch_core::config::PostgresConfig;
use codesearch_e2e_tests::common::*;
use codesearch_storage::postgres::PostgresClient;
use std::sync::Arc;

#[tokio::test]
async fn test_outbox_payload_contains_entity_and_embedding() {
    logging::init_test_logging();

    let postgres = TestPostgres::start()
        .await
        .expect("Failed to start Postgres");

    // Create a Postgres client
    let config = PostgresConfig {
        host: postgres.host().to_string(),
        port: postgres.port(),
        database: postgres.database().to_string(),
        user: postgres.user().to_string(),
        password: postgres.password().to_string(),
    };

    let postgres_client = Arc::new(
        PostgresClient::new(config)
            .await
            .expect("Failed to create Postgres client"),
    );

    // Run migrations
    postgres_client
        .run_migrations()
        .await
        .expect("Failed to run migrations");

    // Create a test repository
    let repository_id = uuid::Uuid::new_v4();
    postgres_client
        .ensure_repository(repository_id, "test-repo", "/tmp/test")
        .await
        .expect("Failed to create repository");

    // Create a test entity
    let entity = codesearch_core::entities::CodeEntity {
        entity_id: "test-entity-1".to_string(),
        repository_id,
        name: "test_function".to_string(),
        qualified_name: "module::test_function".to_string(),
        entity_type: codesearch_core::entities::EntityType::Function,
        file_path: std::path::PathBuf::from("/tmp/test/src/main.rs"),
        start_line: 10,
        end_line: 20,
        content: "fn test_function() { }".to_string(),
        signature: None,
        parent_entity_id: None,
    };

    // Store entity metadata
    let point_id = uuid::Uuid::new_v4();
    postgres_client
        .store_entity_metadata(repository_id, &entity, Some("abc123".to_string()), point_id)
        .await
        .expect("Failed to store entity metadata");

    // Create embedding (non-zero vector with correct dimensions)
    let embedding: Vec<f32> = (0..1536).map(|i| (i as f32) / 1536.0).collect();

    // Write to outbox with both entity and embedding
    let payload = serde_json::json!({
        "entity": entity,
        "embedding": embedding
    });

    postgres_client
        .write_outbox_entry(
            repository_id,
            &entity.entity_id,
            codesearch_storage::postgres::OutboxOperation::Insert,
            codesearch_storage::postgres::TargetStore::Qdrant,
            payload,
        )
        .await
        .expect("Failed to write outbox entry");

    // Fetch the outbox entry
    let entries = postgres_client
        .get_unprocessed_outbox_entries(codesearch_storage::postgres::TargetStore::Qdrant, 10)
        .await
        .expect("Failed to fetch outbox entries");

    assert_eq!(entries.len(), 1, "Expected exactly one outbox entry");

    let entry = &entries[0];

    // REGRESSION TEST: Verify payload contains "entity" field
    assert!(
        entry.payload.get("entity").is_some(),
        "Outbox payload missing 'entity' field"
    );

    // REGRESSION TEST: Verify payload contains "embedding" field
    assert!(
        entry.payload.get("embedding").is_some(),
        "Outbox payload missing 'embedding' field - this was the bug!"
    );

    // Deserialize and validate entity
    let stored_entity: codesearch_core::entities::CodeEntity =
        serde_json::from_value(entry.payload.get("entity").unwrap().clone())
            .expect("Failed to deserialize entity");
    assert_eq!(stored_entity.entity_id, "test-entity-1");

    // Deserialize and validate embedding
    let stored_embedding: Vec<f32> =
        serde_json::from_value(entry.payload.get("embedding").unwrap().clone())
            .expect("Failed to deserialize embedding");

    // REGRESSION TEST: Verify embedding is non-zero
    assert!(
        stored_embedding.iter().any(|&v| v != 0.0),
        "Embedding vector is all zeros - embeddings were lost!"
    );

    // REGRESSION TEST: Verify embedding has correct dimensions
    assert_eq!(
        stored_embedding.len(),
        1536,
        "Embedding has incorrect dimensions"
    );

    // Verify embedding values match what we stored
    for (i, &val) in stored_embedding.iter().enumerate() {
        let expected = (i as f32) / 1536.0;
        assert!(
            (val - expected).abs() < 0.0001,
            "Embedding value mismatch at index {}: expected {}, got {}",
            i,
            expected,
            val
        );
    }
}
