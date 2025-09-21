# Qdrant Storage Integration Implementation Plan

## Overview

Implement Qdrant vector database integration for the storage crate to enable semantic code search capabilities through vector embeddings, replacing the current mock storage client with a production-ready Qdrant client.

## Current State Analysis

The storage crate currently has only a minimal `StorageClient` trait with a single `bulk_load_entities()` method and a mock implementation (`crates/storage/src/lib.rs:10-14`). The embeddings crate is functional but returns f64 vectors when Qdrant requires f32 (`crates/embeddings/src/provider.rs:19`). Docker integration and health checking are fully implemented (`crates/cli/src/docker.rs:175-200`). The indexer has placeholder factory functions awaiting real storage implementation (`crates/indexer/src/repository_indexer.rs:373-377`).

## Desired End State

A fully functional Qdrant-backed storage system that:
- Stores code entities with their vector embeddings for semantic search
- Provides efficient bulk loading and similarity search capabilities
- Integrates seamlessly with the embeddings crate for vector generation
- Initializes collections dynamically based on embedding dimensions
- Supports filtering by entity type, language, and file path
- Handles concurrent operations safely with proper error recovery

### Key Discoveries:
- Entity IDs are deterministically generated using XxHash3_128 (`crates/core/src/entity_id.rs:79-105`)
- Configuration supports both gRPC (6334) and REST (6333) ports (`crates/core/src/config.rs:67-92`)
- Indexer processes files in batches of 100 for efficiency (`crates/indexer/src/repository_indexer.rs:90`)
- Docker compose already configured with persistent volumes (`docker-compose.yml:4-25`)
- Embeddings crate uses semaphore-based concurrency control (`crates/embeddings/src/embed_anything_provider.rs:128`)

## What We're NOT Doing

- Schema evolution or migration strategies for CodeEntity changes
- Supporting multiple embedding models simultaneously
- Implementing custom vector distance metrics beyond cosine similarity
- Creating a generic vector database abstraction for other backends
- Adding real-time incremental indexing (only batch indexing)
- Implementing vector quantization or compression optimizations

## Implementation Approach

Use a phased approach starting with core storage functionality, then integrating with embeddings, and finally connecting the complete pipeline through CLI commands. Each phase builds on the previous one with clear integration points.

## Phase 1: Storage Trait Separation and Core Implementation

### Overview
Separate collection management from CRUD operations by creating two focused traits and a coordination layer, following the codebase's established patterns (similar to EmbeddingManager/EmbeddingProvider separation).

### Changes Required:

#### 1. StorageClient Trait (CRUD Operations)
**File**: `crates/storage/src/lib.rs`
**Changes**: Extend minimal trait with CRUD operations only

```rust
// Lines 10-14: Replace current trait with CRUD-focused interface
#[async_trait]
pub trait StorageClient: Send + Sync {
    /// Bulk load entities with their embeddings
    async fn bulk_load_entities(&self, entities: Vec<CodeEntity>, embeddings: Vec<Vec<f32>>) -> Result<()>;

    /// Search for similar entities
    async fn search_similar(&self, query_embedding: Vec<f32>, limit: usize, filters: Option<SearchFilters>) -> Result<Vec<(CodeEntity, f32)>>;

    /// Get entity by ID
    async fn get_entity(&self, entity_id: &str) -> Result<Option<CodeEntity>>;
}

// Add SearchFilters struct for query filtering
pub struct SearchFilters {
    pub entity_type: Option<EntityType>,
    pub language: Option<String>,
    pub file_path: Option<PathBuf>,
}
```

#### 2. CollectionManager Trait (Lifecycle Operations)
**File**: `crates/storage/src/collection_manager.rs` (new file)
**Changes**: Create trait for collection lifecycle management

```rust
// New trait for collection management operations
#[async_trait]
pub trait CollectionManager: Send + Sync {
    /// Create or verify collection with specified dimensions
    async fn ensure_collection(&mut self, collection_name: &str, vector_dimensions: usize) -> Result<()>;

    /// Delete collection (for testing/reset)
    async fn delete_collection(&self, collection_name: &str) -> Result<()>;

    /// Check if collection exists
    async fn collection_exists(&self, collection_name: &str) -> Result<bool>;

    /// Health check for the storage backend
    async fn health_check(&self) -> Result<()>;
}
```

