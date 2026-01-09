# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## PROJECT OVERVIEW

Codesearch is a Rust-based semantic code indexing system that provides intelligent code search through AST-based code graph extraction, local/remote embeddings, and real-time file watching with REST API server integration.

## REPOSITORY STRUCTURE

```
codesearch/
├── .git/                # Shared git directory (separate from worktrees)
├── main/                # Worktree for main branch
├── <branch-name>/       # Additional worktrees for feature branches
│
# Within each worktree:
├── crates/              # Rust workspace crates (see CRATE ARCHITECTURE below)
├── infrastructure/      # Docker Compose configuration for services
├── migrations/          # PostgreSQL database migrations
├── scripts/             # Development and deployment scripts
│   └── hooks/           # Git hook scripts
├── .githooks/           # Active git hooks (pre-commit, pre-merge-commit)
├── Cargo.toml           # Workspace configuration
└── CLAUDE.md            # This file
```

## WORKTREE WORKFLOW

This project uses git worktrees with a separate git directory for parallel development. Each issue or feature gets its own worktree.

**Important:** The parent directory containing `.git/` is NOT a worktree. All worktrees (`main/`, `feature--xyz/`, etc.) are subdirectories. All worktree management commands must be run from this parent directory.

**Important:** Never use `git checkout <branch>` inside a worktree. Worktrees are permanently tied to their specific branch.

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

## Language Extraction (crates/languages)

**Reference Documentation:** `crates/languages/docs/new-language-onboarding.md`
- This doc exists but may not be fully current - verify requirements with user when starting work
