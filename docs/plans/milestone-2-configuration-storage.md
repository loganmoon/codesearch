# Milestone 2: Configuration & Storage Implementation Plan

## Overview

This plan implements core configuration handling and the Qdrant storage backend with a trait-based abstraction layer. The implementation will establish the configuration system and vector database storage layer with proper abstraction, ensuring no implementation details leak through the public API.

## Current State Analysis

The codesearch codebase has a comprehensive configuration infrastructure but minimal actual configuration for storage. The storage layer has clean trait abstractions defined but only mock implementations. Docker infrastructure for Qdrant is ready but not integrated with the application code.

### Key Discoveries:
- Configuration system uses `config` crate with TOML format and environment variable overrides at `crates/core/src/config.rs:187-215`
- Storage traits defined at `crates/storage/src/lib.rs:11-31` with `StorageClient` and `StorageManager`
- Mock storage client exists at `crates/storage/src/lib.rs:64-114`
- Docker Compose setup ready for Qdrant at `docker-compose.yml:2-20`
- Indexer uses factory function pattern at `crates/indexer/src/repository_indexer.rs:439`
- `StorageConfig` struct is empty at `crates/core/src/config.rs:67-68`

## Desired End State

After implementation:
- Full configuration support for Qdrant connection parameters and storage settings
- Production-ready Qdrant client implementing all storage traits
- Factory pattern hiding all Qdrant-specific implementation details
- Integration tests passing against live Qdrant instance
- No Qdrant types exposed in public API

### Verification:
- Configuration loads from `.codesearch/config.toml` and environment variables
- Storage client successfully connects to Qdrant and manages collections
- Bulk loading processes 1000+ entities efficiently
- Vector similarity search returns results (with random vectors for now)

## What We're NOT Doing

- Not implementing actual embedding generation (vectors will be random f32 for testing)
- Not implementing search ranking algorithms beyond Qdrant's built-in similarity
- Not implementing data migration from other storage backends
- Not adding authentication/authorization layers
- Not implementing distributed/clustered Qdrant setup
- Not implementing actual semantic search (requires embeddings from future milestone)

## Implementation Approach

Follow a layered approach: first establish configuration, then refine traits, implement Qdrant backend privately, expose via factory pattern, and thoroughly test each layer.

## Phase 1: Configuration Implementation

### Overview
Establish complete configuration structure for storage settings with validation.

### Changes Required:

#### 1. Extend StorageConfig Structure
**File**: `crates/core/src/config.rs`
**Changes**: Add fields to the empty `StorageConfig` struct (lines 67-68)

```rust
// Add these fields to StorageConfig:
- provider: String (default: "qdrant")
- host: String (default: "localhost")
- port: u16 (default: 6334 for gRPC)
- api_key: Option<String> (for cloud Qdrant)
- collection_name: String (default: "codesearch")
- vector_size: usize (default: 768 for all-minilm-l6-v2)
- distance_metric: String (default: "cosine")
- batch_size: usize (default: 100)
- timeout_ms: u64 (default: 30000)
- use_mock: bool (default: false, for testing)
```

#### 2. Create Configuration File Template
**File**: `.codesearch/config.toml` (new)
**Changes**: Create example configuration with documentation

```toml
# Storage backend configuration
[storage]
provider = "qdrant"
host = "localhost"
port = 6334
# api_key = "your-api-key"  # Optional, for Qdrant Cloud
collection_name = "codesearch"
vector_size = 768  # Must match embedding model dimensions
distance_metric = "cosine"  # Options: cosine, euclidean, dot
batch_size = 100
timeout_ms = 30000
use_mock = false  # Set to true for testing without Qdrant
```

#### 3. Add Configuration Validation
**File**: `crates/core/src/config.rs`
**Changes**: Extend `Config::validate()` method (lines 223-243)

```rust
// Add storage configuration validation:
- Check valid storage providers: ["qdrant", "mock"]
- Validate vector_size > 0 and reasonable (e.g., <= 4096)
- Validate distance_metric in ["cosine", "euclidean", "dot"]
- Validate batch_size > 0 and reasonable (e.g., <= 1000)
- Validate port is valid network port
```

### Success Criteria:

#### Automated Verification:
- [ ] Configuration loads from file: `cargo test --package codesearch-core test_config_from_file`
- [ ] Environment variables override: `CODESEARCH_STORAGE__HOST=qdrant cargo test`
- [ ] Validation rejects invalid settings: `cargo test test_config_validation`

#### Manual Verification:
- [ ] Config file is well-documented and understandable
- [ ] Invalid configurations produce helpful error messages

---

## Phase 2: Storage Trait Enhancement

### Overview
Extend storage traits to support search operations while maintaining abstraction.

### Changes Required:

#### 1. Add Search Methods to StorageClient
**File**: `crates/storage/src/lib.rs`
**Changes**: Extend `StorageClient` trait (lines 11-24)