#### 3. CollectionManager Coordination Layer
**File**: `crates/storage/src/manager.rs` (new file)
**Changes**: Create manager following EmbeddingManager pattern

```rust
// Coordination layer that owns both client and collection manager
pub struct CollectionManager {
    client: Arc<dyn StorageClient>,
    collection_manager: Arc<dyn CollectionManager>,
    collection_name: String,
}

impl CollectionManager {
    /// Initialize from configuration
    pub async fn from_config(config: &StorageConfig) -> Result<Self> {
        // Create Qdrant connection
        // Initialize both client and collection manager
        // Return configured manager
    }

    /// Get the storage client for CRUD operations
    pub fn client(&self) -> Arc<dyn StorageClient> {
        Arc::clone(&self.client)
    }

    /// Initialize collection (called during codesearch init)
    pub async fn initialize_collection(&mut self, dimensions: usize) -> Result<()> {
        self.collection_manager.ensure_collection(&self.collection_name, dimensions).await
    }

    /// Health check delegating to collection manager
    pub async fn health_check(&self) -> Result<()> {
        self.collection_manager.health_check().await
    }
}
```

#### 4. Add Qdrant Client Dependency
**File**: `crates/storage/Cargo.toml`
**Changes**: Add qdrant-client and required dependencies

```toml
# Line 10: Add after existing dependencies
qdrant-client = "1.10"
tonic = "0.11"
uuid = { version = "1.0", features = ["v4", "serde"] }
```

#### 5. Implement Qdrant Storage Client
**File**: `crates/storage/src/qdrant/client.rs` (new file)
**Changes**: Implement StorageClient trait for CRUD operations

```rust
// QdrantStorageClient implements only CRUD operations
pub struct QdrantStorageClient {
    qdrant_client: QdrantClient,
    collection_name: String,
}

impl QdrantStorageClient {
    pub async fn new(connection: Arc<QdrantClient>, collection_name: String) -> Result<Self> {
        // Store shared connection and collection name
    }
}

#[async_trait]
impl StorageClient for QdrantStorageClient {
    // Implement bulk_load_entities, search_similar, get_entity
    // No collection management methods here
}
```

#### 6. Implement Qdrant Collection Manager
**File**: `crates/storage/src/qdrant/collection_manager.rs` (new file)
**Changes**: Implement CollectionManager trait for lifecycle operations

```rust
// QdrantCollectionManager handles collection lifecycle
pub struct QdrantCollectionManager {
    qdrant_client: Arc<QdrantClient>,
}

impl QdrantCollectionManager {
    pub async fn new(connection: Arc<QdrantClient>) -> Result<Self> {
        // Store shared connection
    }
}

#[async_trait]
impl CollectionManager for QdrantCollectionManager {
    // Implement ensure_collection, delete_collection, collection_exists, health_check
    // No data operations here
}
```

#### 7. Implement Qdrant Connection Factory
**File**: `crates/storage/src/qdrant/mod.rs` (new file)
**Changes**: Create shared connection and component factory

```rust
// Factory for creating Qdrant components with shared connection
pub async fn create_qdrant_components(config: &StorageConfig) -> Result<(Arc<QdrantClient>, String)> {
    // Create single QdrantClient connection
    // Generate collection name from repo path
    // Return shared connection and collection name
}
```

### Success Criteria:

#### Automated Verification:
- [x] Compilation succeeds: `cargo build -p codesearch-storage`
- [x] Unit tests pass: `cargo test -p codesearch-storage`
- [x] Clippy passes: `cargo clippy -p codesearch-storage -- -D warnings`
- [x] Traits can be mocked independently for testing

#### Manual Verification:
- [x] Can connect to running Qdrant instance
- [x] Collection manager can create collections with custom dimensions
- [x] Storage client operations work on existing collections
- [x] Search returns expected results
- [x] Indexer uses only StorageClient, not CollectionManager

---

## Phase 2: Embeddings Provider f32 Conversion

### Overview
Update the embeddings crate to return f32 vectors directly instead of f64, eliminating unnecessary conversions and matching Qdrant's native format.

### Changes Required:

#### 1. Update EmbeddingProvider Trait
**File**: `crates/embeddings/src/provider.rs`
**Changes**: Change return type from f64 to f32

```rust
// Line 19: Update return type
async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;
```

