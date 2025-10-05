//! E2E test for entity deletion flow
//!
//! Tests the complete deletion pathway:
//! 1. Mark entities as deleted in Postgres
//! 2. Write DELETE operation to outbox
//! 3. Outbox processor removes entities from Qdrant

use codesearch_core::config::{PostgresConfig, QdrantConfig, StorageConfig};
use codesearch_e2e_tests::common::*;
use codesearch_storage::{
    create_storage_client,
    postgres::{OutboxOperation, PostgresClient, TargetStore},
    StorageClient,
};
use std::sync::Arc;

#[tokio::test]
async fn test_deletion_flow_through_outbox() {
    logging::init_test_logging();

    let qdrant = TestQdrant::start().await.expect("Failed to start Qdrant");
    let postgres = TestPostgres::start()
        .await
        .expect("Failed to start Postgres");
    let outbox = TestOutboxProcessor::start(&qdrant, &postgres)
        .await
        .expect("Failed to start outbox processor");

    // Create Postgres client
    let pg_config = PostgresConfig {
        host: postgres.host().to_string(),
        port: postgres.port(),
        database: postgres.database().to_string(),
        user: postgres.user().to_string(),
        password: postgres.password().to_string(),
    };

    let postgres_client = Arc::new(
        PostgresClient::new(pg_config)
            .await
            .expect("Failed to create Postgres client"),
    );

    postgres_client
        .run_migrations()
        .await
        .expect("Failed to run migrations");

    // Create Qdrant storage client
    let collection_name = format!("test-deletion-{}", uuid::Uuid::new_v4());
    let storage_config = StorageConfig {
        postgres: PostgresConfig {
            host: postgres.host().to_string(),
            port: postgres.port(),
            database: postgres.database().to_string(),
            user: postgres.user().to_string(),
            password: postgres.password().to_string(),
        },
        qdrant: QdrantConfig {
            host: qdrant.host().to_string(),
            port: qdrant.http_port(),
            grpc_port: qdrant.grpc_port(),
            collection: collection_name.clone(),
            embedding_dimension: 1536,
        },
    };

    let qdrant_client = Arc::new(
        create_storage_client(&storage_config)
            .await
            .expect("Failed to create Qdrant client"),
    );

    // Create collection
    qdrant_client
        .create_collection()
        .await
        .expect("Failed to create collection");

    // Create a test repository
    let repository_id = uuid::Uuid::new_v4();
    postgres_client
        .ensure_repository(repository_id, "test-repo", "/tmp/test")
        .await
        .expect("Failed to create repository");

    // Create test entities
    let entity1 = codesearch_core::entities::CodeEntity {
        entity_id: "entity-to-delete-1".to_string(),
        repository_id,
        name: "function1".to_string(),
        qualified_name: "module::function1".to_string(),
        entity_type: codesearch_core::entities::EntityType::Function,
        file_path: std::path::PathBuf::from("/tmp/test/src/main.rs"),
        start_line: 10,
        end_line: 20,
        content: "fn function1() { }".to_string(),
        signature: None,
        parent_entity_id: None,
    };

    let entity2 = codesearch_core::entities::CodeEntity {
        entity_id: "entity-to-delete-2".to_string(),
        repository_id,
        name: "function2".to_string(),
        qualified_name: "module::function2".to_string(),
        entity_type: codesearch_core::entities::EntityType::Function,
        file_path: std::path::PathBuf::from("/tmp/test/src/main.rs"),
        start_line: 30,
        end_line: 40,
        content: "fn function2() { }".to_string(),
        signature: None,
        parent_entity_id: None,
    };

    let entity3 = codesearch_core::entities::CodeEntity {
        entity_id: "entity-to-keep".to_string(),
        repository_id,
        name: "function3".to_string(),
        qualified_name: "module::function3".to_string(),
        entity_type: codesearch_core::entities::EntityType::Function,
        file_path: std::path::PathBuf::from("/tmp/test/src/lib.rs"),
        start_line: 10,
        end_line: 20,
        content: "fn function3() { }".to_string(),
        signature: None,
        parent_entity_id: None,
    };

    // Store entities in Postgres and write INSERT to outbox
    let embeddings: Vec<Vec<f32>> = (0..3)
        .map(|i| (0..1536).map(|j| (i * 1000 + j) as f32 / 1536.0).collect())
        .collect();

    for (entity, embedding) in [&entity1, &entity2, &entity3].iter().zip(embeddings.iter()) {
        let point_id = uuid::Uuid::new_v4();
        postgres_client
            .store_entity_metadata(repository_id, entity, Some("abc123".to_string()), point_id)
            .await
            .expect("Failed to store entity metadata");

        let payload = serde_json::json!({
            "entity": entity,
            "embedding": embedding
        });

        postgres_client
            .write_outbox_entry(
                repository_id,
                &entity.entity_id,
                OutboxOperation::Insert,
                TargetStore::Qdrant,
                payload,
            )
            .await
            .expect("Failed to write INSERT outbox entry");
    }

    // Wait for outbox processor to sync entities to Qdrant
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Verify all entities are in Qdrant
    let search_result1 = qdrant_client
        .search_similar(&embeddings[0], 10)
        .await
        .expect("Failed to search");
    assert!(
        !search_result1.is_empty(),
        "Entity 1 should be in Qdrant before deletion"
    );

    // Mark entities 1 and 2 as deleted in Postgres
    postgres_client
        .mark_entities_deleted(
            repository_id,
            &[
                "entity-to-delete-1".to_string(),
                "entity-to-delete-2".to_string(),
            ],
        )
        .await
        .expect("Failed to mark entities as deleted");

    // Write DELETE operation to outbox
    let delete_payload = serde_json::json!({
        "entity_ids": ["entity-to-delete-1", "entity-to-delete-2"]
    });

    postgres_client
        .write_outbox_entry(
            repository_id,
            "entity-to-delete-1", // Use first entity_id as identifier
            OutboxOperation::Delete,
            TargetStore::Qdrant,
            delete_payload,
        )
        .await
        .expect("Failed to write DELETE outbox entry");

    // Wait for outbox processor to process DELETE
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Verify entities are deleted from Postgres (soft delete)
    let entities = postgres_client
        .get_entities_by_ids(&[
            (repository_id, "entity-to-delete-1".to_string()),
            (repository_id, "entity-to-delete-2".to_string()),
        ])
        .await
        .expect("Failed to fetch entities");

    assert_eq!(
        entities.len(),
        0,
        "Deleted entities should not be returned from Postgres"
    );

    // Verify entity3 is still in Postgres
    let entities = postgres_client
        .get_entities_by_ids(&[(repository_id, "entity-to-keep".to_string())])
        .await
        .expect("Failed to fetch entity3");

    assert_eq!(
        entities.len(),
        1,
        "Non-deleted entity should still be in Postgres"
    );

    // Verify entities are deleted from Qdrant
    // Note: This is indirect - we verify by searching and checking the count
    let total_results = qdrant_client
        .search_similar(&embeddings[2], 10)
        .await
        .expect("Failed to search");

    // We should only find entity3 now (entities 1 and 2 should be deleted)
    assert_eq!(
        total_results.len(),
        1,
        "Only 1 entity should remain in Qdrant after deletion"
    );

    // Cleanup
    drop(outbox);
    cleanup::cleanup_qdrant(&qdrant, &collection_name).await;
    cleanup::cleanup_postgres(&postgres).await;
}