```rust
// Add these methods to StorageClient trait:
async fn search_similar(
    &self,
    query_vector: Vec<f32>,
    limit: usize,
    score_threshold: Option<f32>,
) -> Result<Vec<ScoredEntity>, Error>;

async fn get_entity_by_id(&self, id: &str) -> Result<Option<StorageEntity>, Error>;

async fn get_entities_by_ids(&self, ids: &[String]) -> Result<Vec<StorageEntity>, Error>;
```

#### 2. Define Search Result Types
**File**: `crates/storage/src/lib.rs`
**Changes**: Add new types after `StorageEntity` (after line 45)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredEntity {
    pub entity: StorageEntity,
    pub score: f32,
}
```

#### 3. Create Storage-Specific Error Types
**File**: `crates/storage/src/error.rs` (new)
**Changes**: Define storage error variants and map to core::Error

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    #[error("Batch size exceeded: requested {requested}, max {max}")]
    BatchSizeExceeded { requested: usize, max: usize },

    #[error("Invalid vector dimensions: expected {expected}, got {actual}")]
    InvalidDimensions { expected: usize, actual: usize },

    #[error("Operation timeout after {0}ms")]
    Timeout(u64),
}
```

### Success Criteria:

#### Automated Verification:
- [ ] Trait definitions compile: `cargo check --package codesearch-storage`
- [ ] Mock implementation updated: `cargo test --package codesearch-storage`

#### Manual Verification:
- [ ] Trait methods are well-documented with clear semantics
- [ ] Error types cover all expected failure modes

---

## Phase 3: Qdrant Backend Implementation

### Overview
Implement Qdrant client in private modules, keeping all implementation details hidden.

### Changes Required:

#### 1. Add Qdrant Client Dependency
**File**: `crates/storage/Cargo.toml`
**Changes**: Add dependency (after line 10)

```toml
qdrant-client = "1.12"
tokio = { version = "1.42", features = ["sync"] }
```

#### 2. Create Qdrant Module Structure
**File**: `crates/storage/src/qdrant/mod.rs` (new)
**Changes**: Main Qdrant client implementation

```rust
// Private module - not exposed in public API
use qdrant_client::prelude::*;
use crate::{StorageClient, StorageManager, StorageEntity, ScoredEntity};

pub(crate) struct QdrantStorage {
    client: QdrantClient,
    config: StorageConfig,
}

impl QdrantStorage {
    pub(crate) async fn new(config: StorageConfig) -> Result<Self, Error> {
        // Initialize Qdrant client with config
        // Set up connection with retries
        // Verify connection is alive
    }
}
```

#### 3. Implement Collection Management
**File**: `crates/storage/src/qdrant/collections.rs` (new)
**Changes**: Collection operations

```rust
// Implementation of StorageManager trait for QdrantStorage
// Handle collection creation with proper vector configuration
// Map distance metrics from config to Qdrant types
// Set up proper indexing parameters
```

#### 4. Implement Bulk Operations
**File**: `crates/storage/src/qdrant/operations.rs` (new)
**Changes**: Data operations and conversions

```rust
// Convert CodeEntity to Qdrant PointStruct
// Handle batching for large datasets
// Map entity fields to payload
// Implement upsert with proper error handling
```

#### 5. Implement Search Operations
**File**: `crates/storage/src/qdrant/search.rs` (new)
**Changes**: Vector similarity search

```rust
// Implement search_similar with score threshold
// Convert Qdrant search results to ScoredEntity
// Handle get_entity_by_id using Qdrant's retrieve API
```

### Success Criteria:

#### Automated Verification:
- [ ] Qdrant module compiles: `cargo build --package codesearch-storage`
- [ ] Unit tests pass: `cargo test --package codesearch-storage --lib`
- [ ] No Qdrant types in public API: Check with `cargo doc`

#### Manual Verification:
- [ ] Connection handling is robust with retries
- [ ] Error messages are informative and actionable

---

## Phase 4: Factory Pattern Implementation

### Overview
Create public factory function that hides implementation selection.

### Changes Required:

#### 1. Create Factory Module
**File**: `crates/storage/src/factory.rs` (new)
**Changes**: Public factory function

```rust
use crate::{StorageClient, StorageManager};
use codesearch_core::config::StorageConfig;
use std::sync::Arc;

/// Creates a storage client based on configuration
/// Returns trait objects, hiding implementation details
pub async fn create_storage_client(
    config: StorageConfig,
) -> Result<Arc<dyn StorageClient + StorageManager>, Error> {
    match config.provider.as_str() {
        "qdrant" if !config.use_mock => {
            // Create QdrantStorage (private type)
            // Wrap in Arc and return as trait object
        }
        "mock" | _ => {
            // Return MockStorageClient for testing
        }
    }
}
```

#### 2. Update Public Exports
**File**: `crates/storage/src/lib.rs`
**Changes**: Export factory function (line 1)

