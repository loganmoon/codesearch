//! Core types and traits for the Code Context semantic indexing system
//!
//! This crate provides the foundational abstractions used throughout the
//! codesearch system, including:
//!
//! - **Entities**: Code entities like functions, classes, and modules
//! - **Chunks**: Semantic units of code with metadata and embeddings
//! - **Traits**: Core traits for language-specific implementations
//! - **Configuration**: System configuration management
//! - **Error handling**: Unified error types
//!

pub mod config;
pub mod entities;
pub mod entity_id;
pub mod error;

// Re-export main types for convenience
pub use config::{Config, EmbeddingsConfig, IndexerConfig, StorageConfig, WatcherConfig};
pub use entities::{
    CodeEntity, CodeRelationship, EntityType, FunctionSignature, InternedString, Language,
    RelationshipType, SourceLocation, Visibility,
};
pub use entity_id::{generate_entity_id, generate_entity_id_with_separator, ScopeContext};
pub use error::{Error, Result, ResultExt};

/// Version of the core library
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::config::Config;
    pub use crate::entities::{CodeEntity, EntityType};
    pub use crate::error::{Result, ResultExt};
}
