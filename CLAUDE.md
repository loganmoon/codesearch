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
- **languages**: AST parsing and entity extraction. Currently only Rust is fully implemented with complete AST parsing and entity extraction. Python, JavaScript/TypeScript, and Go have partial infrastructure (dependencies, type system, file filtering) but no actual parsing implementation.
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
All repositories connect to the same Postgres, Qdrant, vLLM, and outbox-processor containers.

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
docker logs codesearch-vllm
docker logs codesearch-outbox-processor

# Manually clean up stale containers (usually not needed, CLI auto-cleans)
docker rm -f codesearch-postgres codesearch-qdrant codesearch-vllm codesearch-outbox-processor

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

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.
