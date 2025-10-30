//! Neo4j graph database client for code relationship storage

mod client;
pub mod mock;
pub(crate) mod relationship_builder;
mod traits;

// Public exports: trait and mock for external use
pub use mock::MockNeo4jClient;
pub use traits::Neo4jClientTrait;

// Internal use only: concrete client implementation
pub(crate) use client::Neo4jClient;

// Public export: security-critical constants for documentation
pub use client::ALLOWED_RELATIONSHIP_TYPES;
