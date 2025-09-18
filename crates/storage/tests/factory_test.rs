use codesearch_core::config::StorageConfig;
use codesearch_storage::{create_storage_client, StorageClient, StorageManager};

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
