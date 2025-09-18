//! Integration tests for Qdrant storage backend
//!
//! These tests require a running Qdrant instance.
//! Run with: cargo test --package codesearch-storage -- --ignored

use codesearch_core::{
    config::StorageConfig,
    entities::{CodeEntityBuilder, Language, SourceLocation},
    CodeEntity, EntityType,
};
use codesearch_storage::create_storage_client;
use std::path::PathBuf;

/// Helper to check if Qdrant is available
async fn qdrant_available() -> bool {
    let config = StorageConfig {
        provider: "qdrant".to_string(),
        host: "localhost".to_string(),
        port: 6334,
        ..Default::default()
    };

    create_storage_client(config).await.is_ok()
}

/// Helper to create test entities
fn create_test_entities(count: usize) -> Vec<CodeEntity> {
    (0..count)
        .map(|i| {
            CodeEntityBuilder::default()
                .entity_id(format!("entity_{i}"))
                .name(format!("function_{i}"))
                .qualified_name(format!("module::function_{i}"))
                .entity_type(EntityType::Function)
                .file_path(PathBuf::from(format!("/test/file_{i}.rs")))
                .location(SourceLocation {
                    start_line: i * 10,
                    end_line: i * 10 + 5,
                    start_column: 0,
                    end_column: 0,
                })
                .line_range((i * 10, i * 10 + 5))
                .content(Some(format!("fn function_{i}() {{}}")))
                .language(Language::Rust)
                .build()
                .unwrap()
        })
        .collect()
}

#[tokio::test]
#[ignore] // Run with --ignored when Qdrant is available
async fn test_qdrant_collection_lifecycle() {
    if !qdrant_available().await {
        panic!("Qdrant not available, skipping test");
    }

    let config = StorageConfig {
        provider: "qdrant".to_string(),
        host: "localhost".to_string(),
        port: 6334,
        collection_name: "test_lifecycle".to_string(),
        ..Default::default()
    };

    let client = create_storage_client(config).await.unwrap();

    // Delete collection if it exists
    if client.collection_exists("test_lifecycle").await.unwrap() {
        client.delete_collection("test_lifecycle").await.unwrap();
    }

    // Create collection
    client.create_collection("test_lifecycle").await.unwrap();
    assert!(client.collection_exists("test_lifecycle").await.unwrap());

    // Creating again should be idempotent
    client.create_collection("test_lifecycle").await.unwrap();

    // Delete collection
    client.delete_collection("test_lifecycle").await.unwrap();
    assert!(!client.collection_exists("test_lifecycle").await.unwrap());
}

#[tokio::test]
#[ignore] // Run with --ignored when Qdrant is available
async fn test_qdrant_bulk_load() {
    if !qdrant_available().await {
        panic!("Qdrant not available, skipping test");
    }

    let config = StorageConfig {
        provider: "qdrant".to_string(),
        host: "localhost".to_string(),
        port: 6334,
        collection_name: "test_bulk_load".to_string(),
        batch_size: 10,
        ..Default::default()
    };

    let client = create_storage_client(config).await.unwrap();

    // Initialize (creates collection)
    client.initialize().await.unwrap();

    // Clear any existing data
    client.clear().await.unwrap();

    // Create test entities
    let entities = create_test_entities(50);
    let functions: Vec<CodeEntity> = entities.iter().take(20).cloned().collect();
    let types: Vec<CodeEntity> = entities.iter().skip(20).take(15).cloned().collect();
    let variables: Vec<CodeEntity> = entities.iter().skip(35).cloned().collect();

    // Load entities
    client
        .bulk_load_entities(&entities, &functions, &types, &variables, &Vec::new())
        .await
        .unwrap();

    // Clean up
    client.delete_collection("test_bulk_load").await.unwrap();
}

#[tokio::test]
#[ignore] // Run with --ignored when Qdrant is available
async fn test_qdrant_search_operations() {
    if !qdrant_available().await {
        panic!("Qdrant not available, skipping test");
    }

    let config = StorageConfig {
        provider: "qdrant".to_string(),
        host: "localhost".to_string(),
        port: 6334,
        collection_name: "test_search".to_string(),
        vector_size: 768,
        ..Default::default()
    };

    let client = create_storage_client(config.clone()).await.unwrap();

    // Initialize and clear
    client.initialize().await.unwrap();
    client.clear().await.unwrap();

    // Load some test entities
    let entities = create_test_entities(10);
    client
        .bulk_load_entities(&entities, &[], &[], &[], &[])
        .await
        .unwrap();

    // Wait a bit for indexing
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Test search with random vector (should return something)
    let query_vector: Vec<f32> = (0..config.vector_size).map(|i| i as f32 / 1000.0).collect();

    let results = client.search_similar(query_vector, 5, None).await.unwrap();

    // We should get some results (even with random vectors)
    assert!(!results.is_empty());
    assert!(results.len() <= 5);

    // Each result should have a score
    for result in &results {
        assert!(result.score >= 0.0);
        assert!(!result.entity.id.is_empty());
    }

    // Clean up
    client.delete_collection("test_search").await.unwrap();
}

#[tokio::test]
#[ignore] // Run with --ignored when Qdrant is available
async fn test_qdrant_get_by_id() {
    if !qdrant_available().await {
        panic!("Qdrant not available, skipping test");
    }

    let config = StorageConfig {
        provider: "qdrant".to_string(),
        host: "localhost".to_string(),
        port: 6334,
        collection_name: "test_get_by_id".to_string(),
        ..Default::default()
    };

    let client = create_storage_client(config).await.unwrap();

    // Initialize and clear
    client.initialize().await.unwrap();
    client.clear().await.unwrap();

    // Load test entities
    let entities = create_test_entities(5);
    client
        .bulk_load_entities(&entities, &[], &[], &[], &[])
        .await
        .unwrap();

    // Wait for indexing
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Get entity by ID - should find it
    let result = client.get_entity_by_id("entity_2").await.unwrap();
    assert!(result.is_some());

    if let Some(entity) = result {
        assert_eq!(entity.id, "entity_2");
        assert_eq!(entity.name, "function_2");
    }

    // Get non-existent entity
    let result = client.get_entity_by_id("non_existent").await.unwrap();
    assert!(result.is_none());

    // Get multiple entities by IDs
    let ids = vec![
        "entity_1".to_string(),
        "entity_3".to_string(),
        "non_existent".to_string(),
    ];
    let results = client.get_entities_by_ids(&ids).await.unwrap();

    // Should find 2 out of 3
    assert_eq!(results.len(), 2);

    let found_ids: Vec<String> = results.iter().map(|e| e.id.clone()).collect();
    assert!(found_ids.contains(&"entity_1".to_string()));
    assert!(found_ids.contains(&"entity_3".to_string()));

    // Clean up
    client.delete_collection("test_get_by_id").await.unwrap();
}

#[tokio::test]
#[ignore] // Run with --ignored when Qdrant is available
async fn test_qdrant_error_handling() {
    // Test with invalid connection parameters
    let config = StorageConfig {
        provider: "qdrant".to_string(),
        host: "invalid_host".to_string(),
        port: 9999,
        timeout_ms: 1000, // Short timeout
        ..Default::default()
    };

    let result = create_storage_client(config).await;
    assert!(result.is_err());

    // Error should mention connection failure
    if let Err(e) = result {
        assert!(e.to_string().contains("Connection failed"));
    }
}
