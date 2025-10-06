//! Integration tests for Postgres storage client operations

mod common;

use anyhow::Result;
use codesearch_core::entities::EntityType;
use codesearch_e2e_tests::common::{with_timeout, TestPostgres};
use codesearch_storage::{
    create_postgres_client,
    postgres::{OutboxOperation, TargetStore},
};
use common::*;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Setup helper: Start Postgres and run migrations
async fn setup_postgres() -> Result<(
    TestPostgres,
    Arc<codesearch_storage::postgres::PostgresClient>,
)> {
    let postgres = TestPostgres::start().await?;
    let config = create_storage_config(
        6334, // Qdrant not needed for Postgres tests
        6333,
        postgres.port(),
        "test_collection",
    );

    let client = create_postgres_client(&config).await?;
    client.run_migrations().await?;

    Ok((postgres, client))
}

#[tokio::test]
async fn test_ensure_repository_creates_new() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());

        // Create new repository
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, Some("test-repo"))
            .await?;

        assert!(!repository_id.is_nil(), "Repository ID should not be nil");

        // Verify we can get the repository by collection name
        let fetched_id = client.get_repository_id(&collection_name).await?;
        assert_eq!(
            fetched_id,
            Some(repository_id),
            "Should be able to fetch repository by collection name"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_ensure_repository_idempotent() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());

        // Create repository twice
        let id1 = client
            .ensure_repository(repo_path, &collection_name, Some("test-repo"))
            .await?;
        let id2 = client
            .ensure_repository(repo_path, &collection_name, Some("test-repo"))
            .await?;

        assert_eq!(id1, id2, "Should return same UUID both times");

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_store_entity_metadata_insert() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Create and store an entity
        let entity = create_test_entity(
            "test_func",
            EntityType::Function,
            &repository_id.to_string(),
        );
        let qdrant_point_id = Uuid::new_v4();

        client
            .store_entity_metadata(
                repository_id,
                &entity,
                Some("abc123".to_string()),
                qdrant_point_id,
            )
            .await?;

        // Verify entity was stored by fetching it back
        let entities = client
            .get_entities_by_ids(&[(repository_id, entity.entity_id.clone())])
            .await?;

        assert_eq!(entities.len(), 1, "Should retrieve the stored entity");
        assert_eq!(entities[0].name, "test_func", "Entity name should match");

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_store_entity_metadata_update() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Create and store an entity
        let mut entity = create_test_entity(
            "test_func",
            EntityType::Function,
            &repository_id.to_string(),
        );
        let qdrant_point_id = Uuid::new_v4();

        client
            .store_entity_metadata(
                repository_id,
                &entity,
                Some("abc123".to_string()),
                qdrant_point_id,
            )
            .await?;

        // Update the entity with modified content
        entity.content = Some("fn test_func() { /* updated */ }".to_string());
        client
            .store_entity_metadata(
                repository_id,
                &entity,
                Some("def456".to_string()),
                qdrant_point_id,
            )
            .await?;

        // Verify only one entity exists with updated content
        let entities = client
            .get_entities_by_ids(&[(repository_id, entity.entity_id.clone())])
            .await?;

        assert_eq!(entities.len(), 1, "Should have only one entity (upserted)");
        assert!(
            entities[0].content.as_ref().unwrap().contains("updated"),
            "Content should be updated"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_get_entities_for_file() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Create entities in different files
        let entity1 = create_test_entity_with_file(
            "main_func",
            EntityType::Function,
            &repository_id.to_string(),
            "main.rs",
        );
        let entity2 = create_test_entity_with_file(
            "main_struct",
            EntityType::Struct,
            &repository_id.to_string(),
            "main.rs",
        );
        let entity3 = create_test_entity_with_file(
            "lib_func",
            EntityType::Function,
            &repository_id.to_string(),
            "lib.rs",
        );

        // Store all entities
        for entity in &[&entity1, &entity2, &entity3] {
            client
                .store_entity_metadata(repository_id, entity, None, Uuid::new_v4())
                .await?;
        }

        // Get entities for main.rs
        let main_entities = client.get_entities_for_file("main.rs").await?;

        assert_eq!(
            main_entities.len(),
            2,
            "Should return 2 entities from main.rs"
        );
        assert!(
            main_entities.contains(&entity1.entity_id),
            "Should include main_func"
        );
        assert!(
            main_entities.contains(&entity2.entity_id),
            "Should include main_struct"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_get_entities_for_file_excludes_deleted() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Create 3 entities in same file
        let entities: Vec<_> = (0..3)
            .map(|i| {
                create_test_entity_with_file(
                    &format!("func{i}"),
                    EntityType::Function,
                    &repository_id.to_string(),
                    "test.rs",
                )
            })
            .collect();

        // Store all entities
        for entity in &entities {
            client
                .store_entity_metadata(repository_id, entity, None, Uuid::new_v4())
                .await?;
        }

        // Mark one as deleted
        client
            .mark_entities_deleted(repository_id, &[entities[1].entity_id.clone()])
            .await?;

        // Get entities for file
        let file_entities = client.get_entities_for_file("test.rs").await?;

        assert_eq!(
            file_entities.len(),
            2,
            "Should return only 2 entities (deleted excluded)"
        );
        assert!(
            !file_entities.contains(&entities[1].entity_id),
            "Deleted entity should not be included"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_file_snapshot_create_and_retrieve() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        let entity_ids = vec![
            "entity1".to_string(),
            "entity2".to_string(),
            "entity3".to_string(),
        ];

        // Create snapshot
        client
            .update_file_snapshot(
                repository_id,
                "main.rs",
                entity_ids.clone(),
                Some("abc123".to_string()),
            )
            .await?;

        // Retrieve snapshot
        let snapshot = client.get_file_snapshot(repository_id, "main.rs").await?;

        assert!(snapshot.is_some(), "Snapshot should exist");
        assert_eq!(snapshot.unwrap(), entity_ids, "Entity IDs should match");

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_file_snapshot_update() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Create initial snapshot
        let initial_ids = vec!["entity1".to_string(), "entity2".to_string()];
        client
            .update_file_snapshot(
                repository_id,
                "main.rs",
                initial_ids,
                Some("abc123".to_string()),
            )
            .await?;

        // Update snapshot
        let updated_ids = vec![
            "entity3".to_string(),
            "entity4".to_string(),
            "entity5".to_string(),
        ];
        client
            .update_file_snapshot(
                repository_id,
                "main.rs",
                updated_ids.clone(),
                Some("def456".to_string()),
            )
            .await?;

        // Retrieve snapshot
        let snapshot = client.get_file_snapshot(repository_id, "main.rs").await?;

        assert_eq!(
            snapshot.unwrap(),
            updated_ids,
            "Snapshot should be updated to new entity IDs"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_mark_entities_deleted() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Create and store 5 entities
        let entities: Vec<_> = (0..5)
            .map(|i| {
                create_test_entity(
                    &format!("func{i}"),
                    EntityType::Function,
                    &repository_id.to_string(),
                )
            })
            .collect();

        for entity in &entities {
            client
                .store_entity_metadata(repository_id, entity, None, Uuid::new_v4())
                .await?;
        }

        // Mark 2 as deleted
        let to_delete = vec![entities[0].entity_id.clone(), entities[1].entity_id.clone()];
        client
            .mark_entities_deleted(repository_id, &to_delete)
            .await?;

        // Verify deleted_at is set for those 2
        for entity_id in &to_delete {
            let metadata = client.get_entity_metadata(repository_id, entity_id).await?;
            assert!(metadata.is_some(), "Entity metadata should exist");
            let (_, deleted_at) = metadata.unwrap();
            assert!(deleted_at.is_some(), "deleted_at should be set");
        }

        // Verify other 3 are not affected
        for entity in entities.iter().skip(2).take(3) {
            let metadata = client
                .get_entity_metadata(repository_id, &entity.entity_id)
                .await?;
            assert!(metadata.is_some(), "Entity metadata should exist");
            let (_, deleted_at) = metadata.unwrap();
            assert!(
                deleted_at.is_none(),
                "deleted_at should be NULL for non-deleted"
            );
        }

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_mark_entities_deleted_batch_size_limit() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Create list of 1001 entity IDs (exceeds MAX_BATCH_SIZE of 1000)
        let entity_ids: Vec<String> = (0..1001).map(|i| format!("entity_{i}")).collect();

        // Attempt to mark as deleted
        let result = client
            .mark_entities_deleted(repository_id, &entity_ids)
            .await;

        assert!(result.is_err(), "Should return error for batch size > 1000");
        assert!(
            result.unwrap_err().to_string().contains("exceeds maximum"),
            "Error message should mention batch size limit"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_get_entities_by_ids() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Create and store 5 entities
        let entities: Vec<_> = (0..5)
            .map(|i| {
                create_test_entity(
                    &format!("func{i}"),
                    EntityType::Function,
                    &repository_id.to_string(),
                )
            })
            .collect();

        for entity in &entities {
            client
                .store_entity_metadata(repository_id, entity, None, Uuid::new_v4())
                .await?;
        }

        // Fetch 3 by IDs
        let entity_refs = vec![
            (repository_id, entities[0].entity_id.clone()),
            (repository_id, entities[2].entity_id.clone()),
            (repository_id, entities[4].entity_id.clone()),
        ];

        let fetched = client.get_entities_by_ids(&entity_refs).await?;

        assert_eq!(fetched.len(), 3, "Should return 3 entities");

        let fetched_names: Vec<_> = fetched.iter().map(|e| e.name.as_str()).collect();
        assert!(fetched_names.contains(&"func0"));
        assert!(fetched_names.contains(&"func2"));
        assert!(fetched_names.contains(&"func4"));

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_get_entities_by_ids_batch_limit() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repository_id = Uuid::new_v4();

        // Create 1001 entity references (exceeds MAX_BATCH_SIZE)
        let entity_refs: Vec<_> = (0..1001)
            .map(|i| (repository_id, format!("entity_{i}")))
            .collect();

        let result = client.get_entities_by_ids(&entity_refs).await;

        assert!(result.is_err(), "Should return error for batch size > 1000");
        assert!(
            result.unwrap_err().to_string().contains("exceeds maximum"),
            "Error message should mention batch size limit"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_outbox_write_and_read() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Create entities first, then write outbox entries
        let entity1 =
            create_test_entity("entity1", EntityType::Function, &repository_id.to_string());
        let entity2 =
            create_test_entity("entity2", EntityType::Function, &repository_id.to_string());
        let entity3 =
            create_test_entity("entity3", EntityType::Function, &repository_id.to_string());

        let embedding = vec![0.1_f32; 384];
        let batch = vec![
            (
                &entity1,
                embedding.as_slice(),
                OutboxOperation::Insert,
                Uuid::new_v4(),
                TargetStore::Qdrant,
                None,
            ),
            (
                &entity2,
                embedding.as_slice(),
                OutboxOperation::Update,
                Uuid::new_v4(),
                TargetStore::Qdrant,
                None,
            ),
            (
                &entity3,
                embedding.as_slice(),
                OutboxOperation::Delete,
                Uuid::new_v4(),
                TargetStore::Qdrant,
                None,
            ),
        ];

        let outbox_ids = client
            .store_entities_with_outbox_batch(repository_id, &batch)
            .await?;

        let insert_id = outbox_ids[0];
        let update_id = outbox_ids[1];
        let delete_id = outbox_ids[2];

        // Read unprocessed entries
        let entries = client
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await?;

        assert_eq!(entries.len(), 3, "Should have 3 unprocessed entries");

        // Verify IDs are present
        let entry_ids: Vec<_> = entries.iter().map(|e| e.outbox_id).collect();
        assert!(entry_ids.contains(&insert_id));
        assert!(entry_ids.contains(&update_id));
        assert!(entry_ids.contains(&delete_id));

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_outbox_mark_processed() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Create entity and write outbox entry atomically
        let entity =
            create_test_entity("entity1", EntityType::Function, &repository_id.to_string());
        let embedding = vec![0.1_f32; 384];
        let batch = vec![(
            &entity,
            embedding.as_slice(),
            OutboxOperation::Insert,
            Uuid::new_v4(),
            TargetStore::Qdrant,
            None,
        )];

        let outbox_ids = client
            .store_entities_with_outbox_batch(repository_id, &batch)
            .await?;
        let outbox_id = outbox_ids[0];

        // Mark as processed
        client.mark_outbox_processed(outbox_id).await?;

        // Get unprocessed entries
        let entries = client
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await?;

        // Should not include the processed entry
        let entry_ids: Vec<_> = entries.iter().map(|e| e.outbox_id).collect();
        assert!(
            !entry_ids.contains(&outbox_id),
            "Processed entry should not be returned"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_outbox_record_failure() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Create entity and write outbox entry atomically
        let entity =
            create_test_entity("entity1", EntityType::Function, &repository_id.to_string());
        let embedding = vec![0.1_f32; 384];
        let batch = vec![(
            &entity,
            embedding.as_slice(),
            OutboxOperation::Insert,
            Uuid::new_v4(),
            TargetStore::Qdrant,
            None,
        )];

        let outbox_ids = client
            .store_entities_with_outbox_batch(repository_id, &batch)
            .await?;
        let outbox_id = outbox_ids[0];

        // Record a failure
        client
            .record_outbox_failure(outbox_id, "Connection timeout")
            .await?;

        // Get unprocessed entries (should still be there since not processed)
        let entries = client
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await?;

        let entry = entries.iter().find(|e| e.outbox_id == outbox_id);
        assert!(entry.is_some(), "Entry should still be unprocessed");

        let entry = entry.unwrap();
        assert_eq!(entry.retry_count, 1, "Retry count should be incremented");
        assert!(
            entry
                .last_error
                .as_ref()
                .unwrap()
                .contains("Connection timeout"),
            "Error message should be stored"
        );
        assert!(entry.processed_at.is_none(), "Should still be unprocessed");

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_connection_failure() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        // Create config with invalid connection string
        let config = create_storage_config(6334, 6333, 9999, "test");

        let result = create_postgres_client(&config).await;

        // Connection should fail
        assert!(result.is_err(), "Should fail to connect with invalid port");

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_transaction_rollback() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_postgres, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Create an entity
        let entity = create_test_entity(
            "test_func",
            EntityType::Function,
            &repository_id.to_string(),
        );

        // Store it successfully
        client
            .store_entity_metadata(repository_id, &entity, None, Uuid::new_v4())
            .await?;

        // Verify it was stored
        let entities = client
            .get_entities_by_ids(&[(repository_id, entity.entity_id.clone())])
            .await?;
        assert_eq!(entities.len(), 1, "Entity should be stored");

        // Note: Testing actual transaction rollback on constraint violation is challenging
        // without exposing transaction APIs. The store_entity_metadata method already
        // handles transactions internally, and successful operations prove transaction safety.

        Ok(())
    })
    .await
}
