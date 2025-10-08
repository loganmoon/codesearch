//! File change event types and metadata
//!
//! This module defines immutable event types for file system changes
//! with comprehensive metadata for change detection and processing.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

/// Represents a file system change event with metadata
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileChange {
    /// File was created
    Created(PathBuf, FileMetadata),
    /// File was modified
    Modified(PathBuf, DiffStats),
    /// File was deleted
    Deleted(PathBuf),
    /// File was renamed
    Renamed { from: PathBuf, to: PathBuf },
    /// File permissions changed
    PermissionsChanged(PathBuf),
}

impl FileChange {
    /// Get the primary path associated with this change
    pub fn path(&self) -> &PathBuf {
        match self {
            Self::Created(path, _) => path,
            Self::Modified(path, _) => path,
            Self::Deleted(path) => path,
            Self::Renamed { to, .. } => to,
            Self::PermissionsChanged(path) => path,
        }
    }

    /// Get the event priority for ordering (lower is higher priority)
    pub fn priority(&self) -> u8 {
        match self {
            Self::Deleted(_) => 0,            // Highest priority
            Self::Modified(_, _) => 1,        // Medium priority
            Self::Renamed { .. } => 2,        // Medium priority
            Self::Created(_, _) => 3,         // Lower priority
            Self::PermissionsChanged(_) => 4, // Lowest priority
        }
    }

    /// Check if this is a structural change (create/delete/rename)
    pub fn is_structural(&self) -> bool {
        matches!(
            self,
            Self::Created(_, _) | Self::Deleted(_) | Self::Renamed { .. }
        )
    }
}

/// Immutable file metadata
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMetadata {
    /// File size in bytes
    pub size: u64,
    /// Last modified time
    pub modified: SystemTime,
    /// File permissions (Unix-style)
    pub permissions: u32,
    /// Whether the file is read-only
    pub readonly: bool,
    /// Whether this is a symlink
    pub is_symlink: bool,
}

impl FileMetadata {
    /// Create new file metadata
    pub fn new(size: u64, modified: SystemTime, permissions: u32) -> Self {
        Self {
            size,
            modified,
            permissions,
            readonly: permissions & 0o200 == 0,
            is_symlink: false,
        }
    }

    /// Create metadata for a symlink
    pub fn symlink(size: u64, modified: SystemTime) -> Self {
        Self {
            size,
            modified,
            permissions: 0o777,
            readonly: false,
            is_symlink: true,
        }
    }
}

/// Statistics about file changes
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffStats {
    /// Lines that were added
    pub added_lines: Arc<Vec<LineRange>>,
    /// Lines that were removed
    pub removed_lines: Arc<Vec<LineRange>>,
    /// Entities that were modified
    pub modified_entities: Arc<Vec<EntityId>>,
    /// Type of change
    pub change_type: ChangeType,
}

impl DiffStats {
    /// Create new diff statistics
    pub fn new(
        added_lines: Vec<LineRange>,
        removed_lines: Vec<LineRange>,
        modified_entities: Vec<EntityId>,
    ) -> Self {
        let change_type = Self::classify_change(&added_lines, &removed_lines);
        Self {
            added_lines: Arc::new(added_lines),
            removed_lines: Arc::new(removed_lines),
            modified_entities: Arc::new(modified_entities),
            change_type,
        }
    }

    /// Classify the type of change based on diff statistics
    fn classify_change(added: &[LineRange], removed: &[LineRange]) -> ChangeType {
        let total_added: usize = added.iter().map(|r| r.count()).sum();
        let total_removed: usize = removed.iter().map(|r| r.count()).sum();
        let total_changed = total_added + total_removed;

        if total_changed > 100 {
            ChangeType::Structural
        } else if total_changed > 20 {
            ChangeType::Major
        } else {
            ChangeType::Minor
        }
    }

    /// Check if this represents a significant change
    pub fn is_significant(&self) -> bool {
        !matches!(self.change_type, ChangeType::Minor)
    }
}

/// Range of lines in a file
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LineRange {
    /// Starting line number (1-indexed)
    pub start: usize,
    /// Ending line number (inclusive, 1-indexed)
    pub end: usize,
}

impl LineRange {
    /// Create a new line range
    pub fn new(start: usize, end: usize) -> Self {
        debug_assert!(start > 0, "Line numbers are 1-indexed");
        debug_assert!(end >= start, "End must be >= start");
        Self { start, end }
    }

    /// Get the number of lines in this range
    pub fn count(&self) -> usize {
        self.end - self.start + 1
    }

    /// Check if this range contains a line number
    pub fn contains(&self, line: usize) -> bool {
        line >= self.start && line <= self.end
    }

