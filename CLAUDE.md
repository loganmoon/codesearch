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

**Directory structure:**
```
/path/to/codesearch/              # Parent directory (NOT a worktree itself)
├── .git/                         # Shared git directory
├── main/                         # READ-ONLY - never edit files here during feature work
└── feature--my-feature/          # Your working directory - all edits happen here
```

**Important:** The parent directory containing `.git/` is NOT a worktree. All worktrees (`main/`, `feature--xyz/`, etc.) are subdirectories. All worktree management commands must be run from this parent directory.

**Branch/worktree naming convention:**
- `feature--<issue-number>-short-description` - New features
- `bug--<issue-number>-short-description` - Bug fixes
- `docs--short-description` - Documentation changes
- `chore--short-description` - Maintenance tasks
- `refactor--short-description` - Code refactoring

**Initial clone setup:**
```bash
# Clone with separate git directory (one-time setup)
git clone --separate-git-dir=.git <repo-url> main
cd ..  # Stay in parent directory for worktree management
```

**Creating a new worktree for an issue:**
```bash
# From the parent directory (containing .git/)
git worktree add <branch-name> -b <branch-name>
cd <branch-name>
```

**Working with main:**
The `main/` directory is the worktree for the main branch and is READ-ONLY during feature work:
- **NEVER edit files in `main/`** while working on a feature/bug branch
- To reference main's code, read files using their absolute path (e.g., `/path/to/codesearch/main/crates/...`) without changing directories
- Pulling latest changes: `cd main && git pull` (only when not in the middle of feature work)
- Never commit directly to main (blocked by pre-commit hook)

**During feature/bug work (IMPORTANT):**
- **Stay in your feature worktree for ALL operations** - edits, builds, tests, and cargo commands
- **NEVER switch between worktrees without explicit user instruction** - Claude Code must remain in the current worktree unless the user explicitly requests a worktree change
- **Never switch directories to another worktree to make edits** - this causes code to end up in the wrong place
- If you need to compare with main, READ from main's path but WRITE only in your feature worktree
- Example: You're in `bug--123/`. To see main's version of a file, read `/path/to/codesearch/main/crates/foo/src/lib.rs`. To edit, use `bug--123/crates/foo/src/lib.rs`
- All `cargo build`, `cargo test`, etc. commands should run from within your feature worktree

**Listing worktrees:**
```bash
git worktree list
```

**Removing a worktree after merging:**
```bash
git worktree remove <branch-name>
```

**Important:** Never use `git checkout <branch>` inside a worktree. Worktrees are permanently tied to their specific branch.

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

**Foundation:**
- **core**: Foundation types, entities, configuration, error handling
- **languages**: AST parsing and entity extraction. Fully implemented for Rust, JavaScript/TypeScript, and Python. Go has partial infrastructure but no actual parsing implementation.
- **languages-macros**: Procedural macros for defining language extractors

**Indexing & Storage:**
- **indexer**: Repository indexing logic with Git integration
- **watcher**: Real-time file system monitoring with ignore patterns
- **storage**: Persistent storage layer (Postgres, Qdrant, Neo4j)
- **outbox-processor**: Background processor for Neo4j relationship resolution

**Search & Retrieval:**
- **embeddings**: Vector embedding providers (Jina, LocalApi/vLLM)
- **reranking**: Cross-encoder reranking providers for improved relevance
- **agentic-search**: Multi-agent search orchestration with dual-track pipeline

**Servers & Interfaces:**
- **cli**: Command-line interface (`codesearch` binary)
- **server**: REST API server with filesystem watching integration
- **mcp-server**: Model Context Protocol server for AI tool integration

**Testing:**
- **e2e-tests**: E2E test infrastructure (excluded from default builds, run with `--manifest-path`)

## DEVELOPMENT COMMANDS

**Build & Test:**
```bash
cargo build --workspace --all-targets                                        # Build all
cargo test --workspace                                                       # Unit & integration tests
cargo test --manifest-path crates/e2e-tests/Cargo.toml -- --ignored          # E2E tests (require Docker)
cargo clippy --workspace && cargo fmt                                        # Lint & format
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
