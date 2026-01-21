//! Tests for configuration module

use super::*;
use crate::error::{Error, Result};
use std::io::Write;
use tempfile::NamedTempFile;

fn create_temp_config_file(content: &str) -> Result<NamedTempFile> {
    let mut file = tempfile::Builder::new()
        .suffix(".toml")
        .tempfile()
        .map_err(|e| Error::config(format!("Failed to create temp file: {e}")))?;
    file.write_all(content.as_bytes())
        .map_err(|e| Error::config(format!("Failed to write temp file: {e}")))?;
    file.flush()
        .map_err(|e| Error::config(format!("Failed to flush temp file: {e}")))?;
    Ok(file)
}

fn with_env_var<F, T>(key: &str, value: &str, f: F) -> T
where
    F: FnOnce() -> T,
{
    std::env::set_var(key, value);
    let result = f();
    std::env::remove_var(key);
    result
}

#[test]
fn test_from_toml_str_valid() {
    let toml = r#"
        [indexer]

        [embeddings]
        provider = "localapi"
        model = "nomic-embed-text-v1.5"
        device = "cpu"
        embedding_dimension = 768

        [watcher]

        [storage]
        qdrant_host = "localhost"
        qdrant_port = 6334
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse valid TOML");
    assert_eq!(config.embeddings.provider, "localapi");
    assert_eq!(config.embeddings.embedding_dimension, 768);
}

#[test]
fn test_from_toml_str_minimal() {
    let toml = r#"
        [indexer]

        [embeddings]

        [watcher]

        [storage]
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse minimal TOML");
    // Check defaults are applied
    assert_eq!(config.embeddings.provider, "jina");
    assert_eq!(config.embeddings.device, "cpu");
}

