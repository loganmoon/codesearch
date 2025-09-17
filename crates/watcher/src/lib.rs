//! File system watching for real-time indexing
//!
//! This crate provides comprehensive file system monitoring with:
//! - Intelligent debouncing and event aggregation
//! - Git branch awareness and `.gitignore` support
//! - Cross-platform file watching with fallback mechanisms
//! - Language-specific file filtering
//! - Robust error recovery and resilience
//!
//! # Example
//!
//! ```no_run
//! use codesearch_watcher::{FileWatcher, WatcherConfig};
//! use std::path::PathBuf;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = WatcherConfig::default();
//! let mut watcher = FileWatcher::new(config)?;
//!
//! // Start watching a directory
//! let mut events = watcher.watch(PathBuf::from("/path/to/project")).await?;
//!
//! // Process events
//! while let Some(event) = events.recv().await {
//!     println!("File changed: {:?}", event);
//! }
//! # Ok(())
//! # }
//! ```

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

pub mod config;
pub mod debouncer;
pub mod events;
pub mod git;
pub mod ignore;
pub mod watcher;

// Re-export main types
pub use config::{BranchStrategy, FilterConfig, RecoveryConfig, WatcherConfig};
pub use debouncer::{BatchProcessor, EventDebouncer};
pub use events::{
    ChangeType, DebouncedEvent, DiffStats, EntityId, FileChange, FileMetadata, LineRange,
};
pub use git::{BranchChange, BranchWatcher, GitDetector, GitRepository};
pub use ignore::{CompositeLanguageFilter, IgnoreFilter, LanguageFilter};
pub use watcher::{FileWatcher, PollingWatcher};

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::config::{BranchStrategy, WatcherConfig};
    pub use crate::events::{FileChange, FileMetadata};
    pub use crate::watcher::FileWatcher;
}