#### 2. Remove f64 Conversion in EmbedAnythingProvider
**File**: `crates/embeddings/src/embed_anything_provider.rs`
**Changes**: Remove conversion logic at lines 191-194

```rust
// Lines 191-194: Remove conversion, return f32 directly
embeddings.push(dense_vec); // dense_vec is already Vec<f32>
```

#### 3. Update Manager Delegation
**File**: `crates/embeddings/src/lib.rs`
**Changes**: Update manager's embed method signature

```rust
// Line 58-60: Update to match new f32 return type
pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>
```

### Success Criteria:

#### Automated Verification:
- [x] Embeddings tests pass: `cargo test -p codesearch-embeddings`
- [x] Type checking passes: `cargo check --workspace`
- [x] No f64 vectors in embeddings output

#### Manual Verification:
- [x] Embedding generation produces valid f32 vectors
- [x] Vector dimensions match model configuration

---

## Phase 3: Collection Name Generation

### Overview
Implement collection naming based on repository absolute path to ensure unique collections per repository.

### Changes Required:

#### 1. Add Collection Name Generator
**File**: `crates/core/src/config.rs`
**Changes**: Add function to generate collection names from paths

```rust
// After StorageConfig definition, add:
impl StorageConfig {
    pub fn generate_collection_name(repo_path: &Path) -> String {
        // Use full absolute path as collection name
        // Sanitize for Qdrant requirements (alphanumeric, underscore, dash)
    }
}
```

#### 2. Remove Default Collection Name
**File**: `crates/core/src/config.rs`
**Changes**: Make collection_name required without default

```rust
// Line 80: Remove default annotation
pub collection_name: String, // No default - must be set explicitly
```

### Success Criteria:

#### Automated Verification:
- [ ] Unit tests for name generation: `cargo test -p codesearch-core test_collection_name`
- [ ] Config validation rejects missing collection names

#### Manual Verification:
- [ ] Different repositories get unique collection names
- [ ] Collection names are valid for Qdrant
- [ ] Names are deterministic for same path

---

## Phase 4: CLI Init Command Completion

### Overview
Complete the init command to create collections using the CollectionManager, properly separating collection management from CRUD operations.

### Changes Required:

#### 1. Complete Init Command Implementation
**File**: `crates/cli/src/main.rs`
**Changes**: Replace TODO at line 178 with full implementation

```rust
// Lines 174-179: Replace TODO with:
// 1. Start dependencies if needed
// 2. Create embedding manager from config
// 3. Create CollectionManager from config (not just client)
// 4. Get dimensions from embedding provider
// 5. Use CollectionManager to initialize collection with dimensions
// 6. Perform health check via CollectionManager
// 7. Save updated config with collection name
```

#### 2. Add Storage Initialization Helper
**File**: `crates/cli/src/storage_init.rs` (new file)
**Changes**: Create helper module for storage initialization

```rust
// Helper functions for:
// - Creating CollectionManager with retries
// - Collection initialization via manager.initialize_collection()
// - Health checking via manager.health_check()
// - Proper separation of management vs CRUD operations
```

### Success Criteria:

#### Automated Verification:
- [x] Init command compiles: `cargo build --bin codesearch`
- [x] Integration test passes: `cargo test --test init_integration`

#### Manual Verification:
- [x] Running `codesearch init` creates collection in Qdrant
- [x] Collection has correct vector dimensions
- [x] Config file updated with collection name
- [x] Subsequent init calls handle existing collections gracefully

---

## Phase 5: Indexer Storage Integration

### Overview
Update the indexer to use only the StorageClient trait for CRUD operations, without any collection management responsibilities.

### Changes Required:

#### 1. Update Storage Client Factory
**File**: `crates/indexer/src/repository_indexer.rs`
**Changes**: Replace mock at lines 373-377

```rust
// Lines 373-377: Replace with:
fn create_storage_client(host: String, port: u16, collection_name: String) -> impl StorageClient {
    // Create connection to Qdrant
    // Create QdrantStorageClient (NOT CollectionManager)
    // Return client that assumes collection already exists
    // No collection management here - that's handled by CLI init
}
```

#### 2. Add Embedding Generation to Pipeline
**File**: `crates/indexer/src/repository_indexer.rs`
**Changes**: Integrate embeddings in process_batch method

