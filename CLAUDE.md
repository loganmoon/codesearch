# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## PROJECT OVERVIEW

Codesearch is a Rust-based semantic code indexing system that provides intelligent code search through AST-based code graph extraction, local/remote embeddings, and real-time file watching with REST API server integration.

## REPOSITORY STRUCTURE

```
codesearch/
├── .git/                # Normal git directory (main checked out at repo root)
├── .worktrees/          # Feature branch worktrees only
│   └── <branch-name>/
├── crates/              # Rust workspace crates
│   ├── core/            # Foundation types, config, error handling
│   ├── languages/       # AST parsing with spec-driven YAML config
│   ├── embeddings/      # Dense + sparse embedding providers
│   ├── reranking/       # Cross-encoder result reranking
│   ├── indexer/         # Repository indexing with Git integration
│   ├── outbox-processor/# Reliable event processing
│   ├── watcher/         # Real-time file system monitoring
│   ├── storage/         # Postgres, Qdrant, Neo4j persistence
│   ├── agentic-search/  # Multi-agent query orchestration
│   ├── mcp-server/      # Model Context Protocol server
│   ├── server/          # REST API server
│   ├── cli/             # Command-line interface
│   ├── e2e-tests/       # End-to-end tests (excluded from default workspace build)
│   └── evals/           # Evaluation harness (excluded from default workspace build)
├── infrastructure/      # Docker Compose configuration for services
├── migrations/          # PostgreSQL database migrations (001-019)
├── scripts/             # Development and deployment scripts
│   └── hooks/           # Git hook scripts
├── .githooks/           # Active git hooks (pre-commit, pre-merge-commit)
├── Cargo.toml           # Workspace configuration
└── CLAUDE.md            # This file
```

## Git Worktree Workflow

All feature work MUST use git worktrees for isolation. Never commit feature work directly to main.

- **Main is read-only:** Four hooks enforce this:
  - `guard-worktree-edit.sh` (PreToolUse, Edit|Write) - blocks file writes outside `.worktrees/` and `.claude/`
  - `guard-bash-on-main.sh` (PreToolUse, Bash) - blocks non-whitelisted shell commands when CWD is on main with worktrees present
  - `guard-worktree-escape.sh` (PreToolUse, Bash) - blocks leaving a worktree (`cd` to repo root) unless the branch's PR is merged
  - `warn-worktree-drift.sh` (PostToolUse, Bash) - warns if CWD drifts to repo root while worktrees exist
- `cd` into the worktree root before starting work so it becomes CWD
- Hook changes are git-tracked and must be committed on a branch, not main

### Creating a worktree
- ALWAYS run `git worktree list` before creating a new worktree -- one may already exist
- NEVER create a new worktree when work is in progress on one

```bash
git worktree add .worktrees/<branch-name>
cd .worktrees/<branch-name>
```
- Directory name MUST match branch name (do NOT use `-b` with a different name)
- This creates both the worktree directory and the branch in one step

### Working in a worktree
- NEVER run `git checkout <branch>` inside a worktree -- worktrees are locked to their branch
- To run commands that the bash guard blocks on main (cargo test, npm install, etc.), `cd` into a worktree first

### Cleanup after merge
The escape guard will allow `cd` to repo root only once the PR is merged:
```bash
cd <repo-root>
git worktree remove .worktrees/<branch-name>
git branch -D <branch-name>
```
- NEVER remove a worktree while it is the shell CWD

## Rust Development Practices

**Architecture Principles:**
- IMPORTANT: Design narrow, abstract public APIs centered around traits
- IMPORTANT: Minimize public exports from lib crates. Minimize visibility within crates (default to private)
- Implement From/Into traits for API boundary conversions

**LSP Usage (Should strongly prefer):**
- Use LSP tools to explore code before writing or modifying:
  - `documentSymbol` to discover existing functions/types in a file
  - `goToDefinition` to trace implementations and understand code flow
  - `findReferences` to understand how code is used

**Code Quality Standards:**
- Return Result types - never panic with .unwrap() or .expect() except in tests
- Use `codesearch_core::Error` (`crates/core/src/error.rs`) for all error types
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

## Build and Test

**Build flags:** Always use `--no-default-features` for clippy and tests. Default features include optional components that may not compile in all environments.

```bash
# Pre-commit checks (what the hooks run):
cargo fmt --check
cargo clippy --workspace --all-targets --no-default-features -- -D warnings
cargo test --workspace --lib --no-default-features
```

**Test levels:**
- `cargo test --workspace --lib --no-default-features` - Unit tests only (run by pre-commit hook)
- `cargo test --workspace --no-default-features` - Unit + integration tests
- `cargo test --manifest-path crates/e2e-tests/Cargo.toml -- --ignored` - E2E tests (requires running infrastructure)

**Environment variables:**
- `JINA_API_KEY` - Required for Jina embedding/reranking providers
- `ANTHROPIC_API_KEY` - Required for agentic search endpoint
- `EMBEDDING_API_KEY` - Alternative to `JINA_API_KEY` for embedding providers

## Language Extraction (crates/languages)

**Reference Documentation:** `crates/languages/docs/new-language-onboarding.md`
- This doc exists but may not be fully current - verify requirements with user when starting work