#[test]
fn test_from_toml_str_invalid_syntax() {
    let toml = r#"
        [embeddings
        provider = "localapi"
    "#;

    let result = Config::from_toml_str(toml);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Failed to parse TOML"));
}

#[test]
fn test_validate_valid_config() {
    let toml = r#"
        [indexer]

        [embeddings]
        provider = "localapi"
        device = "cpu"
        embedding_dimension = 1536

        [watcher]

        [storage]
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_invalid_provider() {
    let toml = r#"
        [indexer]

        [embeddings]
        provider = "invalid_provider"

        [watcher]

        [storage]
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid provider"));
}

#[test]
fn test_validate_invalid_device() {
    let toml = r#"
        [indexer]

        [embeddings]
        provider = "localapi"
        device = "gpu"

        [watcher]

        [storage]
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid device"));
}

#[test]
fn test_validate_zero_embedding_dimension() {
    let toml = r#"
        [indexer]

        [embeddings]
        provider = "localapi"
        device = "cpu"
        embedding_dimension = 0

        [watcher]

        [storage]
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("embedding_dimension must be greater than 0"));
}

#[test]
fn test_save_and_load_roundtrip() -> Result<()> {
    let original_toml = r#"
        [indexer]

        [embeddings]
        provider = "mock"
        model = "test-model"
        device = "cpu"
        embedding_dimension = 384

        [watcher]

        [storage]
        qdrant_host = "testhost"
        qdrant_port = 7777
    "#;

    let config = Config::from_toml_str(original_toml)?;

    // Save to temp file
    let temp_file = NamedTempFile::new()
        .map_err(|e| Error::config(format!("Failed to create temp file: {e}")))?;
    config.save(temp_file.path())?;

    // Load from temp file
    let loaded_content = std::fs::read_to_string(temp_file.path())
        .map_err(|e| Error::config(format!("Failed to read temp file: {e}")))?;
    let loaded_config = Config::from_toml_str(&loaded_content)?;

    // Verify roundtrip
    assert_eq!(
        config.embeddings.provider,
        loaded_config.embeddings.provider
    );
    assert_eq!(config.embeddings.model, loaded_config.embeddings.model);
    assert_eq!(
        config.embeddings.embedding_dimension,
        loaded_config.embeddings.embedding_dimension
    );

    Ok(())
}

#[test]
fn test_from_file_loads_successfully() {
    let toml = r#"
        [indexer]

        [embeddings]
        provider = "mock"

        [watcher]

        [storage]
    "#;

    let temp_file = create_temp_config_file(toml).expect("Failed to create temp file");

    let config = Config::from_file(temp_file.path()).expect("Failed to load config from file");
    assert_eq!(config.embeddings.provider, "mock");
}

#[test]
fn test_from_file_backward_compat_qdrant() {
    let toml = r#"
        [indexer]

        [embeddings]

        [watcher]

        [storage]
    "#;

    let temp_file = create_temp_config_file(toml).expect("Failed to create temp file");

    with_env_var("QDRANT_HOST", "remote.example.com", || {
        with_env_var("QDRANT_PORT", "7334", || {
            let config =
                Config::from_file(temp_file.path()).expect("Failed to load config from file");
            assert_eq!(config.storage.qdrant_host, "remote.example.com");
            assert_eq!(config.storage.qdrant_port, 7334);
        });
    });
}

#[test]
fn test_from_file_backward_compat_postgres() {
    let toml = r#"
        [indexer]

        [embeddings]

        [watcher]

        [storage]
    "#;

    let temp_file = create_temp_config_file(toml).expect("Failed to create temp file");

    with_env_var("POSTGRES_HOST", "db.example.com", || {
        with_env_var("POSTGRES_DATABASE", "testdb", || {
            let config =
                Config::from_file(temp_file.path()).expect("Failed to load config from file");
            assert_eq!(config.storage.postgres_host, "db.example.com");
            assert_eq!(config.storage.postgres_database, "testdb");
        });
    });
}

#[test]
fn test_save_creates_valid_toml() {
    let toml = r#"
        [indexer]

        [embeddings]
        provider = "mock"
        model = "test-model"

        [watcher]

        [storage]
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");

    // Save to temp file
    let temp_file = NamedTempFile::new()
        .map_err(|e| Error::config(format!("Failed to create temp file: {e}")))
        .expect("Failed to create temp file");
    config
        .save(temp_file.path())
        .expect("Failed to save config");

    // Verify file was created and is valid TOML
    assert!(temp_file.path().exists());
    let saved_content =
        std::fs::read_to_string(temp_file.path()).expect("Failed to read saved config");
    assert!(saved_content.contains("[embeddings]"));
    assert!(saved_content.contains("[storage]"));
}

#[test]
fn test_generate_collection_name() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let collection_name = StorageConfig::generate_collection_name(temp_dir.path())
        .expect("Failed to generate collection name");

    // Verify format: name_hash
    assert!(collection_name.contains('_'));

    // Verify length is reasonable (50 + 1 + 32 = 83 max)
    assert!(collection_name.len() <= 83);

    // Verify only contains valid characters
    assert!(collection_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
}

#[test]
fn test_generate_collection_name_special_chars() {
    let temp_base = tempfile::tempdir().expect("Failed to create temp dir");
    let special_path = temp_base.path().join("my repo (v2.0)!");

    // Create the directory
    std::fs::create_dir(&special_path).expect("Failed to create dir");

    let collection_name = StorageConfig::generate_collection_name(&special_path)
        .expect("Failed to generate collection name");

    // Special characters should be replaced with underscores
    assert!(!collection_name.contains('('));
    assert!(!collection_name.contains(')'));
    assert!(!collection_name.contains('!'));
    assert!(!collection_name.contains(' '));
}

#[test]
fn test_generate_collection_name_deterministic() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    let name1 = StorageConfig::generate_collection_name(temp_dir.path())
        .expect("Failed to generate collection name");
    let name2 = StorageConfig::generate_collection_name(temp_dir.path())
        .expect("Failed to generate collection name");

    // Same path should generate same name
    assert_eq!(name1, name2);
}

#[test]
fn test_generate_collection_name_nonexistent_path() {
    // Non-existent paths should now work (no canonicalization required)
    let nonexistent = std::path::PathBuf::from("/tmp/this_path_does_not_exist_test_12345");

    let result = StorageConfig::generate_collection_name(&nonexistent);
    assert!(result.is_ok());

    let collection_name = result.expect("test setup failed");
    assert!(collection_name.contains("this_path_does_not_exist_test_12345"));
    assert!(collection_name.contains('_')); // Should have hash separator
}

#[test]
fn test_generate_collection_name_relative_path() {
    // Relative paths should work and be converted to absolute
    let relative = std::path::PathBuf::from("relative/test/path");

    let result = StorageConfig::generate_collection_name(&relative);
    assert!(result.is_ok());

    let collection_name = result.expect("test setup failed");
    // Should use the last component as the name
    assert!(collection_name.starts_with("path_"));
}

#[test]
fn test_generate_collection_name_root_path() {
    // Root path should fail - no filename component
    let root = std::path::PathBuf::from("/");

    let result = StorageConfig::generate_collection_name(&root);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("no valid filename component"));
}

#[test]
fn test_generate_repository_id_deterministic() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    let id1 = StorageConfig::generate_repository_id(temp_dir.path())
        .expect("Failed to generate repository ID");
    let id2 = StorageConfig::generate_repository_id(temp_dir.path())
        .expect("Failed to generate repository ID");

    // Same path should generate same UUID
    assert_eq!(id1, id2);
}

