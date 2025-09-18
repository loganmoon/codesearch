use codesearch_core::config::{Config, StorageConfig};

#[test]
fn test_storage_config_defaults() {
    let config = StorageConfig::default();
    assert_eq!(config.provider, "qdrant");
    assert_eq!(config.host, "localhost");
    assert_eq!(config.port, 6334);
    assert_eq!(config.collection_name, "codesearch");
    assert_eq!(config.vector_size, 768);
    assert_eq!(config.distance_metric, "cosine");
    assert_eq!(config.batch_size, 100);
    assert_eq!(config.timeout_ms, 30000);
    assert!(!config.use_mock);
}

#[test]
fn test_config_validation_storage_provider() {
    let mut config = Config::default();

    // Valid provider
    config.storage.provider = "qdrant".to_string();
    assert!(config.validate().is_ok());

    config.storage.provider = "mock".to_string();
    assert!(config.validate().is_ok());

    // Invalid provider
    config.storage.provider = "invalid".to_string();
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid storage provider"));
}

#[test]
fn test_config_validation_vector_size() {
    let mut config = Config::default();

    // Valid sizes
    config.storage.vector_size = 768;
    assert!(config.validate().is_ok());

    config.storage.vector_size = 1;
    assert!(config.validate().is_ok());

    config.storage.vector_size = 4096;
    assert!(config.validate().is_ok());

    // Invalid sizes
    config.storage.vector_size = 0;
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid vector size"));

    config.storage.vector_size = 4097;
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid vector size"));
}

#[test]
fn test_config_validation_distance_metric() {
    let mut config = Config::default();

    // Valid metrics
    config.storage.distance_metric = "cosine".to_string();
    assert!(config.validate().is_ok());

    config.storage.distance_metric = "euclidean".to_string();
    assert!(config.validate().is_ok());

    config.storage.distance_metric = "dot".to_string();
    assert!(config.validate().is_ok());

    // Invalid metric
    config.storage.distance_metric = "manhattan".to_string();
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid distance metric"));
}

#[test]
fn test_config_validation_batch_size() {
    let mut config = Config::default();

    // Valid sizes
    config.storage.batch_size = 100;
    assert!(config.validate().is_ok());

    config.storage.batch_size = 1;
    assert!(config.validate().is_ok());

    config.storage.batch_size = 1000;
    assert!(config.validate().is_ok());

    // Invalid sizes
    config.storage.batch_size = 0;
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid batch size"));

    config.storage.batch_size = 1001;
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid batch size"));
}

#[test]
fn test_config_validation_port() {
    let mut config = Config::default();

    // Valid port
    config.storage.port = 6334;
    assert!(config.validate().is_ok());

    config.storage.port = 1;
    assert!(config.validate().is_ok());

    // Invalid port
    config.storage.port = 0;
    let result = config.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid port"));
}

#[test]
fn test_config_from_toml() {
    let toml_content = r#"
        [indexer]

        [watcher]
        debounce_ms = 500
        branch_strategy = "index_current"

        [languages]
        enabled = ["rust"]

        [storage]
        provider = "qdrant"
        host = "192.168.1.100"
        port = 6335
        collection_name = "my_collection"
        vector_size = 512
        distance_metric = "euclidean"
        batch_size = 50
        timeout_ms = 60000
        use_mock = false

        [embeddings]
        provider = "local"
        model = "all-minilm-l6-v2"
        device = "cpu"
    "#;

    let config = Config::from_toml_str(toml_content).unwrap();
    assert_eq!(config.storage.provider, "qdrant");
    assert_eq!(config.storage.host, "192.168.1.100");
    assert_eq!(config.storage.port, 6335);
    assert_eq!(config.storage.collection_name, "my_collection");
    assert_eq!(config.storage.vector_size, 512);
    assert_eq!(config.storage.distance_metric, "euclidean");
    assert_eq!(config.storage.batch_size, 50);
    assert_eq!(config.storage.timeout_ms, 60000);
}

#[test]
fn test_config_save_and_load() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // Create a config with custom values
    let mut config = Config::default();
    config.storage.host = "test-host".to_string();
    config.storage.port = 9999;
    config.storage.collection_name = "test_collection".to_string();

    // Save it
    config.save(&config_path).unwrap();

    // Load it back
    let loaded = Config::from_file(&config_path).unwrap();

    // Verify the values match
    assert_eq!(loaded.storage.host, "test-host");
    assert_eq!(loaded.storage.port, 9999);
    assert_eq!(loaded.storage.collection_name, "test_collection");

    // Clean up
    fs::remove_file(config_path).unwrap();
}