#[tokio::test]
async fn test_delete_single_entity() {
    logging::init_test_logging();

    let qdrant = TestQdrant::start().await.expect("Failed to start Qdrant");
    let postgres = TestPostgres::start()
        .await
        .expect("Failed to start Postgres");
    let outbox = TestOutboxProcessor::start(&qdrant, &postgres)
        .await
        .expect("Failed to start outbox processor");

    let pg_config = PostgresConfig {
        host: postgres.host().to_string(),
        port: postgres.port(),
        database: postgres.database().to_string(),
        user: postgres.user().to_string(),
        password: postgres.password().to_string(),
    };

    let postgres_client = Arc::new(
        PostgresClient::new(pg_config)
            .await
            .expect("Failed to create Postgres client"),
    );

    postgres_client
        .run_migrations()
        .await
        .expect("Failed to run migrations");

    let collection_name = format!("test-single-delete-{}", uuid::Uuid::new_v4());
    let storage_config = StorageConfig {
        postgres: PostgresConfig {
            host: postgres.host().to_string(),
            port: postgres.port(),
            database: postgres.database().to_string(),
            user: postgres.user().to_string(),
            password: postgres.password().to_string(),
        },
        qdrant: QdrantConfig {
            host: qdrant.host().to_string(),
            port: qdrant.http_port(),
            grpc_port: qdrant.grpc_port(),
            collection: collection_name.clone(),
            embedding_dimension: 1536,
        },
    };

    let qdrant_client = Arc::new(
        create_storage_client(&storage_config)
            .await
            .expect("Failed to create Qdrant client"),
    );

    qdrant_client
        .create_collection()
        .await
        .expect("Failed to create collection");

    let repository_id = uuid::Uuid::new_v4();
    postgres_client
        .ensure_repository(repository_id, "test-repo", "/tmp/test")
        .await
        .expect("Failed to create repository");

    // Create single entity
    let entity = codesearch_core::entities::CodeEntity {
        entity_id: "single-entity".to_string(),
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

    let embedding: Vec<f32> = (0..1536).map(|i| (i as f32) / 1536.0).collect();

    let point_id = uuid::Uuid::new_v4();
    postgres_client
        .store_entity_metadata(repository_id, &entity, Some("abc123".to_string()), point_id)
        .await
        .expect("Failed to store entity metadata");

    let payload = serde_json::json!({
        "entity": entity,
        "embedding": embedding
    });

    postgres_client
        .write_outbox_entry(
            repository_id,
            &entity.entity_id,
            OutboxOperation::Insert,
            TargetStore::Qdrant,
            payload,
        )
        .await
        .expect("Failed to write INSERT outbox entry");

    // Wait for sync
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Delete the entity (without entity_ids array in payload)
    postgres_client
        .mark_entities_deleted(repository_id, &["single-entity".to_string()])
        .await
        .expect("Failed to mark entity as deleted");

    // Write DELETE with minimal payload (no entity_ids array)
    let delete_payload = serde_json::json!({});

    postgres_client
        .write_outbox_entry(
            repository_id,
            "single-entity",
            OutboxOperation::Delete,
            TargetStore::Qdrant,
            delete_payload,
        )
        .await
        .expect("Failed to write DELETE outbox entry");

    // Wait for deletion
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Verify entity is deleted
    let results = qdrant_client
        .search_similar(&embedding, 10)
        .await
        .expect("Failed to search");

    assert_eq!(results.len(), 0, "Entity should be deleted from Qdrant");

    // Cleanup
    drop(outbox);
    cleanup::cleanup_qdrant(&qdrant, &collection_name).await;
    cleanup::cleanup_postgres(&postgres).await;
}