#[test]
fn test_generate_repository_id_different_paths() {
    let temp_dir1 = tempfile::tempdir().expect("Failed to create temp dir 1");
    let temp_dir2 = tempfile::tempdir().expect("Failed to create temp dir 2");

    let id1 = StorageConfig::generate_repository_id(temp_dir1.path())
        .expect("Failed to generate repository ID 1");
    let id2 = StorageConfig::generate_repository_id(temp_dir2.path())
        .expect("Failed to generate repository ID 2");

    // Different paths should generate different UUIDs
    assert_ne!(id1, id2);
}

#[test]
fn test_generate_repository_id_relative_vs_absolute() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    // Get absolute path
    let absolute_id = StorageConfig::generate_repository_id(temp_dir.path())
        .expect("Failed to generate ID from absolute path");

    // Change to parent directory and use relative path
    let original_dir = std::env::current_dir().expect("Failed to get current dir");
    let parent = temp_dir.path().parent().expect("No parent directory");
    let dir_name = temp_dir.path().file_name().expect("No file name");

    std::env::set_current_dir(parent).expect("Failed to change directory");
    let relative_id = StorageConfig::generate_repository_id(&std::path::PathBuf::from(dir_name))
        .expect("Failed to generate ID from relative path");
    std::env::set_current_dir(original_dir).expect("Failed to restore directory");

    // Relative and absolute paths should generate same UUID
    assert_eq!(absolute_id, relative_id);
}

#[test]
fn test_generate_repository_id_symlink_resolution() {
    use std::os::unix::fs::symlink;

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let real_path = temp_dir.path().join("real_repo");
    let symlink_path = temp_dir.path().join("symlink_repo");

    std::fs::create_dir(&real_path).expect("Failed to create real directory");
    symlink(&real_path, &symlink_path).expect("Failed to create symlink");

    let real_id = StorageConfig::generate_repository_id(&real_path)
        .expect("Failed to generate ID from real path");
    let symlink_id = StorageConfig::generate_repository_id(&symlink_path)
        .expect("Failed to generate ID from symlink");

    // Symlink should resolve to same UUID as real path
    assert_eq!(real_id, symlink_id);
}

#[test]
fn test_generate_repository_id_path_normalization() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    // Create subdirectory and sibling
    let subdir = temp_dir.path().join("subdir");
    let other = temp_dir.path().join("other");
    std::fs::create_dir(&subdir).expect("Failed to create subdirectory");
    std::fs::create_dir(&other).expect("Failed to create other directory");

    // Generate ID from clean path
    let clean_id = StorageConfig::generate_repository_id(&subdir)
        .expect("Failed to generate ID from clean path");

    // Generate ID from path with .. (e.g., /tmp/foo/other/../subdir)
    // This path exists and will canonicalize to /tmp/foo/subdir
    let with_parent = other.join("..").join("subdir");
    let normalized_id = StorageConfig::generate_repository_id(&with_parent)
        .expect("Failed to generate ID from path with ..");

    // Both should generate same UUID (after canonicalization)
    assert_eq!(clean_id, normalized_id);
}

#[test]
fn test_generate_repository_id_uuid_v5_format() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    let id = StorageConfig::generate_repository_id(temp_dir.path())
        .expect("Failed to generate repository ID");

    // UUID v5 has version bits set to 0101 (5) in the time_hi_and_version field
    // The variant should be 10xx (RFC 4122)
    let bytes = id.as_bytes();

    // Check version (bits 4-7 of byte 6 should be 0101 = 5)
    assert_eq!((bytes[6] >> 4) & 0x0F, 5, "UUID should be version 5");

    // Check variant (bits 6-7 of byte 8 should be 10)
    assert_eq!(
        (bytes[8] >> 6) & 0x03,
        2,
        "UUID should have RFC 4122 variant"
    );
}

#[test]
fn test_generate_repository_id_nonexistent_path() {
    // Non-existent paths should work (no canonicalization required for generation)
    let nonexistent = std::path::PathBuf::from("/tmp/this_path_does_not_exist_test_12345");

    let result = StorageConfig::generate_repository_id(&nonexistent);
    assert!(result.is_ok(), "Should handle non-existent paths");

    // Should be deterministic even for non-existent paths
    let id1 = result.expect("Failed to generate ID");
    let id2 = StorageConfig::generate_repository_id(&nonexistent)
        .expect("Failed to generate ID second time");
    assert_eq!(id1, id2, "Non-existent paths should still be deterministic");
}