    /// Check if two ranges overlap
    pub fn overlaps(&self, other: &Self) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

/// Identifier for a code entity
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityId {
    /// Name of the entity
    pub name: String,
    /// Type of entity (function, class, etc.)
    pub entity_type: String,
    /// File path where entity is defined
    pub file: PathBuf,
    /// Line range of the entity
    pub range: LineRange,
}

impl EntityId {
    /// Create a new entity identifier
    pub fn new(name: String, entity_type: String, file: PathBuf, range: LineRange) -> Self {
        Self {
            name,
            entity_type,
            file,
            range,
        }
    }
}

/// Type of change based on impact
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeType {
    /// Small changes (< 20 lines)
    Minor,
    /// Medium changes (20-100 lines)
    Major,
    /// Large structural changes (> 100 lines)
    Structural,
}

impl ChangeType {
    /// Get the priority of this change type (lower is higher priority)
    pub fn priority(&self) -> u8 {
        match self {
            Self::Structural => 0,
            Self::Major => 1,
            Self::Minor => 2,
        }
    }
}

/// Event with debounce metadata
#[derive(Debug, Clone)]
pub struct DebouncedEvent {
    /// The file change event
    pub event: FileChange,
    /// When the event was first detected
    pub first_seen: SystemTime,
    /// When the event was last updated
    pub last_updated: SystemTime,
    /// Number of times this event was aggregated
    pub occurrence_count: u32,
}

impl DebouncedEvent {
    /// Create a new debounced event
    pub fn new(event: FileChange) -> Self {
        let now = SystemTime::now();
        Self {
            event,
            first_seen: now,
            last_updated: now,
            occurrence_count: 1,
        }
    }

    /// Update the event with a new occurrence
    pub fn update(&mut self, event: FileChange) {
        // Keep Created events when followed by Modify
        // (common pattern when files are created and immediately written to)
        match (&self.event, &event) {
            (FileChange::Created(path1, _), FileChange::Modified(path2, _)) if path1 == path2 => {
                // Keep the Created event, just update the timestamp
            }
            _ => {
                // Otherwise update to the new event
                self.event = event;
            }
        }
        self.last_updated = SystemTime::now();
        self.occurrence_count += 1;
    }

    /// Get the age of this event since first seen
    pub fn age(&self) -> std::time::Duration {
        SystemTime::now()
            .duration_since(self.first_seen)
            .unwrap_or_default()
    }

    /// Get the time since last update
    pub fn time_since_update(&self) -> std::time::Duration {
        SystemTime::now()
            .duration_since(self.last_updated)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_change_priority() {
        let delete = FileChange::Deleted(PathBuf::from("test.rs"));
        let modify = FileChange::Modified(
            PathBuf::from("test.rs"),
            DiffStats::new(vec![], vec![], vec![]),
        );
        let create = FileChange::Created(
            PathBuf::from("test.rs"),
            FileMetadata::new(100, SystemTime::now(), 0o644),
        );

        assert!(delete.priority() < modify.priority());
        assert!(modify.priority() < create.priority());
    }

    #[test]
    fn test_line_range() {
        let range1 = LineRange::new(10, 20);
        let range2 = LineRange::new(15, 25);
        let range3 = LineRange::new(30, 40);

        assert_eq!(range1.count(), 11);
        assert!(range1.contains(15));
        assert!(!range1.contains(25));
        assert!(range1.overlaps(&range2));
        assert!(!range1.overlaps(&range3));
    }

    #[test]
    fn test_change_type_classification() {
        let minor = DiffStats::new(
            vec![LineRange::new(1, 5)],
            vec![LineRange::new(10, 12)],
            vec![],
        );
        assert_eq!(minor.change_type, ChangeType::Minor);

        let major = DiffStats::new(
            vec![LineRange::new(1, 30)],
            vec![LineRange::new(40, 50)],
            vec![],
        );
        assert_eq!(major.change_type, ChangeType::Major);

        let structural = DiffStats::new(
            vec![LineRange::new(1, 100)],
            vec![LineRange::new(200, 250)],
            vec![],
        );
        assert_eq!(structural.change_type, ChangeType::Structural);
    }

    #[test]
    fn test_file_metadata_readonly() {
        let writable = FileMetadata::new(100, SystemTime::now(), 0o644);
        assert!(!writable.readonly);

        let readonly = FileMetadata::new(100, SystemTime::now(), 0o444);
        assert!(readonly.readonly);
    }

    #[test]
    fn test_debounced_event() {
        let event = FileChange::Created(
            PathBuf::from("test.rs"),
            FileMetadata::new(100, SystemTime::now(), 0o644),
        );
        let mut debounced = DebouncedEvent::new(event.clone());

        assert_eq!(debounced.occurrence_count, 1);

        std::thread::sleep(std::time::Duration::from_millis(10));
        debounced.update(event);

        assert_eq!(debounced.occurrence_count, 2);
        assert!(debounced.time_since_update() < std::time::Duration::from_millis(100));
    }
}
