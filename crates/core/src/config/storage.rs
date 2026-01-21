//! Storage-related configuration methods

use crate::error::{Error, Result};
use std::path::Path;

use super::StorageConfig;

impl StorageConfig {
    /// Generate a collection name from a repository path
    ///
    /// Creates a unique, deterministic Qdrant-compatible collection name using xxHash3_128.
    /// Format: `<sanitized_repo_name>_<xxhash3_128_hex>`
    ///
    /// The repo name is sanitized (alphanumeric, dash, underscore only) and truncated to
    /// 50 characters if needed. The full absolute path is hashed using xxHash3_128 to ensure
    /// uniqueness. The same path always generates the same collection name.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The current directory cannot be determined (for relative paths)
    /// - The path has no valid filename component
    /// - The filename cannot be converted to UTF-8
    pub fn generate_collection_name(repo_path: &Path) -> Result<String> {
        use twox_hash::XxHash3_128;

        // Get the absolute path without requiring it to exist
        let absolute_path = if repo_path.is_absolute() {
            repo_path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| Error::config(format!("Failed to get current dir: {e}")))?
                .join(repo_path)
        };

        // Canonicalize the path to resolve symlinks and normalize (e.g., remove .. and .)
        // This prevents the same repository from being registered multiple times with
        // different path representations (e.g., /home/user/repo vs /home/user/../user/repo)
        // If the path doesn't exist, fall back to the absolute path
        let normalized_path =
            std::fs::canonicalize(&absolute_path).unwrap_or_else(|_| absolute_path.clone());

        // Extract repository name (last component of path)
        let repo_name = normalized_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                Error::config(format!(
                    "Path {} has no valid filename component",
                    normalized_path.display()
                ))
            })?;

        // Truncate repo name to 50 chars and sanitize
        let sanitized_name: String = repo_name
            .chars()
            .take(50)
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();

        // Hash the full normalized path to ensure uniqueness
        let path_str = normalized_path.to_string_lossy();
        let hash = XxHash3_128::oneshot(path_str.as_bytes());

        // Format: <repo_name>_<hash>
        Ok(format!("{sanitized_name}_{hash:032x}"))
    }

    /// Generate a deterministic repository ID from repository path
    ///
    /// Creates a deterministic UUID v5 from a repository path, ensuring the same path
    /// always generates the same UUID. This makes entity IDs stable across re-indexing,
    /// even if the repository is dropped and re-indexed.
    ///
    /// # UUID v5 Generation
    ///
    /// Uses UUID v5 (name-based, SHA-1) as defined in RFC 4122. The UUID is generated
    /// from the normalized repository path using `NAMESPACE_DNS` as the namespace UUID.
    /// While DNS namespace is typically used for domain names, it's a standard, well-known
    /// namespace suitable for generating deterministic UUIDs from filesystem paths.
    ///
    /// # Path Normalization
    ///
    /// The function normalizes paths to ensure consistent UUID generation:
    ///
    /// 1. **Relative paths**: Converted to absolute paths using the current working directory
    /// 2. **Symlinks**: Resolved to their target path via `std::fs::canonicalize`
    /// 3. **Path components**: Normalized (`.` and `..` are resolved)
    ///
    /// If canonicalization fails (e.g., permission errors, I/O errors), the function falls
    /// back to the absolute (but non-canonical) path. A warning is logged for non-NotFound
    /// errors to help diagnose cases where different path representations might generate
    /// different UUIDs.
    ///
    /// # Idempotency and Thread Safety
    ///
    /// - **Idempotent**: Calling this function multiple times with the same path always
    ///   returns the same UUID
    /// - **Thread-safe**: This function has no mutable state and can be called safely
    ///   from multiple threads
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The current directory cannot be determined (for relative paths)
    ///
    /// # Examples
    ///
    /// ```
    /// use codesearch_core::config::StorageConfig;
    /// use std::path::Path;
    ///
    /// // Absolute path
    /// let id1 = StorageConfig::generate_repository_id(Path::new("/home/user/repo")).unwrap();
    ///
    /// // Same path should produce same UUID
    /// let id2 = StorageConfig::generate_repository_id(Path::new("/home/user/repo")).unwrap();
    /// assert_eq!(id1, id2);
    ///
    /// // Different paths produce different UUIDs
    /// let id3 = StorageConfig::generate_repository_id(Path::new("/home/user/other")).unwrap();
    /// assert_ne!(id1, id3);
    /// ```
    pub fn generate_repository_id(repo_path: &Path) -> Result<uuid::Uuid> {
        // Get the absolute path without requiring it to exist
        let absolute_path = if repo_path.is_absolute() {
            repo_path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| Error::config(format!("Failed to get current dir: {e}")))?
                .join(repo_path)
        };

        // Canonicalize the path to resolve symlinks and normalize
        // If canonicalization fails, fall back to the absolute path and log a warning
        let normalized_path = match std::fs::canonicalize(&absolute_path) {
            Ok(canonical) => canonical,
            Err(e) => {
                // Log warning for non-NotFound errors (permissions, I/O, etc.)
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(
                        path = %absolute_path.display(),
                        error = %e,
                        "Failed to canonicalize repository path, using absolute path. \
                         Different path representations may generate different repository IDs."
                    );
                }
                absolute_path.clone()
            }
        };

        // Generate deterministic UUID v5 from the normalized path
        // Using DNS namespace as it's a standard namespace for name-based UUIDs
        let path_str = normalized_path.to_string_lossy();
        let repository_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, path_str.as_bytes());

        Ok(repository_id)
    }
}