#[test]
fn test_reranking_config_custom_values() {
    let toml = r#"
        [indexer]

        [embeddings]

        [watcher]

        [storage]

        [reranking]
        enabled = true
        model = "custom-model"
        candidates = 100
        top_k = 20
        api_base_url = "http://localhost:8001"
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse TOML with custom reranking");

    assert!(config.reranking.enabled);
    assert_eq!(config.reranking.model, "custom-model");
    assert_eq!(config.reranking.candidates, 100);
    assert_eq!(config.reranking.top_k, 20);
    assert_eq!(
        config.reranking.api_base_url,
        Some("http://localhost:8001".to_string())
    );
}

#[test]
fn test_reranking_validation_candidates_too_large() {
    let toml = r#"
        [indexer]

        [embeddings]

        [watcher]

        [storage]

        [reranking]
        enabled = true
        candidates = 2000
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("too large"));
}

#[test]
fn test_reranking_validation_top_k_exceeds_candidates() {
    let toml = r#"
        [indexer]

        [embeddings]

        [watcher]

        [storage]

        [reranking]
        enabled = true
        candidates = 50
        top_k = 100
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("top_k"));
    assert!(error_msg.contains("cannot exceed"));
}

#[test]
fn test_reranking_validation_disabled_no_check() {
    let toml = r#"
        [indexer]

        [embeddings]

        [watcher]

        [storage]

        [reranking]
        enabled = false
        candidates = 2000
        top_k = 100
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();

    // Should pass validation because reranking is disabled
    assert!(result.is_ok());
}

#[test]
fn test_reranking_config_jina_provider() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]

        [reranking]
        enabled = true
        provider = "jina"
        model = "jina-reranker-v3"
        api_key = "test_key"
        candidates = 100
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse TOML with Jina provider");

    assert!(config.reranking.enabled);
    assert_eq!(config.reranking.provider, "jina");
    assert_eq!(config.reranking.model, "jina-reranker-v3");
    assert_eq!(config.reranking.api_key, Some("test_key".to_string()));
    assert_eq!(config.reranking.candidates, 100);
}

#[test]
fn test_reranking_config_vllm_provider() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]

        [reranking]
        enabled = true
        provider = "vllm"
        model = "BAAI/bge-reranker-v2-m3"
        api_base_url = "http://localhost:8001/v1"
        candidates = 350
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse TOML with vLLM provider");

    assert!(config.reranking.enabled);
    assert_eq!(config.reranking.provider, "vllm");
    assert_eq!(config.reranking.model, "BAAI/bge-reranker-v2-m3");
    assert_eq!(config.reranking.candidates, 350);
}

#[test]
fn test_reranking_config_invalid_provider() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]

        [reranking]
        provider = "invalid_provider"
    "#;

    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Invalid reranking provider"));
    assert!(error_msg.contains("invalid_provider"));
}

#[test]
fn test_reranking_defaults_to_jina() {
    let config = RerankingConfig::default();
    assert_eq!(config.provider, "jina");
    assert_eq!(config.model, "jina-reranker-v3");
    assert_eq!(config.candidates, 100);
}

#[test]
fn test_outbox_validation_poll_interval_zero() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]
        [outbox]
        poll_interval_ms = 0
    "#;
    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("poll_interval_ms must be greater than 0"));
}

#[test]
fn test_outbox_validation_poll_interval_too_large() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]
        [outbox]
        poll_interval_ms = 60001
    "#;
    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("poll_interval_ms too large"));
}

#[test]
fn test_outbox_validation_entries_per_poll_zero() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]
        [outbox]
        entries_per_poll = 0
    "#;
    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("entries_per_poll must be greater than 0"));
}

#[test]
fn test_outbox_validation_entries_per_poll_negative() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]
        [outbox]
        entries_per_poll = -1
    "#;
    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("entries_per_poll must be greater than 0"));
}

#[test]
fn test_outbox_validation_entries_per_poll_too_large() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]
        [outbox]
        entries_per_poll = 1001
    "#;
    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("entries_per_poll too large"));
}

#[test]
fn test_outbox_validation_max_retries_negative() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]
        [outbox]
        max_retries = -1
    "#;
    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("max_retries must be non-negative"));
}

#[test]
fn test_outbox_validation_max_embedding_dim_zero() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]
        [outbox]
        max_embedding_dim = 0
    "#;
    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("max_embedding_dim must be greater than 0"));
}

#[test]
fn test_outbox_validation_max_cached_collections_zero() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]
        [outbox]
        max_cached_collections = 0
    "#;
    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("max_cached_collections must be greater than 0"));
}

