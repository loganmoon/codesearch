//! Type definitions for the indexer
//!
//! This module contains types used throughout the indexing pipeline.

use codesearch_core::error::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Result of an indexing operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexResult {
    /// Statistics about the indexing operation
    pub stats: IndexStats,
    /// Any errors that occurred (non-fatal)
    pub errors: Vec<String>,
}

/// Statistics for indexing operations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexStats {
    /// Total number of files processed
    pub total_files: usize,
    /// Number of files that failed processing
    pub failed_files: usize,
    /// Number of entities extracted
    pub entities_extracted: usize,
    /// Number of relationships extracted
    pub relationships_extracted: usize,
    /// Number of functions indexed
    pub functions_indexed: usize,
    /// Number of types indexed
    pub types_indexed: usize,
    /// Number of variables indexed
    pub variables_indexed: usize,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
    /// Memory usage in bytes (approximate)
    pub memory_usage_bytes: Option<u64>,
}

impl IndexStats {
    /// Merge another stats instance into this one
    pub fn merge(&mut self, other: IndexStats) {
        self.total_files += other.total_files;
        self.failed_files += other.failed_files;
        self.entities_extracted += other.entities_extracted;
        self.relationships_extracted += other.relationships_extracted;
        self.functions_indexed += other.functions_indexed;
        self.types_indexed += other.types_indexed;
        self.variables_indexed += other.variables_indexed;
        self.processing_time_ms += other.processing_time_ms;

        // For memory, take the max if both are present
        self.memory_usage_bytes = match (self.memory_usage_bytes, other.memory_usage_bytes) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
    }
}

/// Context for processing diffs in incremental updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffContext {
    /// The commit hash or version being processed
    pub commit_hash: Option<String>,
    /// Files that were added
    pub added_files: Vec<PathBuf>,
    /// Files that were modified
    pub modified_files: Vec<PathBuf>,
    /// Files that were deleted
    pub deleted_files: Vec<PathBuf>,
    /// Files that were renamed (old_path -> new_path)
    pub renamed_files: Vec<(PathBuf, PathBuf)>,
    /// The diff content for each changed file
    pub file_diffs: HashMap<PathBuf, FileDiff>,
}

impl DiffContext {
    /// Create a new empty diff context
    pub fn new() -> Self {
        Self {
            commit_hash: None,
            added_files: Vec::new(),
            modified_files: Vec::new(),
            deleted_files: Vec::new(),
            renamed_files: Vec::new(),
            file_diffs: HashMap::new(),
        }
    }

    /// Check if the diff context is empty (no changes)
    pub fn is_empty(&self) -> bool {
        self.added_files.is_empty()
            && self.modified_files.is_empty()
            && self.deleted_files.is_empty()
            && self.renamed_files.is_empty()
    }

    /// Get the total number of changed files
    pub fn total_changes(&self) -> usize {
        self.added_files.len()
            + self.modified_files.len()
            + self.deleted_files.len()
            + self.renamed_files.len()
    }
}

impl Default for DiffContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Diff information for a single file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    /// Lines that were added (line number -> content)
    pub added_lines: HashMap<usize, String>,
    /// Lines that were removed (line number -> content)
    pub removed_lines: HashMap<usize, String>,
    /// Hunks of changes
    pub hunks: Vec<DiffHunk>,
}

/// A hunk of changes in a diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    /// Starting line in the old file
    pub old_start: usize,
    /// Number of lines in the old file
    pub old_lines: usize,
    /// Starting line in the new file
    pub new_start: usize,
    /// Number of lines in the new file
    pub new_lines: usize,
    /// The actual diff content
    pub content: String,
}

/// Represents a change to an entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityChange {
    /// The entity ID
    pub entity_id: String,
    /// Type of change
    pub change_type: ChangeType,
    /// Old entity data (for updates and deletes)
    pub old_entity: Option<EntitySnapshot>,
    /// New entity data (for creates and updates)
    pub new_entity: Option<EntitySnapshot>,
    /// Changed fields (for updates)
    pub changed_fields: Vec<String>,
}

/// Type of change to an entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeType {
    /// Entity was created
    Created,
    /// Entity was updated
    Updated,
    /// Entity was deleted
    Deleted,
    /// Entity was moved/renamed
    Moved,
}

/// Snapshot of an entity at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySnapshot {
    /// Entity ID
    pub id: String,
    /// Entity name
    pub name: String,
    /// Qualified name
    pub qualified_name: String,
    /// Entity type
    pub entity_type: String,
    /// File path
    pub file_path: String,
    /// Start line
    pub start_line: usize,
    /// End line
    pub end_line: usize,
    /// Content hash
    pub content_hash: String,
}

