#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

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

// Private implementation modules
mod config;
mod debouncer;
mod events;
mod git;
mod ignore;
mod watcher;

// Public exports - minimal API surface
pub use config::WatcherConfig;
pub use events::{DiffStats, FileChange, FileMetadata};
pub use git::{FileDiffChangeType, FileDiffStatus, GitRepository};
pub use watcher::FileWatcher;

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::config::WatcherConfig;
    pub use crate::events::FileChange;
    pub use crate::watcher::FileWatcher;
}
