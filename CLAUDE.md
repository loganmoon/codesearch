# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## PROJECT OVERVIEW

Codesearch is a Rust-based semantic code indexing system that provides intelligent code search through AST-based code graph extraction, local/remote embeddings, and real-time file watching with MCP server integration.

# Rust Development Practices

## Architecture Principles

  - Design narrow, abstract public APIs centered around traits
  - Limit public exports to traits, models, errors, and factory functions
  - Keep implementation details in private modules
  - Use core domain types directly (`CodeEntity` from `code-context-core`)
  - Implement From/Into traits for API boundary conversions
  - Never assume that backwards compatibility is required, unless specifically requested by the user

## Code Quality Standards

  - Return Result types - never panic with .unwrap() or .expect() except in tests
  - Use core::Error for all error types
  - Enforce `#![deny(warnings)]`, `#![deny(clippy::unwrap_used)]`, `#![deny(clippy::expect_used)]` ONLY in non-test code
  - VERY strongly favor immutability and borrowing over cloning. Favor builders over `new` or direct struct initialization
  - VERY strongly prefer standalone functions over unnecessary &self methods
  - Implement RAII for resource management

## Avoid These Patterns

  - Excessive `Box`/`Pin`/`Arc` wrapping when simpler ownership suffices
  - Global state, e.g., with `OnceLock<Mutex<HashMap<>>>`
  - Mixed responsibilities in single modules
  - Redundant allocations during type conversions
  - Ignoring compiler warnings or clippy lints

## Style Rules
  - When formatting strings use this syntax: println!("The thing is {thing}");

## CRATE ARCHITECTURE

This is a workspace with these crates:
- **core**: Foundation types, entities, configuration, error handling
- **languages**: AST parsing and entity extraction. Fully implemented for Rust and JavaScript/TypeScript with complete AST parsing and entity extraction. Python and Go have partial infrastructure (dependencies, type system, file filtering) but no actual parsing implementation.
- **embeddings**: Vector embedding providers and local/remote embedding generation
- **indexer**: Repository indexing logic with Git integration
- **watcher**: Real-time file system monitoring with ignore patterns
- **storage**: Persistent storage layer for indexed data
- **cli**: Command-line interface and MCP server (`codesearch` binary)

The main binary is `codesearch` which provides serve and index commands.

## DEVELOPMENT COMMANDS

**Building & Testing:**
```bash
cargo build --workspace --all-targets       # Build all debug targets
cargo build --release --workspace --all-targets  # Build all release targets

cargo test --workspace                    # Run unit & integration tests (excludes E2E)
cargo test --package <crate-name>         # Run tests for specific crate
cargo test --test <test-name>             # Run specific integration test

# E2E tests (slow, require Docker)
cargo test --package codesearch-e2e-tests -- --ignored           # Run all E2E tests
cargo test --package codesearch-e2e-tests -- --ignored test_name # Run specific E2E test

# Note: E2E tests manage their own containers (separate from production infrastructure)
# E2E test containers use different names and don't conflict with `codesearch-*` containers

cargo clippy --workspace                  # Lint with strict rules
cargo fmt                                 # Format code
```

**Running:**
```bash
cargo run -- index                       # Index the repository
cargo run -- serve                       # Start MCP server
```

**Docker Builds:**

The outbox processor Docker image automatically rebuilds when source files change. The build system calculates a SHA256 hash of all relevant source files and only rebuilds if the hash has changed.

```bash
codesearch index                          # Automatically rebuilds image if source changed
```

**Docker Infrastructure Management:**

The codesearch CLI uses shared Docker infrastructure located at `~/.codesearch/infrastructure/`.
All repositories connect to the same Postgres, Qdrant, Neo4j, and vLLM containers. The outbox processor runs embedded within the `codesearch serve` process.

Starting infrastructure (automatic on first `codesearch index`):
```bash
codesearch index                          # Auto-starts infrastructure if needed
```

Checking infrastructure status:
```bash
docker ps --filter "name=codesearch"      # Show running containers
docker ps -a --filter "name=codesearch"   # Show all containers (including stopped)
```

Stopping infrastructure:
```bash
cd ~/.codesearch/infrastructure
docker compose stop                       # Stop containers (keeps them for restart)
docker compose down                       # Stop AND remove containers
```

Troubleshooting:
```bash
# View logs for a specific service
docker logs codesearch-postgres
docker logs codesearch-qdrant
docker logs codesearch-neo4j
docker logs codesearch-vllm-embeddings
docker logs codesearch-vllm-reranker

# Manually clean up stale containers (usually not needed, CLI auto-cleans)
docker rm -f codesearch-postgres codesearch-qdrant codesearch-neo4j codesearch-vllm-embeddings codesearch-vllm-reranker

# Nuclear option: remove all stopped containers
docker container prune -f
```

**Note**: The CLI automatically detects and cleans up stopped infrastructure containers before starting new ones, so manual cleanup is rarely needed.

**Multi-Repository Support:**

The `codesearch serve` command serves ALL indexed repositories simultaneously:

