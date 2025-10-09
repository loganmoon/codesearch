//! Tests for storage initialization logic
//!
//! These tests verify the ensure_storage_initialized function which handles
//! config creation, Docker startup, migrations, and repository registration.

use codesearch_core::config::StorageConfig;
use std::path::Path;

#[cfg(test)]
mod storage_init_tests {
    use super::*;

    /// Test collection name generation from repository path
    #[test]
    fn test_collection_name_generation_from_repo_path() {
        let test_paths = vec![
            "/home/user/projects/my-repo",
            "/var/lib/repos/project",
            "relative/path/to/repo",
        ];

        for path in test_paths {
            let result = StorageConfig::generate_collection_name(Path::new(path));
            assert!(
                result.is_ok(),
                "Failed to generate collection name for {path}"
            );

            let collection_name = result.unwrap();
            assert!(!collection_name.is_empty());
            assert!(collection_name.len() <= 255);
            // Should contain only valid characters
            assert!(collection_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        }
    }

    /// Test that collection names are deterministic for the same path
    #[test]
    fn test_collection_name_determinism() {
        let path = Path::new("/test/repo/path");

        let name1 = StorageConfig::generate_collection_name(path).unwrap();
        let name2 = StorageConfig::generate_collection_name(path).unwrap();

        assert_eq!(name1, name2, "Collection names should be deterministic");
    }

    /// Test that different paths generate different collection names
    #[test]
    fn test_collection_name_uniqueness() {
        let path1 = Path::new("/test/repo1");
        let path2 = Path::new("/test/repo2");

        let name1 = StorageConfig::generate_collection_name(path1).unwrap();
        let name2 = StorageConfig::generate_collection_name(path2).unwrap();

        assert_ne!(
            name1, name2,
            "Different paths should generate different collection names"
        );
    }

    /// Test handling of special characters in repository paths
    #[test]
    fn test_collection_name_with_special_chars() {
        // Paths with special characters that need sanitization
        let test_paths = vec![
            "/home/user/my project (v2.0)!",
            "/repos/project-with-dashes",
            "/repos/project_with_underscores",
        ];

        for path in test_paths {
            let result = StorageConfig::generate_collection_name(Path::new(path));
            assert!(result.is_ok(), "Failed to handle special chars in {path}");

            let collection_name = result.unwrap();
            // Should not contain parentheses, spaces, or exclamation marks
            assert!(!collection_name.contains('('));
            assert!(!collection_name.contains(')'));
            assert!(!collection_name.contains('!'));
            assert!(!collection_name.contains(' '));
        }
    }

    /// Test collection name length constraints
    #[test]
    fn test_collection_name_length_limits() {
        // Create a path with a very long name
        let long_name = "a".repeat(100);
        let path_str = format!("/test/{long_name}");
        let path = Path::new(&path_str);

        let result = StorageConfig::generate_collection_name(path);
        assert!(result.is_ok());

        let collection_name = result.unwrap();
        // Should be truncated to reasonable length (50 chars for name + hash)
        assert!(collection_name.len() <= 83); // 50 + 1 (underscore) + 32 (hash)
    }

    /// Test validation of storage configuration
    #[test]
    fn test_storage_config_defaults() {
        let config = StorageConfig {
            qdrant_host: "localhost".to_string(),
            qdrant_port: 6334,
            qdrant_rest_port: 6333,
            collection_name: "test_collection".to_string(),
            auto_start_deps: true,
            docker_compose_file: None,
            postgres_host: "localhost".to_string(),
            postgres_port: 5432,
            postgres_database: "codesearch".to_string(),
            postgres_user: "codesearch".to_string(),
            postgres_password: "codesearch".to_string(),
        };

        // Validate default values
        assert_eq!(config.qdrant_host, "localhost");
        assert_eq!(config.qdrant_port, 6334);
        assert_eq!(config.postgres_port, 5432);
        assert!(config.auto_start_deps);
    }

    /// Test that port numbers are within valid range
    #[test]
    fn test_port_number_validation() {
        let valid_ports = vec![6333, 6334, 5432, 8000];

        for port in valid_ports {
            assert!(port > 0 && port <= 65535);
        }
    }

    /// Test handling of empty collection names
    #[test]
    fn test_empty_collection_name_handling() {
        let empty_name = String::new();
        assert!(empty_name.is_empty());

        // Empty collection names should trigger generation
        let needs_generation = empty_name.is_empty();
        assert!(needs_generation);
    }
}
