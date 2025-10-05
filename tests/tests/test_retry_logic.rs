//! Tests for outbox processor retry logic
//!
//! Validates that failed outbox entries are retried correctly and eventually
//! skipped after exceeding max retries.

use codesearch_core::config::PostgresConfig;
use codesearch_e2e_tests::common::*;
use codesearch_storage::postgres::{OutboxOperation, PostgresClient, TargetStore};
use std::sync::Arc;

#[tokio::test]
async fn test_outbox_retry_count_increments() {
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

    // Create an outbox entry with invalid payload (missing embedding)
    let invalid_payload = serde_json::json!({
        "entity": {
            "entity_id": "test-entity-1",
            "repository_id": repository_id.to_string(),
            "name": "test",
            "qualified_name": "test",
            "entity_type": "Function",
            "file_path": "/tmp/test.rs",
            "start_line": 1,
            "end_line": 10,
            "content": "fn test() {}",
            "signature": null,
            "parent_entity_id": null
        }
        // Missing "embedding" field - will cause processing to fail
    });

    postgres_client
        .write_outbox_entry(
            repository_id,
            "test-entity-1",
            OutboxOperation::Insert,
            TargetStore::Qdrant,
            invalid_payload,
        )
        .await
        .expect("Failed to write outbox entry");

    // Fetch the entry
    let entries = postgres_client
        .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
        .await
        .expect("Failed to fetch outbox entries");

    assert_eq!(entries.len(), 1);
    let outbox_id = entries[0].outbox_id;
    assert_eq!(entries[0].retry_count, 0, "Initial retry count should be 0");

    // Simulate failure by recording a failure
    postgres_client
        .record_outbox_failure(outbox_id, "Missing embedding in payload")
        .await
        .expect("Failed to record failure");

    // Fetch again and verify retry count incremented
    let entries = postgres_client
        .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
        .await
        .expect("Failed to fetch outbox entries");

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].retry_count, 1,
        "Retry count should increment after failure"
    );
    assert!(
        entries[0].last_error.is_some(),
        "Last error should be recorded"
    );
    assert!(
        entries[0]
            .last_error
            .as_ref()
            .unwrap()
            .contains("Missing embedding"),
        "Error message should be stored"
    );

    // Record multiple failures
    for i in 1..5 {
        postgres_client
            .record_outbox_failure(outbox_id, &format!("Retry attempt {i}"))
            .await
            .expect("Failed to record failure");

        let entries = postgres_client
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await
            .expect("Failed to fetch outbox entries");

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].retry_count,
            i + 1,
            "Retry count should be {}",
            i + 1
        );
    }
}

#[tokio::test]
async fn test_max_retries_skips_entry() {
    logging::init_test_logging();

    let postgres = TestPostgres::start()
        .await
        .expect("Failed to start Postgres");

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

    postgres_client
        .run_migrations()
        .await
        .expect("Failed to run migrations");

    let repository_id = uuid::Uuid::new_v4();
    postgres_client
        .ensure_repository(repository_id, "test-repo", "/tmp/test")
        .await
        .expect("Failed to create repository");

    // Create a valid entry
    let valid_payload = serde_json::json!({
        "entity": {
            "entity_id": "test-entity-valid",
            "repository_id": repository_id.to_string(),
            "name": "test",
            "qualified_name": "test",
            "entity_type": "Function",
            "file_path": "/tmp/test.rs",
            "start_line": 1,
            "end_line": 10,
            "content": "fn test() {}",
            "signature": null,
            "parent_entity_id": null
        },
        "embedding": vec![0.1f32; 1536]
    });

    postgres_client
        .write_outbox_entry(
            repository_id,
            "test-entity-valid",
            OutboxOperation::Insert,
            TargetStore::Qdrant,
            valid_payload,
        )
        .await
        .expect("Failed to write valid outbox entry");

    // Create an invalid entry that will exceed max retries
    let invalid_payload = serde_json::json!({
        "entity": {
            "entity_id": "test-entity-invalid",
            "repository_id": repository_id.to_string(),
            "name": "test2",
            "qualified_name": "test2",
            "entity_type": "Function",
            "file_path": "/tmp/test2.rs",
            "start_line": 1,
            "end_line": 10,
            "content": "fn test2() {}",
            "signature": null,
            "parent_entity_id": null
        }
        // Missing embedding
    });

    postgres_client
        .write_outbox_entry(
            repository_id,
            "test-entity-invalid",
            OutboxOperation::Insert,
            TargetStore::Qdrant,
            invalid_payload,
        )
        .await
        .expect("Failed to write invalid outbox entry");

    // Fail the invalid entry 3 times (default max_retries)
    let entries = postgres_client
        .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
        .await
        .expect("Failed to fetch outbox entries");

    let invalid_entry = entries
        .iter()
        .find(|e| e.entity_id == "test-entity-invalid")
        .unwrap();

    for _ in 0..3 {
        postgres_client
            .record_outbox_failure(invalid_entry.outbox_id, "Missing embedding")
            .await
            .expect("Failed to record failure");
    }

    // Verify the invalid entry now has retry_count >= 3
    let entries = postgres_client
        .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
        .await
        .expect("Failed to fetch outbox entries");

    let invalid_entry = entries
        .iter()
        .find(|e| e.entity_id == "test-entity-invalid")
        .unwrap();

    assert!(
        invalid_entry.retry_count >= 3,
        "Invalid entry should have retry_count >= 3, got {}",
        invalid_entry.retry_count
    );

    // NOTE: The actual skipping logic is tested in the outbox processor integration
    // This test validates that the retry_count is properly tracked in the database
}
