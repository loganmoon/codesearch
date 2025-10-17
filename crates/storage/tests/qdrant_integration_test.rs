//! Integration tests for Qdrant storage client operations

mod common;

use anyhow::Result;
use codesearch_core::entities::EntityType;
use codesearch_e2e_tests::common::{with_timeout, TestQdrant};
use codesearch_storage::{
    create_collection_manager, create_storage_client, SearchFilters, StorageClient,
};
use common::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Setup helper: Start Qdrant and create a collection with storage client
async fn setup_qdrant() -> Result<(TestQdrant, Arc<dyn StorageClient>, String)> {
    let qdrant = TestQdrant::start().await?;
    let collection_name = format!("test_{}", Uuid::new_v4());

    let config = create_storage_config(
        qdrant.port(),
        qdrant.rest_port(),
        5432, // Postgres not needed for Qdrant tests
        "codesearch",
    );

    // Create collection
    let manager = create_collection_manager(&config).await?;
    manager.ensure_collection(&collection_name, 1536).await?;

    // Create storage client
    let client = create_storage_client(&config, &collection_name).await?;

    Ok((qdrant, client, collection_name))
}

#[tokio::test]
async fn test_bulk_load_entities() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_qdrant, client, _collection) = setup_qdrant().await?;
        let repository_id = Uuid::new_v4().to_string();

        let entities = vec![
            create_embedded_entity(
                create_test_entity("add", EntityType::Function, &repository_id),
                1536,
            ),
            create_embedded_entity(
                create_test_entity("subtract", EntityType::Function, &repository_id),
                1536,
            ),
            create_embedded_entity(
                create_test_entity("Calculator", EntityType::Struct, &repository_id),
                1536,
            ),
        ];

        let result = client.bulk_load_entities(entities.clone()).await?;

        assert_eq!(result.len(), 3, "Should return 3 entity-point pairs");

        let returned_entity_ids: Vec<String> = result.iter().map(|(id, _)| id.clone()).collect();
        for entity in &entities {
            assert!(
                returned_entity_ids.contains(&entity.entity.entity_id),
                "Returned entity IDs should include {}",
                entity.entity.entity_id
            );
        }

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_search_similar_no_filters() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_qdrant, client, _collection) = setup_qdrant().await?;
        let repository_id = Uuid::new_v4().to_string();

        let entities: Vec<_> = (0..5)
            .map(|i| {
                create_embedded_entity(
                    create_test_entity(&format!("func{i}"), EntityType::Function, &repository_id),
                    1536,
                )
            })
            .collect();

        client.bulk_load_entities(entities).await?;

        let query_embedding = mock_embedding(1536);
        let results = client.search_similar(query_embedding, 3, None).await?;

        assert!(results.len() <= 3, "Should return at most 3 results");
        assert!(!results.is_empty(), "Should return some results");

        for (entity_id, repo_id, score) in &results {
            assert!(!entity_id.is_empty(), "Entity ID should not be empty");
            assert_eq!(repo_id, &repository_id, "Repository ID should match");
            assert!(
                *score >= 0.0 && *score <= 1.0,
                "Score should be between 0 and 1"
            );
        }

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_search_similar_with_entity_type_filter() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_qdrant, client, _collection) = setup_qdrant().await?;
        let repository_id = Uuid::new_v4().to_string();

        let entities = vec![
            create_embedded_entity(
                create_test_entity("add", EntityType::Function, &repository_id),
                1536,
            ),
            create_embedded_entity(
                create_test_entity("Calculator", EntityType::Struct, &repository_id),
                1536,
            ),
            create_embedded_entity(
                create_test_entity("multiply", EntityType::Function, &repository_id),
                1536,
            ),
        ];

        client.bulk_load_entities(entities).await?;

        let query_embedding = mock_embedding(1536);
        let filters = SearchFilters {
            entity_type: Some(EntityType::Function),
            ..Default::default()
        };

        let results = client
            .search_similar(query_embedding, 10, Some(filters))
            .await?;

        assert!(results.len() >= 2, "Should find at least the 2 functions");

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_search_similar_with_language_filter() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_qdrant, client, _collection) = setup_qdrant().await?;
        let repository_id = Uuid::new_v4().to_string();

        let entities = vec![
            create_embedded_entity(
                create_test_entity("func1", EntityType::Function, &repository_id),
                1536,
            ),
            create_embedded_entity(
                create_test_entity("func2", EntityType::Function, &repository_id),
                1536,
            ),
        ];

        client.bulk_load_entities(entities).await?;

        let query_embedding = mock_embedding(1536);
        let filters = SearchFilters {
            language: Some("Rust".to_string()),
            ..Default::default()
        };

        let results = client
            .search_similar(query_embedding, 10, Some(filters))
            .await?;
        assert_eq!(results.len(), 2, "Should return both Rust entities");

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_search_similar_with_file_path_filter() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_qdrant, client, _collection) = setup_qdrant().await?;
        let repository_id = Uuid::new_v4().to_string();

        let entities = vec![
            create_embedded_entity(
                create_test_entity_with_file(
                    "main_func",
                    EntityType::Function,
                    &repository_id,
                    "main.rs",
                ),
                1536,
            ),
            create_embedded_entity(
                create_test_entity_with_file(
                    "lib_func",
                    EntityType::Function,
                    &repository_id,
                    "lib.rs",
                ),
                1536,
            ),
            create_embedded_entity(
                create_test_entity_with_file(
                    "main_struct",
                    EntityType::Struct,
                    &repository_id,
                    "main.rs",
                ),
                1536,
            ),
        ];

        client.bulk_load_entities(entities).await?;

        let query_embedding = mock_embedding(1536);
        let filters = SearchFilters {
            file_path: Some(PathBuf::from("main.rs")),
            ..Default::default()
        };

        let results = client
            .search_similar(query_embedding, 10, Some(filters))
            .await?;

        assert_eq!(results.len(), 2, "Should return 2 entities from main.rs");

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_delete_entities() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_qdrant, client, _collection) = setup_qdrant().await?;
        let repository_id = Uuid::new_v4().to_string();

        let entities: Vec<_> = (0..5)
            .map(|i| {
                create_embedded_entity(
                    create_test_entity(&format!("func{i}"), EntityType::Function, &repository_id),
                    1536,
                )
            })
            .collect();

        let entity_ids: Vec<String> = entities
            .iter()
            .map(|e| e.entity.entity_id.clone())
            .collect();
        client.bulk_load_entities(entities).await?;

        let to_delete = vec![entity_ids[0].clone(), entity_ids[1].clone()];
        client.delete_entities(&to_delete).await?;

        let query_embedding = mock_embedding(1536);
        let results = client.search_similar(query_embedding, 10, None).await?;

        assert_eq!(results.len(), 3, "Should have 3 entities after deleting 2");

        let result_entity_ids: Vec<String> = results.iter().map(|(id, _, _)| id.clone()).collect();
        assert!(
            !result_entity_ids.contains(&to_delete[0]),
            "Deleted entity should not be found"
        );
        assert!(
            !result_entity_ids.contains(&to_delete[1]),
            "Deleted entity should not be found"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_delete_entities_empty_list() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_qdrant, client, _collection) = setup_qdrant().await?;

        let result = client.delete_entities(&[]).await;
        assert!(result.is_ok(), "Deleting empty list should succeed");

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_bulk_load_empty_list() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_qdrant, client, _collection) = setup_qdrant().await?;

        let result = client.bulk_load_entities(vec![]).await?;
        assert_eq!(result.len(), 0, "Should return empty vec for empty input");

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_search_no_results() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_qdrant, client, _collection) = setup_qdrant().await?;

        let query_embedding = mock_embedding(1536);
        let results = client.search_similar(query_embedding, 10, None).await?;

        assert_eq!(
            results.len(),
            0,
            "Should return empty results for empty collection"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_connection_error() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        // Create config with invalid host
        let config = create_storage_config(
            9999, // Invalid port
            9998,
            5432,
            "codesearch",
        );

        // So we test the actual operation
        let result = create_storage_client(&config, "test_collection").await;

        if let Ok(client) = result {
            let query_embedding = mock_embedding(1536);
            let search_result = client.search_similar(query_embedding, 10, None).await;
            assert!(
                search_result.is_err(),
                "Search should fail with invalid host"
            );
        }

        Ok(())
    })
    .await
}