#[test]
fn test_outbox_validation_max_cached_collections_too_large() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]
        [outbox]
        max_cached_collections = 1001
    "#;
    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("max_cached_collections too large"));
}

#[test]
fn test_outbox_validation_valid_config() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]
        [outbox]
        poll_interval_ms = 1000
        entries_per_poll = 100
        max_retries = 3
        max_embedding_dim = 100000
        max_cached_collections = 200
    "#;
    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_ok());
}

#[test]
fn test_outbox_validation_defaults() {
    let toml = r#"
        [indexer]
        [embeddings]
        [watcher]
        [storage]
    "#;
    let config = Config::from_toml_str(toml).expect("Failed to parse TOML");
    let result = config.validate();
    assert!(result.is_ok());
    assert_eq!(config.outbox.poll_interval_ms, 1000);
    assert_eq!(config.outbox.entries_per_poll, 500);
    assert_eq!(config.outbox.max_retries, 3);
    assert_eq!(config.outbox.max_embedding_dim, 100_000);
    assert_eq!(config.outbox.max_cached_collections, 200);
}

#[test]
fn test_reranking_request_config_merge_override_all() {
    let base = RerankingConfig {
        enabled: false,
        provider: "jina".to_string(),
        model: "base-model".to_string(),
        candidates: 100,
        top_k: 10,
        api_base_url: Some("http://base.com".to_string()),
        api_key: Some("base-key".to_string()),
        timeout_secs: 30,
        max_concurrent_requests: 16,
    };

    let request = RerankingRequestConfig {
        enabled: Some(true),
        candidates: Some(350),
        top_k: Some(20),
    };

    let merged = request.merge_with(&base);

    assert!(merged.enabled);
    assert_eq!(merged.candidates, 350);
    assert_eq!(merged.top_k, 20);
    assert_eq!(merged.model, "base-model");
    assert_eq!(merged.api_base_url, Some("http://base.com".to_string()));
    assert_eq!(merged.api_key, Some("base-key".to_string()));
    assert_eq!(merged.timeout_secs, 30);
}

#[test]
fn test_reranking_request_config_merge_partial_override() {
    let base = RerankingConfig {
        enabled: true,
        provider: "jina".to_string(),
        model: "base-model".to_string(),
        candidates: 100,
        top_k: 10,
        api_base_url: None,
        api_key: None,
        timeout_secs: 30,
        max_concurrent_requests: 16,
    };

    let request = RerankingRequestConfig {
        enabled: None,
        candidates: Some(200),
        top_k: None,
    };

    let merged = request.merge_with(&base);

    assert!(merged.enabled);
    assert_eq!(merged.candidates, 200);
    assert_eq!(merged.top_k, 10);
    assert_eq!(merged.model, "base-model");
}

#[test]
fn test_reranking_request_config_merge_no_override() {
    let base = RerankingConfig {
        enabled: true,
        provider: "jina".to_string(),
        model: "base-model".to_string(),
        candidates: 100,
        top_k: 10,
        api_base_url: None,
        api_key: None,
        timeout_secs: 30,
        max_concurrent_requests: 16,
    };

    let request = RerankingRequestConfig {
        enabled: None,
        candidates: None,
        top_k: None,
    };

    let merged = request.merge_with(&base);

    assert!(merged.enabled);
    assert_eq!(merged.candidates, 100);
    assert_eq!(merged.top_k, 10);
    assert_eq!(merged.model, "base-model");
}

#[test]
fn test_reranking_request_config_merge_enforces_1000_limit() {
    let base = RerankingConfig {
        enabled: true,
        provider: "jina".to_string(),
        model: "base-model".to_string(),
        candidates: 100,
        top_k: 10,
        api_base_url: None,
        api_key: None,
        timeout_secs: 30,
        max_concurrent_requests: 16,
    };

    let request = RerankingRequestConfig {
        enabled: None,
        candidates: Some(5000),
        top_k: None,
    };

    let merged = request.merge_with(&base);

    assert_eq!(merged.candidates, 1000);
}

#[test]
fn test_reranking_request_config_merge_allows_1000() {
    let base = RerankingConfig {
        enabled: true,
        provider: "jina".to_string(),
        model: "base-model".to_string(),
        candidates: 100,
        top_k: 10,
        api_base_url: None,
        api_key: None,
        timeout_secs: 30,
        max_concurrent_requests: 16,
    };

    let request = RerankingRequestConfig {
        enabled: None,
        candidates: Some(1000),
        top_k: None,
    };

    let merged = request.merge_with(&base);

    assert_eq!(merged.candidates, 1000);
}
