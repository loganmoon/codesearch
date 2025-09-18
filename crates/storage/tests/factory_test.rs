use codesearch_core::config::StorageConfig;
use codesearch_storage::{create_storage_client, StorageClient, StorageManager};

#[tokio::test]
async fn test_factory_creates_mock_client() {
    let mut config = StorageConfig::default();
    config.use_mock = true;

    let client = create_storage_client(config).await.unwrap();

    // Mock client should always succeed
    assert!(client.initialize().await.is_ok());
    assert!(client.clear().await.is_ok());

    // Mock collection operations should work
    assert!(client.create_collection("test_collection").await.is_ok());
    assert!(!client.collection_exists("test_collection").await.unwrap());
    assert!(client.delete_collection("test_collection").await.is_ok());
}

#[tokio::test]
async fn test_factory_with_mock_provider() {
    let mut config = StorageConfig::default();
    config.provider = "mock".to_string();

    let client = create_storage_client(config).await.unwrap();

    // Should create mock client regardless of use_mock flag
    assert!(client.initialize().await.is_ok());
}

#[tokio::test]
async fn test_factory_returns_trait_object() {
    let config = StorageConfig {
        use_mock: true,
        ..Default::default()
    };

    let client = create_storage_client(config).await.unwrap();

    // Verify we can use it as both traits
    let _storage_client: &dyn StorageClient = &*client;
    let _storage_manager: &dyn StorageManager = &*client;
}

#[tokio::test]
async fn test_storage_entity_conversion() {
    use codesearch_core::{
        entities::{CodeEntityBuilder, Language, SourceLocation},
        EntityType,
    };
    use codesearch_storage::StorageEntity;
    use std::path::PathBuf;

    let code_entity = CodeEntityBuilder::default()
        .entity_id("test_id".to_string())
        .name("test_function".to_string())
        .qualified_name("module::test_function".to_string())
        .entity_type(EntityType::Function)
        .file_path(PathBuf::from("/test/file.rs"))
        .location(SourceLocation {
            start_line: 10,
            end_line: 20,
            start_column: 0,
            end_column: 0,
        })
        .line_range((10, 20))
        .content(Some("fn test_function() {}".to_string()))
        .language(Language::Rust)
        .build()
        .unwrap();

    let storage_entity = StorageEntity::from(code_entity.clone());

    assert_eq!(storage_entity.id, "test_id");
    assert_eq!(storage_entity.name, "test_function");
    assert_eq!(storage_entity.kind, "Function");
    assert_eq!(storage_entity.file_path, "/test/file.rs");
    assert_eq!(storage_entity.start_line, 10);
    assert_eq!(storage_entity.end_line, 20);
    assert_eq!(storage_entity.content, "fn test_function() {}");
    assert!(storage_entity.embedding.is_none());
}