/// Configuration for the indexer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexerConfig {
    /// Maximum number of concurrent file processing tasks
    pub max_concurrent_files: usize,
    /// Enable incremental indexing
    pub incremental: bool,
    /// Enable progress reporting
    pub show_progress: bool,
    /// Patterns to exclude from indexing
    pub exclude_patterns: Vec<String>,
    /// Patterns to include in indexing
    pub include_patterns: Vec<String>,
    /// Maximum file size to process (in bytes)
    pub max_file_size: u64,
    /// Languages to process
    pub languages: Vec<String>,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_files: 4,
            incremental: false,
            show_progress: true,
            exclude_patterns: Vec::new(),
            include_patterns: Vec::new(),
            max_file_size: 10 * 1024 * 1024, // 10MB
            languages: vec![
                "rust".to_string(),
                "python".to_string(),
                "javascript".to_string(),
                "typescript".to_string(),
                "go".to_string(),
            ],
        }
    }
}

/// Error types specific to indexing operations
#[derive(Debug, Clone)]
pub enum IndexError {
    /// File could not be read
    FileReadError(PathBuf, String),
    /// Language not supported
    UnsupportedLanguage(String),
    /// Extraction failed
    ExtractionError(PathBuf, String),
    /// Transformation failed
    TransformationError(PathBuf, String),
    /// Storage operation failed
    StorageError(PathBuf, String),
    /// Generic error
    Generic(String),
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileReadError(path, msg) => {
                write!(f, "Failed to read file {path:?}: {msg}")
            }
            Self::UnsupportedLanguage(lang) => {
                write!(f, "Unsupported language: {lang}")
            }
            Self::ExtractionError(path, msg) => {
                write!(f, "Extraction failed for {path:?}: {msg}")
            }
            Self::TransformationError(path, msg) => {
                write!(f, "Transformation failed for {path:?}: {msg}")
            }
            Self::StorageError(path, msg) => {
                write!(f, "Storage operation failed for {path:?}: {msg}")
            }
            Self::Generic(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<IndexError> for Error {
    fn from(err: IndexError) -> Self {
        match err {
            IndexError::FileReadError(path, msg) => Error::parse(path.display().to_string(), msg),
            IndexError::UnsupportedLanguage(lang) => {
                Error::entity_extraction(format!("Unsupported language: {lang}"))
            }
            IndexError::ExtractionError(path, msg) => {
                Error::entity_extraction(format!("{path:?}: {msg}"))
            }
            IndexError::TransformationError(path, msg) => {
                Error::storage(format!("Transform {path:?}: {msg}"))
            }
            IndexError::StorageError(path, msg) => {
                Error::storage(format!("Storage {path:?}: {msg}"))
            }
            IndexError::Generic(msg) => Error::entity_extraction(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_stats_merge() {
        let mut stats1 = IndexStats {
            total_files: 10,
            failed_files: 1,
            entities_extracted: 50,
            relationships_extracted: 20,
            functions_indexed: 15,
            types_indexed: 10,
            variables_indexed: 25,
            processing_time_ms: 1000,
            memory_usage_bytes: Some(1024),
        };

        let stats2 = IndexStats {
            total_files: 5,
            failed_files: 0,
            entities_extracted: 25,
            relationships_extracted: 10,
            functions_indexed: 8,
            types_indexed: 5,
            variables_indexed: 12,
            processing_time_ms: 500,
            memory_usage_bytes: Some(2048),
        };

        stats1.merge(stats2);

        assert_eq!(stats1.total_files, 15);
        assert_eq!(stats1.failed_files, 1);
        assert_eq!(stats1.entities_extracted, 75);
        assert_eq!(stats1.relationships_extracted, 30);
        assert_eq!(stats1.functions_indexed, 23);
        assert_eq!(stats1.types_indexed, 15);
        assert_eq!(stats1.variables_indexed, 37);
        assert_eq!(stats1.processing_time_ms, 1500);
        assert_eq!(stats1.memory_usage_bytes, Some(2048));
    }

    #[test]
    fn test_diff_context() {
        let mut ctx = DiffContext::new();
        assert!(ctx.is_empty());
        assert_eq!(ctx.total_changes(), 0);

        ctx.added_files.push(PathBuf::from("new.rs"));
        ctx.modified_files.push(PathBuf::from("existing.rs"));

        assert!(!ctx.is_empty());
        assert_eq!(ctx.total_changes(), 2);
    }

    #[test]
    fn test_change_type() {
        assert_eq!(
            serde_json::to_string(&ChangeType::Created).unwrap(),
            "\"Created\""
        );
        assert_eq!(
            serde_json::to_string(&ChangeType::Updated).unwrap(),
            "\"Updated\""
        );
    }

    #[test]
    fn test_index_error_display() {
        let err =
            IndexError::FileReadError(PathBuf::from("test.rs"), "Permission denied".to_string());
        let display = format!("{err}");
        assert!(display.contains("test.rs"));
        assert!(display.contains("Permission denied"));
    }
}
