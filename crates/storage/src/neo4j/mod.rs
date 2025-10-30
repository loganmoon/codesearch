//! Neo4j graph database client for code relationship storage

pub(crate) mod client;
pub(crate) mod relationship_builder;

// Public exports: client type and security-critical constants
pub use client::{Neo4jClient, ALLOWED_RELATIONSHIP_TYPES};
