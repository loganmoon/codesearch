//! Common utilities for the indexer
//!
//! This module provides utility functions for file operations,
//! pattern matching, and other common tasks.

use codesearch_core::error::{Error, Result};
use codesearch_watcher::GitRepository;
use glob::glob;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

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

/// Default patterns to exclude from indexing
const DEFAULT_EXCLUDE_PATTERNS: &[&str] = &[
    "**/target/**",
    "**/node_modules/**",
    "**/.git/**",
    "**/dist/**",
    "**/build/**",
    "**/.vscode/**",
    "**/.idea/**",
    "**/vendor/**",
    "**/__pycache__/**",
    "**/.pytest_cache/**",
    "**/.cargo/**",
    "**/Cargo.lock",
    "**/package-lock.json",
    "**/yarn.lock",
    "**/*.min.js",
    "**/*.min.css",
];

/// Supported file extensions for indexing
/// Note: Only Rust is fully implemented. Python, JS/TS, and Go have partial infrastructure.
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "rs",  // Rust (fully implemented)
    "py",  // Python (partial infrastructure, no parsing)
    "js",  // JavaScript (partial infrastructure, no parsing)
    "jsx", // React JavaScript (partial infrastructure, no parsing)
    "ts",  // TypeScript (partial infrastructure, no parsing)
    "tsx", // React TypeScript (partial infrastructure, no parsing)
    "go",  // Go (partial infrastructure, no parsing)
];

/// Find all files in a directory that should be indexed
pub fn find_files(root_path: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = Vec::new();

    // Process each supported extension separately
    for ext in SUPPORTED_EXTENSIONS {
        let pattern = format!("{}/**/*.{ext}", root_path.display());
        debug!("Searching for {} files with pattern: {}", ext, pattern);

        for entry in glob(&pattern)
            .map_err(|e| Error::parse("glob", format!("Invalid glob pattern: {e}")))?
        {
            match entry {
                Ok(path) => {
                    // Check if file should be included
                    if should_include_file(&path) {
                        files.push(path);
                    }
                }
                Err(e) => {
                    warn!("Error reading file entry: {}", e);
                }
            }
        }
    }

    // Sort files for consistent processing order
    files.sort();

    debug!("Found {} files to index", files.len());
    Ok(files)
}

/// Check if a file should be included in indexing
pub fn should_include_file(file_path: &Path) -> bool {
    let path_str = file_path.to_string_lossy();

    // Check against exclude patterns
    for pattern in DEFAULT_EXCLUDE_PATTERNS {
        if path_matches_pattern(&path_str, pattern) {
            debug!(
                "Excluding file: {} (matches pattern: {})",
                path_str, pattern
            );
            return false;
        }
    }

    // Reject symlinks to prevent following links outside repository
    if file_path.is_symlink() {
        debug!("Excluding symlink: {}", path_str);
        return false;
    }

    // Check if it's a regular file (not a directory)
    if !file_path.is_file() {
        return false;
    }

    // Check file size (skip very large files > 10MB)
    if let Ok(metadata) = file_path.metadata() {
        const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB
        if metadata.len() > MAX_FILE_SIZE {
            debug!(
                "Excluding large file: {} (size: {} bytes)",
                path_str,
                metadata.len()
            );
            return false;
        }
    }

    true
}

/// Check if a path matches a glob-like pattern
fn path_matches_pattern(path: &str, pattern: &str) -> bool {
    // Simple pattern matching for common cases
    // This is a simplified version - could be enhanced with full glob support

    // Handle ** wildcard (matches any number of directories)
    let pattern = pattern.replace("**", "__STARSTAR__");
    let pattern = pattern.replace('*', "__STAR__");
    let pattern = pattern.replace("__STARSTAR__", ".*");
    let pattern = pattern.replace("__STAR__", "[^/]*");

    // Convert to regex pattern
    let pattern = format!("^{pattern}$");

    // Try to match
    if let Ok(re) = regex::Regex::new(&pattern) {
        re.is_match(path)
    } else {
        // Fallback to simple contains check
        path.contains(&pattern.replace(".*", "").replace("[^/]*", ""))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_should_include_file() {
        // Test exclude patterns
        assert!(!should_include_file(Path::new("target/debug/main")));
        assert!(!should_include_file(Path::new(
            "node_modules/package/index.js"
        )));
        assert!(!should_include_file(Path::new(".git/config")));

        // Test include patterns (these would need actual files to fully test)
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "fn main() {}").expect("Failed to write test file");
        assert!(should_include_file(&test_file));
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