```bash
# Index multiple repositories
cd /path/to/repo1 && codesearch index
cd /path/to/repo2 && codesearch index
cd /path/to/repo3 && codesearch index

# Serve all indexed repositories
codesearch serve

# The MCP server will list all available repositories on startup
```

**Important Notes:**
- Collection names are automatically generated from repository paths using deterministic hashing
- Collection names are stored in the PostgreSQL database, not in config files
- If a repository is moved to a new path, you must drop the old data and re-index:

```bash
cd /new/path/to/repo
codesearch drop
codesearch index
```

## Reranking Feature

Codesearch supports optional reranking of search results using cross-encoder models to improve relevance ranking.

### Overview

Reranking is a two-stage search approach:
1. **Initial Retrieval**: Vector search retrieves top candidates using embedding similarity
2. **Reranking**: A cross-encoder model reranks candidates for improved relevance

This provides better accuracy than vector search alone, as cross-encoders can model query-document interactions more precisely.

### Configuration

Enable reranking in your `~/.codesearch/config.toml`:

```toml
[reranking]
enabled = true
model = "BAAI/bge-reranker-v2-m3"
api_base_url = "http://localhost:8001"  # Optional, defaults to embeddings.api_base_url
```

### Infrastructure Requirements

Reranking requires a vLLM instance with the reranker model loaded. The shared infrastructure at `~/.codesearch/infrastructure/` includes vLLM by default.

To configure vLLM for reranking, update your `docker-compose.yml`:

```yaml
vllm:
  image: vllm/vllm-openai:latest
  command: --model BAAI/bge-reranker-v2-m3 --served-model-name BAAI/bge-reranker-v2-m3
  ports:
    - "8001:8000"
```

### Implementation Details

- **Content Consistency**: Reranking uses the same `extract_embedding_content()` function as indexing to ensure identical content representation
- **Document Truncation**: Documents are automatically truncated to ~4,800 characters (~1,200 tokens) to fit within the model's 8,192 token context window, with room for the query and multiple candidates
- **Graceful Degradation**: If reranking fails (network error, timeout), the system falls back to vector search scores
- **Performance**: Borrowed strings minimize allocations during reranking
- **Error Handling**: All errors are logged and handled gracefully without crashing the search

### BGE Instruction Usage

The codebase correctly implements asymmetric BGE instruction usage:

- **Queries**: Use BGE instruction prefix format: `<instruct>{instruction}\n<query>{query}`
- **Documents**: Do NOT use instruction prefix (raw content only)
- **Reranking**: Neither queries nor documents use instruction prefix

This asymmetry is INTENTIONAL and follows official BGE model design:
- BGE embedding model (bge-code-v1) is trained for asymmetric search
- Queries with instructions guide the model for search intent
- Documents without instructions preserve raw semantic representation
- Reranker model (bge-reranker-v2-m3) uses simple `[query, passage]` pairs without instructions

**References:**
- BGE Code Embedding: https://huggingface.co/BAAI/bge-code-v1
- BGE Reranker: https://huggingface.co/BAAI/bge-reranker-v2-m3

### Testing

Run reranking tests:
```bash
cargo test --package codesearch-embeddings

# Tests requiring a running vLLM instance (ignored by default):
cargo test --package codesearch-embeddings -- --ignored
```

## Hybrid Search Feature

Codesearch uses hybrid search by default, combining BM25 sparse embeddings with dense vector embeddings using Reciprocal Rank Fusion (RRF) for optimal search relevance.

### Overview

Hybrid search is a multi-stage retrieval approach:
1. **BM25 Sparse Retrieval**: Traditional keyword-based search using term frequency and inverse document frequency
2. **Dense Vector Retrieval**: Semantic search using learned embeddings
3. **RRF Fusion**: Combines results from both methods using Reciprocal Rank Fusion

This approach provides better accuracy than either method alone, capturing both exact keyword matches and semantic similarity.

### Configuration

Configure hybrid search in your `~/.codesearch/config.toml`:

```toml
[hybrid_search]
# Prefetch multiplier: retrieve N × limit candidates per method before fusion
# Valid range: 1-100, default: 5
# Higher values improve recall but increase latency
prefetch_multiplier = 5
```

### BM25 Implementation Details

**Tokenization:**
- Uses `CodeTokenizer` for consistent tokenization
- Splits on whitespace and special characters appropriate for code
- Same tokenization used for both indexing and querying

**Average Document Length (avgdl):**
- Calculated incrementally as entities are indexed
- Preserved when entity count reaches zero (not reset to default)
- Default fallback value: 50.0 tokens (only for brand new repositories)
- Stored per-repository in PostgreSQL

**Statistics Management:**
- `update_bm25_statistics_incremental()`: Updates after adding entities
- `update_bm25_statistics_after_deletion()`: Updates after removing entities
- `get_bm25_statistics_batch()`: Batch fetches statistics for multiple repositories
- `get_bm25_statistics_in_tx()`: Transaction-safe statistics retrieval

### Testing

