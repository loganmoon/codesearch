//! Common utilities for the indexer
//!
//! This module provides utility functions for file operations,
//! pattern matching, and other common tasks.

use codesearch_core::error::{Error, Result};
use glob::glob;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

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
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "rs",    // Rust
    "py",    // Python
    "js",    // JavaScript
    "jsx",   // React JavaScript
    "ts",    // TypeScript
    "tsx",   // React TypeScript
    "go",    // Go
    "java",  // Java
    "c",     // C
    "cpp",   // C++
    "cc",    // C++
    "cxx",   // C++
    "h",     // C/C++ headers
    "hpp",   // C++ headers
    "cs",    // C#
    "rb",    // Ruby
    "php",   // PHP
    "swift", // Swift
    "kt",    // Kotlin
    "scala", // Scala
    "r",     // R
    "lua",   // Lua
    "dart",  // Dart
    "zig",   // Zig
];

/// Find all files in a directory that should be indexed
pub fn find_files(root_path: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = Vec::new();

    // Process each supported extension separately
    for ext in SUPPORTED_EXTENSIONS {
        let pattern = format!("{}/**/*.{}", root_path.display(), ext);
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

    // Check if it's a regular file (not a directory or symlink)
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
    let pattern = format!("^{}$", pattern.replace('/', "/"));

    // Try to match
    if let Ok(re) = regex::Regex::new(&pattern) {
        re.is_match(path)
    } else {
        // Fallback to simple contains check
        path.contains(&pattern.replace(".*", "").replace("[^/]*", ""))
    }
}

/// Get the language identifier from a file extension
pub fn get_language_from_extension(extension: &str) -> Option<&'static str> {
    match extension.to_lowercase().as_str() {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "js" | "jsx" => Some("javascript"),
        "ts" | "tsx" => Some("typescript"),
        "go" => Some("go"),
        "java" => Some("java"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" => Some("cpp"),
        "cs" => Some("csharp"),
        "rb" => Some("ruby"),
        "php" => Some("php"),
        "swift" => Some("swift"),
        "kt" => Some("kotlin"),
        "scala" => Some("scala"),
        "r" => Some("r"),
        "lua" => Some("lua"),
        "dart" => Some("dart"),
        "zig" => Some("zig"),
        _ => None,
    }
}

/// Calculate a simple hash for a file's content
pub fn calculate_file_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Count lines in a string
pub fn count_lines(content: &str) -> usize {
    content.lines().count()
}

/// Extract the relative path from a base directory
pub fn get_relative_path(base: &Path, full_path: &Path) -> Result<PathBuf> {
    full_path
        .strip_prefix(base)
        .map(|p| p.to_path_buf())
        .map_err(|_| {
            Error::parse(
                full_path.display().to_string(),
                format!("Failed to get relative path from {base:?}"),
            )
        })
}

#[cfg(test)]
mod tests {
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
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "fn main() {}").unwrap();
        assert!(should_include_file(&test_file));
    }

    #[test]
    fn test_get_language_from_extension() {
        assert_eq!(get_language_from_extension("rs"), Some("rust"));
        assert_eq!(get_language_from_extension("py"), Some("python"));
        assert_eq!(get_language_from_extension("js"), Some("javascript"));
        assert_eq!(get_language_from_extension("unknown"), None);
    }

    #[test]
    fn test_count_lines() {
        assert_eq!(count_lines(""), 0);
        assert_eq!(count_lines("single line"), 1);
        assert_eq!(count_lines("line 1\nline 2\nline 3"), 3);
        assert_eq!(count_lines("line 1\n\nline 3"), 3); // Empty line counts
    }

    #[test]
    fn test_calculate_file_hash() {
        let hash1 = calculate_file_hash("content");
        let hash2 = calculate_file_hash("content");
        let hash3 = calculate_file_hash("different");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_get_relative_path() {
        let base = Path::new("/home/user/project");
        let full = Path::new("/home/user/project/src/main.rs");
        let relative = get_relative_path(base, full).unwrap();
        assert_eq!(relative, Path::new("src/main.rs"));

        // Test error case
        let other = Path::new("/other/path");
        assert!(get_relative_path(base, other).is_err());
    }
}