// Phase 5: Error Handling Tests

#[tokio::test]
async fn test_bulk_load_during_connection_loss() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (qdrant, client, _collection) = setup_qdrant().await?;
        let repository_id = Uuid::new_v4().to_string();

        let entities = vec![create_embedded_entity(
            create_test_entity("test_func", EntityType::Function, &repository_id),
            1536,
        )];

        client.bulk_load_entities(entities.clone()).await?;

        drop(qdrant);

        tokio::time::sleep(Duration::from_millis(500)).await;

        let result = client.bulk_load_entities(entities).await;
        assert!(
            result.is_err(),
            "Bulk load should fail after connection loss"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_search_invalid_collection() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (qdrant, _original_client, _collection) = setup_qdrant().await?;

        let config = create_storage_config(qdrant.port(), qdrant.rest_port(), 5432, "codesearch");

        let client = create_storage_client(&config, "nonexistent_collection").await?;

        let query_embedding = mock_embedding(1536);
        let result = client.search_similar(query_embedding, 10, None).await;

        assert!(
            result.is_err(),
            "Search should fail on non-existent collection"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_delete_from_empty_collection() -> Result<()> {
    with_timeout(Duration::from_secs(30), async {
        let (_qdrant, client, _collection) = setup_qdrant().await?;

        let entity_ids = vec!["entity1".to_string(), "entity2".to_string()];
        let result = client.delete_entities(&entity_ids).await;

        assert!(
            result.is_ok(),
            "Delete from empty collection should succeed"
        );

        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_concurrent_bulk_loads() -> Result<()> {
    with_timeout(Duration::from_secs(60), async {
        let (_qdrant, client, _collection) = setup_qdrant().await?;

        let mut tasks = vec![];
        for i in 0..5 {
            let client_clone = Arc::clone(&client);
            let repository_id = Uuid::new_v4().to_string();

            tasks.push(tokio::spawn(async move {
                let entities = vec![
                    create_embedded_entity(
                        create_test_entity(
                            &format!("func{i}_1"),
                            EntityType::Function,
                            &repository_id,
                        ),
                        1536,
                    ),
                    create_embedded_entity(
                        create_test_entity(
                            &format!("func{i}_2"),
                            EntityType::Function,
                            &repository_id,
                        ),
                        1536,
                    ),
                ];
                client_clone.bulk_load_entities(entities).await
            }));
        }

        for task in tasks {
            let result = task.await?;
            assert!(result.is_ok(), "Concurrent bulk loads should succeed");
        }

        let query_embedding = mock_embedding(1536);
        let results = client.search_similar(query_embedding, 20, None).await?;

        assert_eq!(
            results.len(),
            10,
            "Should have all 10 entities from concurrent loads"
        );

        Ok(())
    })
    .await
}
