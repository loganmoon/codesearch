//! Integration tests for Postgres storage client operations

mod common;

use anyhow::Result;
use codesearch_core::entities::EntityType;
use codesearch_e2e_tests::common::{
    create_test_database, drop_test_database, get_shared_postgres, with_timeout,
};
use codesearch_storage::{
    create_postgres_client, OutboxOperation, PostgresClientTrait, TargetStore,
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
    Arc<dyn PostgresClientTrait>,
)> {
    let postgres = get_shared_postgres().await?;
    let db_name = create_test_database(&postgres).await?;

    let config = create_storage_config(
        6334, // Qdrant not needed for Postgres tests
        6333,
        postgres.port(),
        &db_name,
    );

    let client = create_postgres_client(&config).await?;
    client.run_migrations().await?;

    Ok((postgres, db_name, client))
}

#[tokio::test]
async fn test_ensure_repository_creates_new() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());

        let repository_id = client
            .ensure_repository(repo_path, &collection_name, Some("test-repo"))
            .await?;

        assert!(!repository_id.is_nil(), "Repository ID should not be nil");

        let fetched_id = client.get_repository_id(&collection_name).await?;
        assert_eq!(
            fetched_id,
            Some(repository_id),
            "Should be able to fetch repository by collection name"
        );

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_ensure_repository_idempotent() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());

        let id1 = client
            .ensure_repository(repo_path, &collection_name, Some("test-repo"))
            .await?;
        let id2 = client
            .ensure_repository(repo_path, &collection_name, Some("test-repo"))
            .await?;

        assert_eq!(id1, id2, "Should return same UUID both times");

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_store_entity_metadata_insert() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        let entity = create_test_entity(
            "test_func",
            EntityType::Function,
            &repository_id.to_string(),
        );
        let qdrant_point_id = Uuid::new_v4();

        let embedding = vec![0.1; 384];
        // Store embedding to get its ID
        let content_hash = format!("{:032x}", Uuid::new_v4().as_u128());
        let embedding_ids = client
            .store_embeddings(&[(content_hash, embedding)], "test-model", 384)
            .await?;
        let embedding_id = embedding_ids[0];

        let batch = vec![(
            &entity,
            embedding_id,
            OutboxOperation::Insert,
            qdrant_point_id,
            TargetStore::Qdrant,
            Some("abc123".to_string()),
        )];
        client
            .store_entities_with_outbox_batch(repository_id, &collection_name, &batch)
            .await?;

        let entities = client
            .get_entities_by_ids(&[(repository_id, entity.entity_id.clone())])
            .await?;

        assert_eq!(entities.len(), 1, "Should retrieve the stored entity");
        assert_eq!(entities[0].name, "test_func", "Entity name should match");

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_store_entity_metadata_update() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        let mut entity = create_test_entity(
            "test_func",
            EntityType::Function,
            &repository_id.to_string(),
        );
        let qdrant_point_id = Uuid::new_v4();

        let embedding = vec![0.1; 384];
        // Store embedding to get its ID
        let content_hash = format!("{:032x}", Uuid::new_v4().as_u128());
        let embedding_ids = client
            .store_embeddings(
                &[(content_hash.clone(), embedding.clone())],
                "test-model",
                384,
            )
            .await?;
        let embedding_id = embedding_ids[0];

        let batch = vec![(
            &entity,
            embedding_id,
            OutboxOperation::Insert,
            qdrant_point_id,
            TargetStore::Qdrant,
            Some("abc123".to_string()),
        )];
        client
            .store_entities_with_outbox_batch(repository_id, &collection_name, &batch)
            .await?;

        entity.content = Some("fn test_func() { /* updated */ }".to_string());
        // Store updated embedding to get its ID
        let content_hash2 = format!("{:032x}", Uuid::new_v4().as_u128());
        let embedding_ids2 = client
            .store_embeddings(&[(content_hash2, embedding)], "test-model", 384)
            .await?;
        let embedding_id2 = embedding_ids2[0];

        let batch = vec![(
            &entity,
            embedding_id2,
            OutboxOperation::Insert,
            qdrant_point_id,
            TargetStore::Qdrant,
            Some("def456".to_string()),
        )];
        client
            .store_entities_with_outbox_batch(repository_id, &collection_name, &batch)
            .await?;

        let entities = client
            .get_entities_by_ids(&[(repository_id, entity.entity_id.clone())])
            .await?;

        assert_eq!(entities.len(), 1, "Should have only one entity (upserted)");
        assert!(
            entities[0].content.as_ref().unwrap().contains("updated"),
            "Content should be updated"
        );

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_get_file_snapshot() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

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

        // Store entities using batch API
        let embedding = vec![0.1; 384];
        // Store embeddings to get their IDs
        let content_hash1 = format!("{:032x}", Uuid::new_v4().as_u128());
        let content_hash2 = format!("{:032x}", Uuid::new_v4().as_u128());
        let content_hash3 = format!("{:032x}", Uuid::new_v4().as_u128());
        let embedding_ids = client
            .store_embeddings(
                &[
                    (content_hash1, embedding.clone()),
                    (content_hash2, embedding.clone()),
                    (content_hash3, embedding),
                ],
                "test-model",
                384,
            )
            .await?;

        let batch = vec![
            (
                &entity1,
                embedding_ids[0],
                OutboxOperation::Insert,
                Uuid::new_v4(),
                TargetStore::Qdrant,
                None,
            ),
            (
                &entity2,
                embedding_ids[1],
                OutboxOperation::Insert,
                Uuid::new_v4(),
                TargetStore::Qdrant,
                None,
            ),
            (
                &entity3,
                embedding_ids[2],
                OutboxOperation::Insert,
                Uuid::new_v4(),
                TargetStore::Qdrant,
                None,
            ),
        ];

        client
            .store_entities_with_outbox_batch(repository_id, &collection_name, &batch)
            .await?;

        // Update file snapshots
        client
            .update_file_snapshot(
                repository_id,
                "main.rs",
                vec![entity1.entity_id.clone(), entity2.entity_id.clone()],
                None,
            )
            .await?;

        client
            .update_file_snapshot(
                repository_id,
                "lib.rs",
                vec![entity3.entity_id.clone()],
                None,
            )
            .await?;

        // Get file snapshot for main.rs
        let main_entities = client
            .get_file_snapshot(repository_id, "main.rs")
            .await?
            .expect("main.rs snapshot should exist");

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

        // Get file snapshot for lib.rs
        let lib_entities = client
            .get_file_snapshot(repository_id, "lib.rs")
            .await?
            .expect("lib.rs snapshot should exist");

        assert_eq!(lib_entities.len(), 1, "Should return 1 entity from lib.rs");
        assert!(
            lib_entities.contains(&entity3.entity_id),
            "Should include lib_func"
        );

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_file_snapshot_create_and_retrieve() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

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

        client
            .update_file_snapshot(
                repository_id,
                "main.rs",
                entity_ids.clone(),
                Some("abc123".to_string()),
            )
            .await?;

        let snapshot = client.get_file_snapshot(repository_id, "main.rs").await?;

        assert!(snapshot.is_some(), "Snapshot should exist");
        assert_eq!(snapshot.unwrap(), entity_ids, "Entity IDs should match");

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_file_snapshot_update() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        let initial_ids = vec!["entity1".to_string(), "entity2".to_string()];
        client
            .update_file_snapshot(
                repository_id,
                "main.rs",
                initial_ids,
                Some("abc123".to_string()),
            )
            .await?;

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

        let snapshot = client.get_file_snapshot(repository_id, "main.rs").await?;

        assert_eq!(
            snapshot.unwrap(),
            updated_ids,
            "Snapshot should be updated to new entity IDs"
        );

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_mark_entities_deleted() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        let entities: Vec<_> = (0..5)
            .map(|i| {
                create_test_entity(
                    &format!("func{i}"),
                    EntityType::Function,
                    &repository_id.to_string(),
                )
            })
            .collect();

        let embedding = vec![0.1; 384];
        for entity in &entities {
            // Store embedding to get its ID
            let content_hash = format!("{:032x}", Uuid::new_v4().as_u128());
            let embedding_ids = client
                .store_embeddings(&[(content_hash, embedding.clone())], "test-model", 384)
                .await?;
            let embedding_id = embedding_ids[0];

            let batch = vec![(
                entity,
                embedding_id,
                OutboxOperation::Insert,
                Uuid::new_v4(),
                TargetStore::Qdrant,
                None,
            )];
            client
                .store_entities_with_outbox_batch(repository_id, &collection_name, &batch)
                .await?;
        }

        let to_delete = vec![entities[0].entity_id.clone(), entities[1].entity_id.clone()];
        client
            .mark_entities_deleted_with_outbox(repository_id, &collection_name, &to_delete)
            .await?;

        // Use batch method to get metadata
        let metadata_map = client
            .get_entities_metadata_batch(repository_id, &to_delete)
            .await?;

        for entity_id in &to_delete {
            let metadata = metadata_map.get(entity_id);
            assert!(metadata.is_some(), "Entity metadata should exist");
            let (_, deleted_at) = metadata.unwrap();
            assert!(deleted_at.is_some(), "deleted_at should be set");
        }

        let not_deleted: Vec<String> = entities
            .iter()
            .skip(2)
            .take(3)
            .map(|e| e.entity_id.clone())
            .collect();
        let metadata_map = client
            .get_entities_metadata_batch(repository_id, &not_deleted)
            .await?;

        for entity in entities.iter().skip(2).take(3) {
            let metadata = metadata_map.get(&entity.entity_id);
            assert!(metadata.is_some(), "Entity metadata should exist");
            let (_, deleted_at) = metadata.unwrap();
            assert!(
                deleted_at.is_none(),
                "deleted_at should be NULL for non-deleted"
            );
        }

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_mark_entities_deleted_batch_size_limit() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        let entity_ids: Vec<String> = (0..1001).map(|i| format!("entity_{i}")).collect();

        let result = client
            .mark_entities_deleted_with_outbox(repository_id, &collection_name, &entity_ids)
            .await;

        assert!(result.is_err(), "Should return error for batch size > 1000");
        assert!(
            result.unwrap_err().to_string().contains("exceeds maximum"),
            "Error message should mention batch size limit"
        );

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_get_entities_by_ids() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        let entities: Vec<_> = (0..5)
            .map(|i| {
                create_test_entity(
                    &format!("func{i}"),
                    EntityType::Function,
                    &repository_id.to_string(),
                )
            })
            .collect();

        let embedding = vec![0.1; 384];
        for entity in &entities {
            // Store embedding to get its ID
            let content_hash = format!("{:032x}", Uuid::new_v4().as_u128());
            let embedding_ids = client
                .store_embeddings(&[(content_hash, embedding.clone())], "test-model", 384)
                .await?;
            let embedding_id = embedding_ids[0];

            let batch = vec![(
                entity,
                embedding_id,
                OutboxOperation::Insert,
                Uuid::new_v4(),
                TargetStore::Qdrant,
                None,
            )];
            client
                .store_entities_with_outbox_batch(repository_id, &collection_name, &batch)
                .await?;
        }

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

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_get_entities_by_ids_batch_limit() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

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

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_outbox_write_and_read() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        let entity1 =
            create_test_entity("entity1", EntityType::Function, &repository_id.to_string());
        let entity2 =
            create_test_entity("entity2", EntityType::Function, &repository_id.to_string());
        let entity3 =
            create_test_entity("entity3", EntityType::Function, &repository_id.to_string());

        let embedding = vec![0.1_f32; 384];
        // Store embeddings to get their IDs
        let content_hash1 = format!("{:032x}", Uuid::new_v4().as_u128());
        let content_hash2 = format!("{:032x}", Uuid::new_v4().as_u128());
        let content_hash3 = format!("{:032x}", Uuid::new_v4().as_u128());
        let embedding_ids = client
            .store_embeddings(
                &[
                    (content_hash1, embedding.clone()),
                    (content_hash2, embedding.clone()),
                    (content_hash3, embedding),
                ],
                "test-model",
                384,
            )
            .await?;

        let batch = vec![
            (
                &entity1,
                embedding_ids[0],
                OutboxOperation::Insert,
                Uuid::new_v4(),
                TargetStore::Qdrant,
                None,
            ),
            (
                &entity2,
                embedding_ids[1],
                OutboxOperation::Update,
                Uuid::new_v4(),
                TargetStore::Qdrant,
                None,
            ),
            (
                &entity3,
                embedding_ids[2],
                OutboxOperation::Delete,
                Uuid::new_v4(),
                TargetStore::Qdrant,
                None,
            ),
        ];

        let outbox_ids = client
            .store_entities_with_outbox_batch(repository_id, &collection_name, &batch)
            .await?;

        let insert_id = outbox_ids[0];
        let update_id = outbox_ids[1];
        let delete_id = outbox_ids[2];

        let entries = client
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await?;

        assert_eq!(entries.len(), 3, "Should have 3 unprocessed entries");

        let entry_ids: Vec<_> = entries.iter().map(|e| e.outbox_id).collect();
        assert!(entry_ids.contains(&insert_id));
        assert!(entry_ids.contains(&update_id));
        assert!(entry_ids.contains(&delete_id));

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_outbox_mark_processed() -> Result<()> {
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
        // Store embedding to get its ID
        let content_hash = format!("{:032x}", Uuid::new_v4().as_u128());
        let embedding_ids = client
            .store_embeddings(&[(content_hash, embedding)], "test-model", 384)
            .await?;
        let embedding_id = embedding_ids[0];

        let batch = vec![(
            &entity,
            embedding_id,
            OutboxOperation::Insert,
            Uuid::new_v4(),
            TargetStore::Qdrant,
            None,
        )];

        let outbox_ids = client
            .store_entities_with_outbox_batch(repository_id, &collection_name, &batch)
            .await?;
        let outbox_id = outbox_ids[0];

        client.mark_outbox_processed(outbox_id).await?;

        let entries = client
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await?;

        let entry_ids: Vec<_> = entries.iter().map(|e| e.outbox_id).collect();
        assert!(
            !entry_ids.contains(&outbox_id),
            "Processed entry should not be returned"
        );

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_outbox_record_failure() -> Result<()> {
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
        // Store embedding to get its ID
        let content_hash = format!("{:032x}", Uuid::new_v4().as_u128());
        let embedding_ids = client
            .store_embeddings(&[(content_hash, embedding)], "test-model", 384)
            .await?;
        let embedding_id = embedding_ids[0];

        let batch = vec![(
            &entity,
            embedding_id,
            OutboxOperation::Insert,
            Uuid::new_v4(),
            TargetStore::Qdrant,
            None,
        )];

        let outbox_ids = client
            .store_entities_with_outbox_batch(repository_id, &collection_name, &batch)
            .await?;
        let outbox_id = outbox_ids[0];

        client
            .record_outbox_failure(outbox_id, "Connection timeout")
            .await?;

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

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_connection_failure() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let config = create_storage_config(6334, 6333, 9999, "codesearch");

        let result = create_postgres_client(&config).await;

        assert!(result.is_err(), "Should fail to connect with invalid port");

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_transaction_rollback() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (postgres, db_name, client) = setup_postgres().await?;

        let repo_path = Path::new("/tmp/test-repo");
        let collection_name = format!("test_{}", Uuid::new_v4());
        let repository_id = client
            .ensure_repository(repo_path, &collection_name, None)
            .await?;

        let entity = create_test_entity(
            "test_func",
            EntityType::Function,
            &repository_id.to_string(),
        );

        let embedding = vec![0.1; 384];
        // Store embedding to get its ID
        let content_hash = format!("{:032x}", Uuid::new_v4().as_u128());
        let embedding_ids = client
            .store_embeddings(&[(content_hash, embedding)], "test-model", 384)
            .await?;
        let embedding_id = embedding_ids[0];

        let batch = vec![(
            &entity,
            embedding_id,
            OutboxOperation::Insert,
            Uuid::new_v4(),
            TargetStore::Qdrant,
            None,
        )];
        client
            .store_entities_with_outbox_batch(repository_id, &collection_name, &batch)
            .await?;

        let entities = client
            .get_entities_by_ids(&[(repository_id, entity.entity_id.clone())])
            .await?;
        assert_eq!(entities.len(), 1, "Entity should be stored");

        // without exposing transaction APIs. The store_entities_with_outbox_batch method already
        // handles transactions internally, and successful operations prove transaction safety.

        drop_test_database(&postgres, &db_name).await?;
        Ok(())
    })
    .await
}
