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
- **languages**: AST parsing and entity extraction for supported languages (Rust, Python, JS/TS, Go)
- **embeddings**: Vector embedding providers and local/remote embedding generation
- **indexer**: Repository indexing logic with Git integration
- **watcher**: Real-time file system monitoring with ignore patterns
- **storage**: Persistent storage layer for indexed data
- **cli**: Command-line interface and MCP server (`codesearch` binary)

The main binary is `codesearch` which provides init, serve, index, and watch commands.

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

cargo clippy --workspace                  # Lint with strict rules
cargo fmt                                 # Format code
```

**Running:**
```bash
cargo run -- init                        # Initialize codesearch in current repo
cargo run -- index                       # Index the repository
cargo run -- serve                       # Start MCP server
```

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.