```rust
mod factory;
pub use factory::create_storage_client;

// Keep qdrant module private
mod qdrant;
```

#### 3. Update Indexer Integration
**File**: `crates/indexer/src/repository_indexer.rs`
**Changes**: Use factory instead of mock (line 439)

```rust
// Replace current mock implementation with:
use codesearch_storage::create_storage_client;

async fn create_storage_client(config: StorageConfig) -> Result<impl StorageClient> {
    create_storage_client(config).await
}
```

### Success Criteria:

#### Automated Verification:
- [ ] Factory creates correct implementation: `cargo test test_factory`
- [ ] Indexer compiles with new factory: `cargo build --package codesearch-indexer`

#### Manual Verification:
- [ ] Public API contains no Qdrant-specific types
- [ ] Documentation clearly explains factory usage

---

## Phase 5: Testing Implementation

### Overview
Comprehensive testing at unit, integration, and system levels.

### Changes Required:

#### 1. Unit Tests for Conversions
**File**: `crates/storage/src/qdrant/tests/conversions.rs` (new)
**Changes**: Test data transformations

```rust
#[cfg(test)]
mod tests {
    // Test CodeEntity -> PointStruct conversion
    // Test payload field mapping
    // Test vector dimension validation
    // Test batch chunking logic
}
```

#### 2. Integration Tests with Live Qdrant
**File**: `crates/storage/tests/integration_test.rs` (new)
**Changes**: Tests requiring Qdrant instance

```rust
#[tokio::test]
#[ignore] // Run with --ignored when Qdrant is available
async fn test_collection_lifecycle() {
    // Create client with test config
    // Create collection
    // Verify collection exists
    // Delete collection
    // Verify collection gone
}

#[tokio::test]
#[ignore]
async fn test_bulk_load_and_search() {
    // Create collection
    // Generate 1000 test entities with random vectors
    // Bulk load entities
    // Search with random query vector
    // Verify results returned
}
```

#### 3. Docker-Based Integration Tests
**File**: `crates/storage/tests/docker_test.rs` (new)
**Changes**: Automated tests with Docker

```rust
#[tokio::test]
async fn test_with_docker_qdrant() {
    // Check if Docker is available
    // Start Qdrant using docker-compose
    // Wait for health check
    // Run integration tests
    // Clean up
}
```

#### 4. End-to-End Test Helper
**File**: `crates/storage/tests/helpers/mod.rs` (new)
**Changes**: Test utilities

```rust
// Helper to generate test entities with random vectors
pub fn generate_test_entities(count: usize, vector_size: usize) -> Vec<CodeEntity> {
    // Create realistic test entities
    // Add random vectors of correct dimension
}

// Helper to wait for Qdrant availability
pub async fn wait_for_qdrant(host: &str, port: u16, timeout: Duration) -> Result<()> {
    // Poll connection until ready or timeout
}
```

### Success Criteria:

#### Automated Verification:
- [ ] All unit tests pass: `cargo test --package codesearch-storage --lib`
- [ ] Integration tests pass with Qdrant: `docker compose up -d && cargo test --package codesearch-storage -- --ignored`
- [ ] No test failures in CI: `cargo test --workspace`
- [ ] Clippy passes: `cargo clippy --workspace -- -D warnings`

#### Manual Verification:
- [ ] Tests are readable and maintainable
- [ ] Test coverage includes error cases
- [ ] Performance with 1000+ entities is acceptable
- [ ] Docker tests clean up properly

---

## Testing Strategy

### Unit Tests:
- Test all type conversions and mappings
- Test error handling for invalid inputs
- Mock external dependencies

### Integration Tests:
- Test against live Qdrant instance
- Verify collection management
- Test bulk operations with realistic data volumes
- Verify search functionality

### Manual Testing Steps:
1. Start Qdrant: `docker compose up -d qdrant`
2. Run configuration test: `CODESEARCH_STORAGE__HOST=localhost cargo run -- index`
3. Verify collection created in Qdrant UI: `http://localhost:6333/dashboard`
4. Load test data and verify search works
5. Test error handling by stopping Qdrant mid-operation

## Performance Considerations

- Batch size tuning for optimal throughput (default 100, configurable)
- Connection pooling for concurrent operations
- Async/await for non-blocking I/O
- Retry logic with exponential backoff for transient failures

## Migration Notes

Since this is the initial storage implementation, no migration is needed. Future storage backend changes should:
- Maintain the same trait interface
- Support data export/import if switching providers
- Version the collection schema for future updates

## References

- Original issue: GitHub Issue #2
- Qdrant documentation: https://qdrant.tech/documentation/
- Docker Compose setup: `docker-compose.yml:2-20`
- Current mock implementation: `crates/storage/src/lib.rs:64-114`
- Indexer integration point: `crates/indexer/src/repository_indexer.rs:439`