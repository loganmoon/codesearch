# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## PROJECT OVERVIEW

Codesearch is a Rust-based semantic code indexing system that provides intelligent code search through AST-based code graph extraction, local/remote embeddings, and real-time file watching with REST API server integration.

## Rust Development Practices

**Architecture Principles:**
- IMPORTANT: Design narrow, abstract public APIs centered around traits
- IMPORTANT: Limit public exports to traits, models, errors, and factory functions
- Implement From/Into traits for API boundary conversions

**Code Quality Standards:**
- Return Result types - never panic with .unwrap() or .expect() except in tests
- Use core::Error for all error types
- Enforce `#![deny(warnings)]`, `#![deny(clippy::unwrap_used)]`, `#![deny(clippy::expect_used)]` in non-test code
- Strongly favor immutability, borrowing over cloning, builders over `new`
- Prefer standalone functions over unnecessary &self methods
- Implement RAII for resource management

**Avoid These Patterns:**
- Excessive `Box`/`Pin`/`Arc` wrapping when simpler ownership suffices
- Global state (e.g., `OnceLock<Mutex<HashMap<>>>`)
- Mixed responsibilities in single modules
- Redundant allocations during type conversions

**Style Rules:**
- String formatting: `println!("The thing is {thing}");`, NOT `println!("The thing is {}", thing);`

## CRATE ARCHITECTURE

This is a workspace with these crates:
- **core**: Foundation types, entities, configuration, error handling
- **languages**: AST parsing and entity extraction. Fully implemented for Rust, JavaScript/TypeScript, and Python. Go has partial infrastructure but no actual parsing implementation.
- **embeddings**: Vector embedding providers and local/remote embedding generation
- **indexer**: Repository indexing logic with Git integration
- **watcher**: Real-time file system monitoring with ignore patterns
- **storage**: Persistent storage layer (Postgres, Qdrant, Neo4j)
- **cli**: Command-line interface and REST API server (`codesearch` binary)

## DEVELOPMENT COMMANDS

**Build & Test:**
```bash
cargo build --workspace --all-targets                           # Build all
cargo test --workspace                                          # Unit & integration tests
cargo test --package codesearch-e2e-tests -- --ignored          # E2E tests (require Docker)
cargo clippy --workspace && cargo fmt                           # Lint & format
```

**Run:**
```bash
cargo run -- index        # Index current repository
cargo run -- serve        # Start REST API server
```

**Infrastructure:**
- Shared Docker infrastructure at `~/.codesearch/infrastructure/`
- Services: Postgres, Qdrant, Neo4j, vLLM (embeddings + reranker when `provider = "localapi"`)
- vLLM container only starts when `embeddings.provider = "localapi"` (requires GPU)
- Default Jina provider requires no local containers for embeddings
- Auto-starts on first `codesearch index`
- Multi-repository support: `codesearch serve` serves all indexed repositories
- Check status: `docker ps --filter "name=codesearch"`
- Stop: `cd ~/.codesearch/infrastructure && docker compose down`

## KEY FEATURES

**Search Architecture:**
- **Hybrid Search**: Combines sparse retrieval (Granite/BM25) + dense vector embeddings using RRF fusion in Qdrant (default)
- **Reranking**: Optional cross-encoder reranking for improved relevance (configure in `~/.codesearch/config.toml`)
- **Graph Queries**: Neo4j stores code relationships (calls, inherits, implements, etc.)

**Embedding Providers:**
- **Jina (default)**: Uses Jina AI API for embeddings. Zero-config, no GPU required. Set `JINA_API_KEY` or `embeddings.api_key` in config.
- **LocalApi**: Self-hosted vLLM with BGE models. Requires GPU. Set `embeddings.provider = "localapi"` in config.
- Provider handles query vs passage formatting internally via `EmbeddingTask` enum
- Jina: Uses task parameter (`retrieval.query`, `retrieval.passage`)
- BGE (LocalApi): Uses instruction prefix for queries only (`<instruct>...\n<query>...`)

**Neo4j Relationships:**
- Forward: CONTAINS, IMPLEMENTS, ASSOCIATES, EXTENDS_INTERFACE, INHERITS_FROM, USES, CALLS, IMPORTS
- Reciprocal: IMPLEMENTED_BY, ASSOCIATED_WITH, EXTENDED_BY, HAS_SUBCLASS, USED_BY, CALLED_BY, IMPORTED_BY
- Relationships resolved automatically by outbox processor after entity creation
- Database per repository: `codesearch_{repository_uuid}`