Run hybrid search tests:
```bash
# Unit tests for tokenization and BM25
cargo test --package codesearch-storage test_bm25

# Integration tests for hybrid search
cargo test --workspace

# E2E tests (requires Docker)
cargo test --package codesearch-e2e-tests -- --ignored
```

## Neo4j Graph Database

Codesearch uses Neo4j to store and query code relationships, enabling graph-based queries like "find all callers of this function" or "show the inheritance hierarchy."

### Overview

Neo4j stores code entities as nodes and their relationships as edges in a graph database:
- **Nodes**: Represent code entities (functions, classes, methods, etc.)
- **Relationships**: Represent connections between entities (calls, inherits, implements, etc.)
- **Database Per Repository**: Each repository gets its own Neo4j database for isolation

### Supported Relationship Types

The following relationship types are supported (enforced by Cypher injection protection):
- `CONTAINS`: Parent-child containment (e.g., class contains method)
- `IMPLEMENTS`: Implementation of trait/interface
- `ASSOCIATES`: Association from impl block to type
- `EXTENDS_INTERFACE`: Interface inheritance
- `INHERITS_FROM`: Class inheritance
- `USES`: Type usage in fields or parameters
- `CALLS`: Function/method call
- `IMPORTS`: Module imports

### Configuration

Configure Neo4j in your `~/.codesearch/config.toml`:

```toml
[storage]
neo4j_host = "localhost"
neo4j_bolt_port = 7687       # Bolt protocol port
neo4j_http_port = 7474       # HTTP API port
neo4j_user = "neo4j"
neo4j_password = "codesearch"  # Local-only, no security concern
```

### Infrastructure Requirements

Neo4j is included in the shared infrastructure at `~/.codesearch/infrastructure/` and will be automatically started when you run `codesearch index` or `codesearch serve`. The infrastructure includes Neo4j 5.28 with the following configuration:

- **Bolt protocol:** Port 7687 (localhost only)
- **HTTP browser:** Port 7474 (localhost only)
- **Authentication:** neo4j/codesearch (local-only, no security concern)
- **Memory limits:** 512MB-2GB heap, 512MB page cache
- **Volumes:** Persistent data and logs stored in `~/.codesearch/infrastructure/` volumes
- **APOC procedures:** Enabled for advanced graph operations

### Architecture Details

**Database Management:**
- Repository database names: `codesearch_{repository_uuid}`
- Database names stored in PostgreSQL for consistency
- Automatic database creation on first index
- Indexes created automatically for common queries

**Relationship Resolution:**
- **Automatic Resolution**: Relationships are automatically resolved by the outbox processor after entities are created in Neo4j
- **No Manual Step Required**: Unlike previous versions, `codesearch index` no longer requires a manual post-indexing resolution step
- **Event-Based Triggering**: The processor sets a `pending_relationship_resolution` flag when entities are added, triggers resolution automatically
- **Supports Both Types**: Handles resolved relationships (direct entity_id) and unresolved relationships (qualified name lookup)
- **Batch Processing**: All resolvers run in batch mode to reduce network overhead
- **Resolution Stages**:
  1. Entities indexed → nodes created in Neo4j → flag set
  2. Outbox processor detects flag → runs all relationship resolvers
  3. Relationships created → flag cleared → `graph_ready` set to true
- **Resolver Types**:
  - `TraitImplResolver`: IMPLEMENTS, ASSOCIATES, EXTENDS_INTERFACE
  - `InheritanceResolver`: INHERITS_FROM
  - `TypeUsageResolver`: USES
  - `CallGraphResolver`: CALLS
  - `ImportsResolver`: IMPORTS
  - Special CONTAINS resolver with optimized batch resolution
- **Error Handling**: Failed resolutions are logged but don't block entity processing; will retry in next cycle
- **Real-Time Updates**: Works with incremental file changes through the watcher

**Security:**
- Cypher injection protection via allowlist validation
- All user-provided relationship types validated against allowed list
- Parameterized queries for all entity properties
- No dynamic Cypher construction from user input

### Testing

Run Neo4j tests:
```bash
# Unit tests (no Neo4j required)
cargo test --package codesearch-storage

# Integration tests (requires Neo4j running)
cargo test --package codesearch-storage --test neo4j_integration_test -- --ignored
```

### Troubleshooting

**Common Issues:**
```bash
# Check if Neo4j is running
docker ps | grep neo4j

# View Neo4j logs
docker logs codesearch-neo4j

# Access Neo4j browser (for manual inspection)
open http://localhost:7474

# Check repository database status
# Connect to Neo4j and run:
SHOW DATABASES
```

**Performance:**
- Uses UNWIND batching: N entities/relationships → M queries (one per type)
- Example: 10,000 entities of 5 types = 5 queries instead of 10,000
- Significantly reduces network round-trips compared to individual inserts
- Relationship resolution happens automatically in the background via outbox processor
- Resolution runs outside transaction boundaries to avoid holding locks
- BoltType system handles automatic conversion of Rust types to Neo4j parameters
- Resolvers run in parallel with other outbox processing tasks

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.
