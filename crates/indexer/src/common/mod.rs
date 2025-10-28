//! Common utilities for the indexer
//!
//! This module provides utility functions for file operations,
//! pattern matching, and other common tasks.

use codesearch_core::error::{Error, Result};
use codesearch_watcher::GitRepository;
use std::path::Path;
use tracing::debug;

/// Convert a Path to &str with proper error handling
pub fn path_to_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| Error::Storage(format!("Invalid file path: {}", path.display())))
}

/// Get current git commit from a repository, with fallback behavior
pub fn get_current_commit(git_repo: Option<&GitRepository>, repo_root: &Path) -> Option<String> {
    git_repo
        .and_then(|repo| repo.current_commit_hash().ok())
        .or_else(|| {
            GitRepository::open(repo_root)
                .ok()
                .and_then(|repo| repo.current_commit_hash().ok())
        })
}

/// Extension trait for Result types to add storage error context
pub trait ResultExt<T> {
    /// Convert error to Storage error with context message
    fn storage_err(self, msg: &str) -> Result<T>;
}

impl<T, E: std::fmt::Display> ResultExt<T> for std::result::Result<T, E> {
    fn storage_err(self, msg: &str) -> Result<T> {
        self.map_err(|e| Error::Storage(format!("{msg}: {e}")))
    }
}

/// Check if a file has a supported extension
pub fn has_supported_extension(path: &Path) -> bool {
    codesearch_languages::detect_language(path).is_some()
}

/// Check if a file should be included in indexing
pub fn should_include_file(file_path: &Path) -> bool {
    // Single metadata call to avoid redundant syscalls and TOCTOU race conditions
    // Use symlink_metadata() to check the symlink itself, not its target
    let metadata = match file_path.symlink_metadata() {
        Ok(m) => m,
        Err(_) => return false,
    };

    // Reject symlinks to prevent following links outside repository
    // Note: The ignore crate's default behavior is to not follow symlinks,
    // but we explicitly check here as a defense-in-depth measure for safety
    if metadata.is_symlink() {
        debug!("Excluding symlink: {}", file_path.display());
        return false;
    }

    // Check if it's a regular file (not a directory)
    if !metadata.is_file() {
        return false;
    }

    // Check file size (skip very large files > 10MB)
    const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB
    if metadata.len() > MAX_FILE_SIZE {
        debug!(
            "Excluding large file: {} (size: {} bytes)",
            file_path.display(),
            metadata.len()
        );
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_should_include_file() {
        // Test that regular files are included
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "fn main() {}").expect("Failed to write test file");
        assert!(should_include_file(&test_file));

        // Test that large files are excluded (> 10MB)
        let large_file = temp_dir.path().join("large.rs");
        let large_content = "x".repeat(11 * 1024 * 1024); // 11MB
        fs::write(&large_file, large_content).expect("Failed to write large file");
        assert!(!should_include_file(&large_file));
    }

    #[test]
    fn test_has_supported_extension() {
        // Rust is fully implemented and registered
        assert!(has_supported_extension(Path::new("main.rs")));

        // JavaScript is now implemented (Phase 3 complete)
        assert!(has_supported_extension(Path::new("app.js")));
        assert!(has_supported_extension(Path::new("component.jsx")));

        // TypeScript is now implemented (Phase 4 complete)
        assert!(has_supported_extension(Path::new("module.ts")));
        assert!(has_supported_extension(Path::new("component.tsx")));

        // Other languages not yet implemented (Phase 5+ pending)
        assert!(!has_supported_extension(Path::new("lib.py")));
        assert!(!has_supported_extension(Path::new("main.go")));

        // Non-code files should never be supported
        assert!(!has_supported_extension(Path::new("README.md")));
        assert!(!has_supported_extension(Path::new("Cargo.toml")));
        assert!(!has_supported_extension(Path::new("file.txt")));
    }

    #[test]
    fn test_path_to_str_valid_utf8() {
        let path = Path::new("/valid/path/file.rs");
        let result = path_to_str(path);
        assert!(result.is_ok());
        assert_eq!(result.expect("Should convert path"), "/valid/path/file.rs");
    }

    #[test]
    fn test_path_to_str_handles_conversion() {
        // Test with a normal path that should convert successfully
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let test_file = temp_dir.path().join("test.rs");
        let result = path_to_str(&test_file);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_current_commit_with_none() {
        // Test fallback behavior when git_repo is None
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let result = get_current_commit(None, temp_dir.path());
        // Should be None since temp_dir is not a git repo
        assert!(result.is_none());
    }

    #[test]
    fn test_result_ext_storage_err() {
        // Test the ResultExt trait
        let error_result: std::result::Result<(), String> = Err("test error".to_string());
        let converted = error_result.storage_err("context message");

        assert!(converted.is_err());
        if let Err(Error::Storage(msg)) = converted {
            assert!(msg.contains("context message"));
            assert!(msg.contains("test error"));
        } else {
            panic!("Expected Storage error");
        }
    }

    #[test]
    fn test_result_ext_storage_err_ok() {
        // Test that Ok values pass through unchanged
        let ok_result: std::result::Result<i32, String> = Ok(42);
        let converted = ok_result.storage_err("context");

        assert!(converted.is_ok());
        assert_eq!(converted.expect("Should be Ok"), 42);
    }
}