```rust
// After line 172: Add embedding generation
// 1. Extract content from entities
// 2. Call embedding_manager.embed()
// 3. Pass both entities and embeddings to bulk_load_entities()
```

#### 3. Update Indexer Dependencies
**File**: `crates/indexer/Cargo.toml`
**Changes**: Ensure embeddings crate is available

```rust
// Already present at line 15, but ensure it's used
codesearch-embeddings = { path = "../embeddings" }
```

### Success Criteria:

#### Automated Verification:
- [ ] Indexer tests pass: `cargo test -p codesearch-indexer`
- [ ] Integration test with real storage: `cargo test --test indexer_integration`

#### Manual Verification:
- [ ] Files are indexed with embeddings
- [ ] Entities appear in Qdrant with vectors
- [ ] Batch processing handles large repositories
- [ ] Error recovery works for failed batches

---

## Phase 6: Complete Index and Search Commands

### Overview
Implement the index and search CLI commands using the appropriate components - CollectionManager for initialization checks, StorageClient for operations.

### Changes Required:

#### 1. Implement Index Command
**File**: `crates/cli/src/main.rs`
**Changes**: Replace TODO at line 247

```rust
// Lines 245-248: Implement full indexing
// 1. Ensure dependencies running
// 2. Create CollectionManager from config
// 3. Verify collection exists via manager (fail if not initialized)
// 4. Get storage client from manager for indexer
// 5. Create embedding manager
// 6. Create indexer with storage client (not manager)
// 7. Run indexing with progress tracking
// 8. Report statistics
```

#### 2. Implement Search Command
**File**: `crates/cli/src/main.rs`
**Changes**: Replace TODO at line 254

```rust
// Lines 251-255: Implement search
// 1. Create CollectionManager from config
// 2. Get storage client from manager
// 3. Create embedding manager
// 4. Generate query embedding
// 5. Use client.search_similar() for search
// 6. Format and display results
```

### Success Criteria:

#### Automated Verification:
- [ ] CLI commands compile: `cargo build --bin codesearch`
- [ ] End-to-end test passes: `cargo test --test e2e_test`

#### Manual Verification:
- [ ] `codesearch index` processes entire repository
- [ ] Progress bar shows accurate progress
- [ ] `codesearch search "query"` returns relevant results
- [ ] Search results include similarity scores
- [ ] Results are properly formatted and readable

---

## Testing Strategy

### Unit Tests:
- StorageClient CRUD operations with mock collection
- CollectionManager lifecycle operations independently
- Point conversion from CodeEntity to Qdrant format
- Collection name generation from paths
- Search filter construction
- Error handling for network failures
- Verify trait separation (client can't manage collections)

### Integration Tests:
- Full indexing pipeline with temporary Qdrant container
- Search accuracy with known code samples
- Batch processing with various file sizes
- Concurrent indexing operations
- Recovery from partial failures

### Manual Testing Steps:
1. Initialize repository with `codesearch init`
2. Verify Qdrant collection created with correct dimensions
3. Run `codesearch index --progress` and monitor completion
4. Search for known functions/classes
5. Test filter combinations (language, file path, entity type)
6. Verify Docker container management (start/stop/status)
7. Test with large repository (1000+ files)
8. Verify memory usage stays reasonable during batch processing

## Performance Considerations

- Use `upsert_points_chunked` for automatic batch optimization
- Implement connection pooling for concurrent operations
- Cache embedding model in memory to avoid reload
- Use streaming for large result sets
- Consider adding payload indexes for common filter fields
- Monitor Qdrant memory usage and adjust batch sizes accordingly

## Migration Notes

For existing users with mock storage:
- First run will create new Qdrant collection via CollectionManager
- Collection management now separate from CRUD operations
- Indexer no longer responsible for collection initialization
- Config file will be updated with collection name
- Docker compose will start Qdrant automatically if configured

## Architecture Benefits

This separation of concerns provides:
- **Testing**: Each trait can be mocked independently
- **Clarity**: Indexer only needs StorageClient, not management operations
- **Flexibility**: Can swap storage backends without changing collection management
- **Safety**: CRUD operations can't accidentally delete/modify collections
- **Consistency**: Follows established patterns (EmbeddingManager/Provider)

## References

- Original issue: GitHub issue pending
- Qdrant client docs: https://github.com/qdrant/rust-client