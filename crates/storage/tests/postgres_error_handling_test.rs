//! Error handling tests for Postgres storage client

mod common;

use anyhow::Result;
use codesearch_core::entities::EntityType;
use codesearch_e2e_tests::common::{
    create_test_database, drop_test_database, get_shared_postgres, with_timeout,
};
use codesearch_storage::{
    create_postgres_client,
    postgres::{OutboxOperation, TargetStore},
};
use common::*;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Setup helper: Use shared Postgres instance and create unique database
async fn setup_postgres() -> Result<(
    Arc<codesearch_e2e_tests::common::TestPostgres>,
    String,
    Arc<codesearch_storage::postgres::PostgresClient>,
)> {
    let postgres = get_shared_postgres().await?;
    let db_name = create_test_database(&postgres).await?;

    let config = create_storage_config(
        6334, // Qdrant not needed for Postgres tests
        6333,
        postgres.port(),
        "test_collection",
        &db_name,
    );

    let client = create_postgres_client(&config).await?;
    client.run_migrations().await?;

    Ok((postgres, db_name, client))
}

#[tokio::test]
async fn test_connection_pool_exhaustion() -> Result<()> {
    with_timeout(Duration::from_secs(60), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Note: Connection pool exhaustion testing is difficult without low-level pool access
        // We can verify concurrent operations complete successfully
        let mut tasks = vec![];

        for i in 0..10 {
            let client_clone = Arc::clone(&client);
            let repo_id = repository_id;
            tasks.push(tokio::spawn(async move {
                let entity = create_test_entity(
                    &format!("concurrent_{i}"),
                    EntityType::Function,
                    &repo_id.to_string(),
                );
                client_clone
                    .store_entity_metadata(repo_id, &entity, None, Uuid::new_v4())
                    .await
            }));
        }

        // All tasks should complete successfully
        for task in tasks {
            let result = task.await?;
            assert!(result.is_ok(), "Concurrent operations should succeed");
        }

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_store_entity_during_connection_loss() -> Result<()> {
    with_timeout(Duration::from_secs(60), async {
        let config = create_storage_config(6334, 6333, 9999, "test", "codesearch");
        let result = create_postgres_client(&config).await;

        assert!(
            result.is_err(),
            "Should fail to connect to invalid Postgres port"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_concurrent_writes_same_entity() -> Result<()> {
    with_timeout(Duration::from_secs(60), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        let entity_id = format!("concurrent_entity_{}", Uuid::new_v4());
        let entity1 = {
            let mut e =
                create_test_entity("func1", EntityType::Function, &repository_id.to_string());
            e.entity_id = entity_id.clone();
            e.content = Some("Version 1".to_string());
            e
        };

        let entity2 = {
            let mut e =
                create_test_entity("func1", EntityType::Function, &repository_id.to_string());
            e.entity_id = entity_id.clone();
            e.content = Some("Version 2".to_string());
            e
        };

        // Spawn concurrent updates
        let client1 = Arc::clone(&client);
        let client2 = Arc::clone(&client);

        let task1 = tokio::spawn(async move {
            client1
                .store_entity_metadata(repository_id, &entity1, None, Uuid::new_v4())
                .await
        });

        let task2 = tokio::spawn(async move {
            client2
                .store_entity_metadata(repository_id, &entity2, None, Uuid::new_v4())
                .await
        });

        // Both should succeed (last write wins)
        let result1 = task1.await?;
        let result2 = task2.await?;

        assert!(result1.is_ok(), "First concurrent write should succeed");
        assert!(result2.is_ok(), "Second concurrent write should succeed");

        let entities = client
            .get_entities_by_ids(&[(repository_id, entity_id)])
            .await?;
        assert_eq!(
            entities.len(),
            1,
            "Should have exactly one entity (upserted)"
        );

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_concurrent_snapshot_updates() -> Result<()> {
    with_timeout(Duration::from_secs(60), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Spawn concurrent snapshot updates for the same file
        let mut tasks = vec![];
        for i in 0..3 {
            let client_clone = Arc::clone(&client);
            let entity_ids = vec![
                format!("entity_{i}_1"),
                format!("entity_{i}_2"),
                format!("entity_{i}_3"),
            ];
            tasks.push(tokio::spawn(async move {
                client_clone
                    .update_file_snapshot(
                        repository_id,
                        "main.rs",
                        entity_ids,
                        Some("commit".to_string()),
                    )
                    .await
            }));
        }

        // All should succeed (last write wins)
        for task in tasks {
            let result = task.await?;
            assert!(result.is_ok(), "Concurrent snapshot updates should succeed");
        }

        let snapshot = client.get_file_snapshot(repository_id, "main.rs").await?;
        assert!(
            snapshot.is_some(),
            "Snapshot should exist after concurrent updates"
        );

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_mark_deleted_nonexistent_entities() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Try to delete non-existent entities
        let non_existent_ids = vec![
            Uuid::new_v4().to_string(),
            Uuid::new_v4().to_string(),
            Uuid::new_v4().to_string(),
        ];

        let result = client
            .mark_entities_deleted(repository_id, &non_existent_ids)
            .await;

        // Should succeed with 0 rows affected (no error)
        assert!(
            result.is_ok(),
            "Deleting non-existent entities should not error"
        );

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_get_entities_by_ids_some_missing() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        let entities: Vec<_> = (0..3)
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

        // Request 5 entity IDs (3 exist, 2 don't)
        let entity_refs = vec![
            (repository_id, entities[0].entity_id.clone()),
            (repository_id, entities[1].entity_id.clone()),
            (repository_id, entities[2].entity_id.clone()),
            (repository_id, Uuid::new_v4().to_string()), // Non-existent
            (repository_id, Uuid::new_v4().to_string()), // Non-existent
        ];

        let fetched = client.get_entities_by_ids(&entity_refs).await?;

        // Should return only the 3 that exist
        assert_eq!(
            fetched.len(),
            3,
            "Should return only existing entities (no error for missing)"
        );

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_outbox_concurrent_writes() -> Result<()> {
    with_timeout(Duration::from_secs(60), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        // Spawn 10 concurrent entity batch writes (each with size 1)
        let mut tasks = vec![];
        for i in 0..10 {
            let client_clone = Arc::clone(&client);
            tasks.push(tokio::spawn(async move {
                let entity = create_test_entity(
                    &format!("concurrent_{i}"),
                    EntityType::Function,
                    &repository_id.to_string(),
                );
                let embedding = vec![0.1_f32; 384];

                let batch = vec![(
                    &entity,
                    embedding.as_slice(),
                    OutboxOperation::Insert,
                    Uuid::new_v4(),
                    TargetStore::Qdrant,
                    None,
                )];

                client_clone
                    .store_entities_with_outbox_batch(repository_id, &batch)
                    .await
            }));
        }

        // Collect all outbox IDs
        let mut outbox_ids = vec![];
        for task in tasks {
            let result = task.await?;
            assert!(result.is_ok(), "Concurrent outbox writes should succeed");
            outbox_ids.extend(result?);
        }

        // All IDs should be unique
        assert_eq!(outbox_ids.len(), 10, "Should have 10 outbox entries");
        let unique_ids: std::collections::HashSet<_> = outbox_ids.iter().collect();
        assert_eq!(unique_ids.len(), 10, "All outbox IDs should be unique");

        let entries = client
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 20)
            .await?;
        assert_eq!(entries.len(), 10, "Should retrieve all 10 entries");

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_outbox_mark_processed_twice() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

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

        // Mark as processed again (should be idempotent)
        let result = client.mark_outbox_processed(outbox_id).await;
        assert!(
            result.is_ok(),
            "Marking processed twice should be idempotent"
        );

        let entries = client
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await?;
        let entry_ids: Vec<_> = entries.iter().map(|e| e.outbox_id).collect();
        assert!(
            !entry_ids.contains(&outbox_id),
            "Entry should still be marked processed"
        );

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_migration_already_applied() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        // Migrations already run in setup
        // Run them again - should be idempotent
        let result = client.run_migrations().await;
        assert!(result.is_ok(), "Re-running migrations should be idempotent");

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        assert!(!repository_id.is_nil(), "Schema should still be functional");

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}
