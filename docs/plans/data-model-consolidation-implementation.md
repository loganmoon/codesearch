# Data Model Consolidation Implementation Plan

**Created**: 2025-10-03T10:19:44-05:00
**Status**: Draft
**Related Issue**: GitHub Issue #7 (Entity Invalidation)
**Research Document**: `docs/research/data-model-consolidation-postgres.md`

## Overview

This plan implements a simplified data model for codesearch that consolidates PostgreSQL as the single source of truth for entity data, eliminates version history complexity, and enables entity invalidation when files change. The new design uses semantic structural paths for stable entity identification, adds repository scoping, and reduces Qdrant to a minimal vector index.

## Current State Analysis

### Problems with Current Architecture

1. **File-path-dependent entity IDs**: Moving code between files creates new entity_id, breaking identity
   - Current: `entity_id = hash(file_path:qualified_name)` (core/src/entity_id.rs:73-79)
   - Problem: Same entity in different file = different ID

2. **No repository scoping**: Entity IDs collide across repositories
   - No `repository_id` in schema (migrations/001_initial_schema.sql)
   - Collection name is only identifier

3. **Version history complexity**: Three version identifiers (version_id, version_number, git_commit_hash)
   - entity_versions table stores full history (migrations/001_initial_schema.sql:25-38)
   - Not needed - git already provides this

4. **No qualified name building**: Scope stack never populated
   - ScopeContext created empty for each entity (languages/src/rust/handlers/function_handlers.rs:35)
   - No parent traversal implemented
   - All qualified names are just simple names

5. **Qdrant stores complete entities**: Full CodeEntity in payload, duplicating Postgres
   - storage/src/qdrant/client.rs:34-45 serializes entire entity
   - Violates single source of truth principle

6. **No entity invalidation**: Stale entities remain when files change
   - DELETE operation stubbed (storage/src/postgres/outbox_processor.rs:104-107)
   - No file snapshot tracking
   - No stale detection logic

### Key Discoveries

- **No scope tracking exists**: push_scope() and pop_scope() only used in tests (core/src/entity_id.rs:182-215)
- **No parent traversal**: Zero calls to `.parent()` in languages crate
- **Tree-sitter supports parent navigation**: Available but unused
- **Outbox pattern works**: Postgres → outbox → Qdrant flow is solid foundation
- **Git integration exists**: Commit hash captured per entity (indexer/src/repository_indexer.rs:300)

## Desired End State

### Database Schema

**New simplified schema with 4 tables**:
1. `repositories` - Repository metadata
2. `entity_metadata` - Current entity state (no version fields)
3. `file_entity_snapshots` - Entity IDs per file for diffing
4. `entity_outbox` - Async sync queue (no version_id)

### Entity Identity

**Stable semantic IDs**: `entity_id = hash(repository_id:qualified_name)`
- Example: `"repo-uuid:geometry::Point::new"` → `entity-abc123...`
- File moves don't change ID
- Repository scoped (no collisions)

### Qualified Names

**Built via Tree-sitter parent traversal**:
- Start at entity node
- Walk up via `.parent()` to find scope containers
- Collect names: impl blocks, mod blocks, etc.
- Example: `function_item` → `impl_item` → `mod_item` → `"geometry::Point::new"`

### Data Flow

**Simplified storage**: Postgres only, Qdrant minimal
- Postgres: Full entity data in entity_metadata.entity_data (JSONB)
- Qdrant: Only entity_id + display fields (name, type, file_path, line_range)
- Search: Qdrant → IDs → Postgres batch fetch → full entities

### Verification

**Success means**:
- Re-indexing same code produces same entity_ids
- Moving entity between files preserves ID
- File changes trigger stale entity deletion
- All entities scoped to repository_id
- Qdrant payloads < 500 bytes per entity

## What We're NOT Doing

- ❌ Keeping version history (git provides this)
- ❌ Content-based entity hashing (qualified name is identity)
- ❌ Neo4j integration (future work)
- ❌ Watch command (future work, Phase 6 enables it)
- ❌ Migration of existing data (breaking change, re-index required)
- ❌ Backwards compatibility (acceptable per user)

## Implementation Approach

**Strategy**: Bottom-up migration
1. Schema first (Postgres changes)
2. Core identity layer (entity ID generation)
3. Repository management (init/config)
4. Storage layer (simplified CRUD)
5. Data reduction (Qdrant minimization)
6. Deletion logic (invalidation)

**Principles**:
- Each phase builds on previous
- All changes behind new schema migration
- Old code paths removed (no compatibility layer)
- Re-indexing required for testing each phase

---

## Phase 1: Database Schema Migration

### Overview

Create new simplified schema with repositories, file snapshots, and simplified entity storage. Remove version history table and version-related foreign keys.

### Changes Required

#### 1. New Migration File

**File**: `migrations/002_simplified_entity_storage.sql`
**Changes**: Complete schema overhaul

```sql
-- 1. Create repositories table
CREATE TABLE repositories (
    repository_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    repository_path TEXT UNIQUE NOT NULL,
    repository_name TEXT NOT NULL,
    collection_name VARCHAR(255) UNIQUE NOT NULL,
    git_remote_url TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_repositories_collection ON repositories(collection_name);

-- 2. Drop old entity_versions table (breaking change)
DROP TABLE IF EXISTS entity_versions CASCADE;

-- 3. Recreate entity_metadata without version fields
DROP TABLE IF EXISTS entity_metadata CASCADE;

CREATE TABLE entity_metadata (
    entity_id VARCHAR(64) NOT NULL,
    repository_id UUID NOT NULL,
    qualified_name TEXT NOT NULL,
    name TEXT NOT NULL,
    parent_scope TEXT,
    entity_type VARCHAR(32) NOT NULL,
    language VARCHAR(32) NOT NULL,
    file_path TEXT NOT NULL,
    line_range INT4RANGE NOT NULL,
    visibility VARCHAR(32) NOT NULL,
    entity_data JSONB NOT NULL,
    git_commit_hash VARCHAR(40),
    qdrant_point_id UUID,
    neo4j_node_id BIGINT,
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ,
    PRIMARY KEY (repository_id, entity_id),
    FOREIGN KEY (repository_id) REFERENCES repositories(repository_id) ON DELETE CASCADE
);

CREATE INDEX idx_entity_metadata_file_path
    ON entity_metadata(repository_id, file_path) WHERE deleted_at IS NULL;
CREATE INDEX idx_entity_metadata_qualified_name
    ON entity_metadata(repository_id, qualified_name);
CREATE INDEX idx_entity_metadata_entity_type
    ON entity_metadata(entity_type);
CREATE INDEX idx_entity_metadata_deleted_at
    ON entity_metadata(deleted_at) WHERE deleted_at IS NOT NULL;

-- 4. Create file_entity_snapshots for stale detection
CREATE TABLE file_entity_snapshots (
    repository_id UUID NOT NULL,
    file_path TEXT NOT NULL,
    git_commit_hash VARCHAR(40),
    entity_ids TEXT[] NOT NULL,
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (repository_id, file_path),
    FOREIGN KEY (repository_id) REFERENCES repositories(repository_id) ON DELETE CASCADE
);

CREATE INDEX idx_file_entity_snapshots_commit
    ON file_entity_snapshots(repository_id, git_commit_hash);

-- 5. Recreate entity_outbox without version_id
DROP TABLE IF EXISTS entity_outbox CASCADE;

CREATE TABLE entity_outbox (
    outbox_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    repository_id UUID NOT NULL,
    entity_id VARCHAR(64) NOT NULL,
    operation VARCHAR(10) NOT NULL CHECK (operation IN ('INSERT', 'UPDATE', 'DELETE')),
    target_store VARCHAR(32) NOT NULL CHECK (target_store IN ('qdrant', 'neo4j')),
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMPTZ,
    retry_count INT NOT NULL DEFAULT 0,
    last_error TEXT,
    FOREIGN KEY (repository_id, entity_id)
        REFERENCES entity_metadata(repository_id, entity_id)
        ON DELETE CASCADE
);

CREATE INDEX idx_entity_outbox_unprocessed
    ON entity_outbox(target_store, created_at) WHERE processed_at IS NULL;
CREATE INDEX idx_entity_outbox_entity_id
    ON entity_outbox(repository_id, entity_id);
```

**Key Changes**:
- **repositories**: New table for repository metadata
- **entity_metadata**:
  - Composite PK (repository_id, entity_id)
  - Added: repository_id, name, parent_scope, visibility, entity_data
  - Removed: current_version_id FK
  - Single git_commit_hash for traceability
- **file_entity_snapshots**: New table with entity_id arrays per file
- **entity_outbox**:
  - Added repository_id
  - Removed version_id FK
  - FK to (repository_id, entity_id)

#### 2. Migration Runner

**File**: `crates/storage/src/postgres/migrations.rs`
**Changes**: Add migration execution logic

Currently migrations are applied manually. Need to verify sqlx migrations are working:
- Check `crates/storage/Cargo.toml` for sqlx dependency (should have `migrate` feature)
- Migrations should auto-run on PostgresClient creation (storage/src/postgres/mod.rs)
- If not, add explicit migration runner in init command

### Success Criteria

#### Automated Verification:
- [ ] Migration runs successfully: `sqlx migrate run`
- [ ] All tables created: `psql -c "\dt"` shows repositories, entity_metadata, file_entity_snapshots, entity_outbox
- [ ] Old entity_versions table removed
- [ ] Foreign key constraints valid: `psql -c "\d entity_metadata"` shows FK to repositories
- [ ] Indexes created: Check with `\di` in psql

#### Manual Verification:
- [ ] Can insert test repository record manually
- [ ] Can insert entity_metadata with composite PK
- [ ] Can insert file_entity_snapshots with TEXT[] array
- [ ] Cascade delete works (delete repository → entities removed)

---

## Phase 2: Entity ID Generation with Semantic Paths

### Overview

Implement Tree-sitter parent traversal to build qualified names from AST structure. Update entity_id generation to use `hash(repository_id:qualified_name)` without file_path. Modify all language extractors to build semantic paths.

### Changes Required

#### 1. Core Entity ID Generation

**File**: `crates/core/src/entity_id.rs`
**Changes**: Remove file_path from entity_id, add repository_id

Lines 73-79 currently:
```rust
pub fn generate_entity_id_from_qualified_name(qualified_name: &str, file_path: &Path) -> String {
    let unique_str = format!("{}:{}", file_path.display(), qualified_name);
    format!("entity-{:032x}", XxHash3_128::oneshot(unique_str.as_bytes()))
}
```

Replace with:
```rust
pub fn generate_entity_id(repository_id: &str, qualified_name: &str) -> String {
    let unique_str = format!("{}:{}", repository_id, qualified_name);
    format!("entity-{:032x}", XxHash3_128::oneshot(unique_str.as_bytes()))
}
```

**Anonymous entity ID** (lines 96-113): Also update to include repository_id instead of file_path in hash:
```rust
pub fn generate_anonymous_entity_id(
    repository_id: &str,
    qualified_name: &str,
    anonymous_index: usize,
    start_line: usize,
    start_column: usize,
    entity_type: &str,
) -> String {
    let unique_str = format!(
        "{}:{}:L{}:C{}:{}:anon-{}",
        repository_id,
        qualified_name,
        start_line,
        start_column,
        entity_type,
        anonymous_index
    );
    format!("entity-anon-{:032x}", XxHash3_128::oneshot(unique_str.as_bytes()))
}
```

#### 2. CodeEntity Model Update

**File**: `crates/core/src/entities.rs`
**Changes**: Add repository_id field

At line 95-96, add:
```rust
pub entity_id: String,
pub repository_id: String,  // NEW: Repository UUID
```

Update builder at lines 161-172 to include repository_id field.

#### 3. Parent Traversal Utility

**File**: `crates/languages/src/common.rs` (or new `crates/languages/src/qualified_name.rs`)
**Changes**: Create parent traversal helper

```rust
use tree_sitter::Node;

/// Build qualified name by traversing AST parents to find scope containers
pub fn build_qualified_name_from_ast(node: Node, source: &str, language: &str) -> String {
    let mut scope_parts = Vec::new();
    let mut current = node;

    // Walk up the tree collecting scope names
    while let Some(parent) = current.parent() {
        let scope_name = match language {
            "rust" => extract_rust_scope_name(parent, source),
            "python" => extract_python_scope_name(parent, source),
            "javascript" | "typescript" => extract_js_scope_name(parent, source),
            "go" => extract_go_scope_name(parent, source),
            _ => None,
        };

        if let Some(name) = scope_name {
            scope_parts.push(name);
        }

        current = parent;
    }

    // Reverse to get root-to-leaf order
    scope_parts.reverse();
    scope_parts.join("::")
}

fn extract_rust_scope_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "mod_item" => {
            // Find name child
            node.child_by_field_name("name")
                .and_then(|n| Some(n.utf8_text(source.as_bytes()).ok()?.to_string()))
        }
        "impl_item" => {
            // Find type child
            node.child_by_field_name("type")
                .and_then(|n| Some(n.utf8_text(source.as_bytes()).ok()?.to_string()))
        }
        _ => None,
    }
}

fn extract_python_scope_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "class_definition" => {
            node.child_by_field_name("name")
                .and_then(|n| Some(n.utf8_text(source.as_bytes()).ok()?.to_string()))
        }
        "function_definition" => {
            // Include nested functions in path
            node.child_by_field_name("name")
                .and_then(|n| Some(n.utf8_text(source.as_bytes()).ok()?.to_string()))
        }
        _ => None,
    }
}

fn extract_js_scope_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "class_declaration" => {
            node.child_by_field_name("name")
                .and_then(|n| Some(n.utf8_text(source.as_bytes()).ok()?.to_string()))
        }
        "object" => {
            // Objects assigned to variables
            // Need to check parent assignment
            None // TODO: More complex logic needed
        }
        _ => None,
    }
}

fn extract_go_scope_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "type_declaration" => {
            node.child_by_field_name("name")
                .and_then(|n| Some(n.utf8_text(source.as_bytes()).ok()?.to_string()))
        }
        "method_declaration" => {
            // Extract receiver type
            node.child_by_field_name("receiver")
                .and_then(|r| r.child_by_field_name("type"))
                .and_then(|n| Some(n.utf8_text(source.as_bytes()).ok()?.to_string()))
        }
        _ => None,
    }
}
```

#### 4. Update Rust Extractors

**File**: `crates/languages/src/rust/handlers/function_handlers.rs`
**Changes**: Use parent traversal instead of empty ScopeContext

Lines 29-91, replace `extract_function_components`:

```rust
fn extract_function_components(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,  // NEW parameter
) -> Result<FunctionComponents> {
    // Get function node from @function capture
    let function_node = query_match
        .captures()
        .iter()
        .find(|c| query.capture_names()[c.index as usize] == "function")
        .ok_or_else(|| Error::extraction("No @function capture found"))?
        .node;

    // Extract simple name
    let name = query_match
        .captures()
        .iter()
        .find(|c| query.capture_names()[c.index as usize] == "name")
        .ok_or_else(|| Error::extraction("No @name capture found"))?
        .node
        .utf8_text(source.as_bytes())?
        .to_string();

    // Build qualified name via parent traversal
    let qualified_name = crate::common::build_qualified_name_from_ast(
        function_node,
        source,
        "rust",
    );
    let full_qualified_name = if qualified_name.is_empty() {
        name.clone()
    } else {
        format!("{}::{}", qualified_name, name)
    };

    // Generate entity_id from repository + qualified name
    let entity_id = generate_entity_id(repository_id, &full_qualified_name);

    // ... rest of extraction logic unchanged

    Ok(FunctionComponents {
        entity_id,
        name,
        qualified_name: full_qualified_name,
        // ... other fields
    })
}
```

**Similar changes needed in**:
- `crates/languages/src/rust/handlers/type_handlers.rs:64-288` (structs, enums, traits)
- `crates/languages/src/python/handlers/function_handlers.rs`
- `crates/languages/src/python/handlers/class_handlers.rs`
- `crates/languages/src/javascript/handlers/*.rs`
- `crates/languages/src/go/handlers/*.rs`

#### 5. Update Handler Signatures

**File**: `crates/languages/src/extraction_framework.rs`
**Changes**: Add repository_id parameter to handler trait

Line 16-17 currently:
```rust
pub type EntityHandler =
    Box<dyn Fn(&QueryMatch, &Query, &str, &Path) -> Result<Vec<CodeEntity>> + Send + Sync>;
```

Change to:
```rust
pub type EntityHandler =
    Box<dyn Fn(&QueryMatch, &Query, &str, &Path, &str) -> Result<Vec<CodeEntity>> + Send + Sync>;
    //                                                  ^^^^^ repository_id: &str
```

Update dispatch at line 175-207 to pass repository_id:
```rust
let entities = handler(
    query_match,
    query,
    source,
    file_path,
    repository_id,  // NEW
)?;
```

#### 6. GenericExtractor Update

**File**: `crates/languages/src/extraction_framework.rs`
**Changes**: Add repository_id field and parameter

Line 26-32, add field:
```rust
pub struct GenericExtractor {
    language: tree_sitter::Language,
    combined_query: Query,
    handlers: Vec<(String, EntityHandler)>,
    repository_id: String,  // NEW
}
```

Update `extract()` at line 159:
```rust
pub fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
    // Pass self.repository_id to handlers
    // ... in dispatch loop
}
```

Update factory functions in each language module to accept repository_id:
- `crates/languages/src/rust/mod.rs:27-53`
- `crates/languages/src/python/mod.rs`
- `crates/languages/src/javascript/mod.rs`
- `crates/languages/src/go/mod.rs`

#### 7. Extractor Factory Update

**File**: `crates/indexer/src/repository_indexer.rs`
**Changes**: Pass repository_id to extractor creation

Line 368 currently:
```rust
let extractor = create_extractor(file_path)?;
```

Change to:
```rust
let extractor = create_extractor(file_path, &self.repository_id)?;
```

Add `repository_id` field to RepositoryIndexer struct at line 53-58:
```rust
pub struct RepositoryIndexer {
    repository_path: PathBuf,
    repository_id: String,  // NEW: UUID as string
    embedding_manager: Arc<EmbeddingManager>,
    postgres_client: Arc<PostgresClient>,
    git_repo: Option<GitRepository>,
}
```

### Success Criteria

#### Automated Verification:
- [ ] Rust code compiles: `cargo build --package codesearch-languages`
- [ ] Unit tests pass: `cargo test --package codesearch-languages`
- [ ] Entity ID format correct: Test generates `entity-{32 hex}` without file path
- [ ] Qualified names built: Test nested Rust impl method gets `"MyMod::MyStruct::method"`

#### Manual Verification:
- [ ] Extract entities from test Rust file with nested module/impl
- [ ] Verify qualified_name includes full path (e.g., `"geometry::Point::new"`)
- [ ] Verify entity_id stable when entity moved between files
- [ ] Check Python class methods get `"ClassName::method_name"`
- [ ] Check JavaScript class methods get proper qualification

---

## Phase 3: Repository Management

### Overview

Add repository table management to PostgresClient. Update init command to create repository records. Thread repository_id through indexer initialization.

### Changes Required

#### 1. Repository Operations in PostgresClient

**File**: `crates/storage/src/postgres/client.rs`
**Changes**: Add repository management methods after line 59

```rust
impl PostgresClient {
    // ... existing methods ...

    /// Ensure repository exists, return repository_id
    pub async fn ensure_repository(
        &self,
        repository_path: &Path,
        collection_name: &str,
        repository_name: Option<&str>,
    ) -> Result<uuid::Uuid> {
        let repo_path_str = repository_path.to_str()
            .ok_or_else(|| Error::storage("Invalid repository path"))?;

        // Try to find existing repository
        let existing = sqlx::query!(
            "SELECT repository_id FROM repositories WHERE collection_name = $1",
            collection_name
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(record) = existing {
            return Ok(record.repository_id);
        }

        // Create new repository
        let repo_name = repository_name
            .or_else(|| repository_path.file_name()?.to_str())
            .unwrap_or("unknown");

        let record = sqlx::query!(
            "INSERT INTO repositories (repository_path, repository_name, collection_name, created_at, updated_at)
             VALUES ($1, $2, $3, NOW(), NOW())
             RETURNING repository_id",
            repo_path_str,
            repo_name,
            collection_name
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(record.repository_id)
    }

    /// Get repository by collection name
    pub async fn get_repository_id(&self, collection_name: &str) -> Result<Option<uuid::Uuid>> {
        let record = sqlx::query!(
            "SELECT repository_id FROM repositories WHERE collection_name = $1",
            collection_name
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(record.map(|r| r.repository_id))
    }
}
```

#### 2. Update Init Command

**File**: `crates/cli/src/main.rs`
**Changes**: Create repository record after Qdrant initialization

At line 292-303, after `initialize_collection()` succeeds, add:

```rust
// Create repository record in database
let postgres_client = Arc::new(
    codesearch_storage::create_postgres_client(&config.storage).await?
);

let repository_id = postgres_client
    .ensure_repository(&repo_root, &config.storage.collection_name, None)
    .await?;

info!("Repository initialized with ID: {}", repository_id);
info!("Collection name: {}", config.storage.collection_name);
```

#### 3. Update RepositoryIndexer Creation

**File**: `crates/cli/src/main.rs`
**Changes**: Fetch repository_id before creating indexer

At line 511-516, before creating RepositoryIndexer:

```rust
// Get repository_id from database
let repository_id = postgres_client
    .get_repository_id(&config.storage.collection_name)
    .await?
    .ok_or_else(|| anyhow::anyhow!(
        "Repository not found. Please run 'codesearch init' first."
    ))?;

let mut indexer = RepositoryIndexer::new(
    repo_path.clone(),
    repository_id.to_string(),  // Pass as string
    embedding_manager,
    postgres_client,
    git_repo,
);
```

#### 4. Update RepositoryIndexer Constructor

**File**: `crates/indexer/src/repository_indexer.rs`
**Changes**: Accept repository_id parameter

Line 107-122, update `new()`:

```rust
pub fn new(
    repository_path: PathBuf,
    repository_id: String,  // NEW parameter
    embedding_manager: Arc<EmbeddingManager>,
    postgres_client: Arc<PostgresClient>,
    git_repo: Option<GitRepository>,
) -> Self {
    Self {
        repository_path,
        repository_id,  // Store it
        embedding_manager,
        postgres_client,
        git_repo,
    }
}
```

#### 5. Update Indexer Factory

**File**: `crates/indexer/src/lib.rs`
**Changes**: Add repository_id parameter to factory

Line 216-221:

```rust
pub async fn create_indexer(
    repository_path: PathBuf,
    repository_id: String,  // NEW
    embedding_manager: Arc<EmbeddingManager>,
    postgres_client: Arc<PostgresClient>,
    git_repo: Option<GitRepository>,
) -> RepositoryIndexer {
    RepositoryIndexer::new(
        repository_path,
        repository_id,
        embedding_manager,
        postgres_client,
        git_repo,
    )
}
```

### Success Criteria

#### Automated Verification:
- [ ] Init command succeeds: `cargo run -- init`
- [ ] Repository record created: `psql -c "SELECT * FROM repositories"`
- [ ] Index command finds repository: `cargo run -- index` doesn't error
- [ ] Repository ID logged during init

#### Manual Verification:
- [ ] Run init in new repository, check repositories table has entry
- [ ] Run init again in same repository, verify no duplicate (upsert works)
- [ ] Index command logs repository_id at startup
- [ ] collection_name in repositories table matches codesearch.toml

---

## Phase 4: Simplified Storage Layer

### Overview

Remove version history logic from PostgresClient. Simplify `store_entity_metadata()` to single upsert. Implement file snapshot tracking for stale detection.

### Changes Required

#### 1. Simplify store_entity_metadata

**File**: `crates/storage/src/postgres/client.rs`
**Changes**: Replace lines 62-157 with simplified version

Remove version number calculation (lines 82-88), version insert (lines 91-117). New implementation:

```rust
/// Store or update entity metadata (simplified - no version history)
pub async fn store_entity_metadata(
    &self,
    repository_id: uuid::Uuid,
    entity: &CodeEntity,
    git_commit_hash: Option<String>,
    qdrant_point_id: uuid::Uuid,
) -> Result<()> {
    let mut tx = self.pool.begin().await?;

    // Serialize entity to JSONB
    let entity_json = serde_json::to_value(entity)
        .map_err(|e| Error::storage(format!("Failed to serialize entity: {}", e)))?;

    // Upsert entity_metadata
    sqlx::query!(
        "INSERT INTO entity_metadata (
            entity_id, repository_id, qualified_name, name, parent_scope,
            entity_type, language, file_path, line_range, visibility,
            entity_data, git_commit_hash, qdrant_point_id,
            indexed_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, NOW(), NOW())
        ON CONFLICT (repository_id, entity_id)
        DO UPDATE SET
            qualified_name = EXCLUDED.qualified_name,
            name = EXCLUDED.name,
            parent_scope = EXCLUDED.parent_scope,
            entity_type = EXCLUDED.entity_type,
            language = EXCLUDED.language,
            file_path = EXCLUDED.file_path,
            line_range = EXCLUDED.line_range,
            visibility = EXCLUDED.visibility,
            entity_data = EXCLUDED.entity_data,
            git_commit_hash = EXCLUDED.git_commit_hash,
            qdrant_point_id = EXCLUDED.qdrant_point_id,
            updated_at = NOW(),
            deleted_at = NULL",
        entity.entity_id,
        repository_id,
        entity.qualified_name,
        entity.name,
        entity.parent_scope,
        format!("{:?}", entity.entity_type), // Enum to string
        entity.language.to_string(),
        entity.file_path.to_str().unwrap(),
        format!("[{},{})", entity.line_range.0, entity.line_range.1), // int4range format
        format!("{:?}", entity.visibility),
        entity_json,
        git_commit_hash,
        qdrant_point_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(())
}
```

**Key changes**:
- No version_id return value (was UUID, now returns ())
- No version_number calculation
- No entity_versions insert
- Single UPSERT into entity_metadata
- Sets deleted_at = NULL on update (undeletes if re-indexed)

#### 2. Add File Snapshot Operations

**File**: `crates/storage/src/postgres/client.rs`
**Changes**: Add methods after store_entity_metadata

```rust
/// Get file snapshot (list of entity IDs in file)
pub async fn get_file_snapshot(
    &self,
    repository_id: uuid::Uuid,
    file_path: &str,
) -> Result<Option<Vec<String>>> {
    let record = sqlx::query!(
        "SELECT entity_ids FROM file_entity_snapshots
         WHERE repository_id = $1 AND file_path = $2",
        repository_id,
        file_path
    )
    .fetch_optional(&self.pool)
    .await?;

    Ok(record.map(|r| r.entity_ids))
}

/// Update file snapshot with current entity IDs
pub async fn update_file_snapshot(
    &self,
    repository_id: uuid::Uuid,
    file_path: &str,
    entity_ids: Vec<String>,
    git_commit_hash: Option<String>,
) -> Result<()> {
    sqlx::query!(
        "INSERT INTO file_entity_snapshots (repository_id, file_path, entity_ids, git_commit_hash, indexed_at)
         VALUES ($1, $2, $3, $4, NOW())
         ON CONFLICT (repository_id, file_path)
         DO UPDATE SET
            entity_ids = EXCLUDED.entity_ids,
            git_commit_hash = EXCLUDED.git_commit_hash,
            indexed_at = NOW()",
        repository_id,
        file_path,
        &entity_ids,
        git_commit_hash
    )
    .execute(&self.pool)
    .await?;

    Ok(())
}
```

#### 3. Update Indexer Storage Flow

**File**: `crates/indexer/src/repository_indexer.rs`
**Changes**: Update lines 307-341 to use simplified storage

```rust
// Get repository_id as UUID
let repository_id = uuid::Uuid::parse_str(&self.repository_id)?;

// Get git commit once for entire batch
let git_commit = self.current_git_commit().await.ok();

// Group entities by file for snapshot tracking
let mut entities_by_file: std::collections::HashMap<String, Vec<String>> =
    std::collections::HashMap::new();

// Store each entity
for entity in &entities_with_embeddings {
    // Generate Qdrant point ID
    let point_id = uuid::Uuid::new_v4();

    // Store entity metadata (simplified - no version_id returned)
    self.postgres_client
        .store_entity_metadata(
            repository_id,
            entity,
            git_commit.clone(),
            point_id,
        )
        .await?;

    // Track for file snapshot
    let file_path_str = entity.file_path.to_str().unwrap().to_string();
    entities_by_file
        .entry(file_path_str.clone())
        .or_insert_with(Vec::new)
        .push(entity.entity_id.clone());

    // Write INSERT to outbox (payload is full entity for Phase 4, will be minimal in Phase 5)
    let payload = serde_json::to_value(entity)?;
    self.postgres_client
        .write_outbox_entry(
            repository_id,
            &entity.entity_id,
            OutboxOperation::Insert,
            TargetStore::Qdrant,
            payload,
        )
        .await?;
}

// Update file snapshots
for (file_path, entity_ids) in entities_by_file {
    self.postgres_client
        .update_file_snapshot(
            repository_id,
            &file_path,
            entity_ids,
            git_commit.clone(),
        )
        .await?;
}
```

#### 4. Update Outbox Writer

**File**: `crates/storage/src/postgres/client.rs`
**Changes**: Remove version_id parameter from write_outbox_entry (line 191)

```rust
pub async fn write_outbox_entry(
    &self,
    repository_id: uuid::Uuid,  // NEW
    entity_id: &str,
    operation: OutboxOperation,
    target_store: TargetStore,
    payload: serde_json::Value,
) -> Result<uuid::Uuid> {
    let record = sqlx::query!(
        "INSERT INTO entity_outbox (repository_id, entity_id, operation, target_store, payload, created_at)
         VALUES ($1, $2, $3, $4, $5, NOW())
         RETURNING outbox_id",
        repository_id,
        entity_id,
        format!("{:?}", operation),
        format!("{:?}", target_store),
        payload
    )
    .fetch_one(&self.pool)
    .await?;

    Ok(record.outbox_id)
}
```

#### 5. Remove get_entity_versions

**File**: `crates/storage/src/postgres/client.rs`
**Changes**: Delete method at lines 173-188 (no longer needed)

### Success Criteria

#### Automated Verification:
- [ ] Rust compiles: `cargo build --package codesearch-storage`
- [ ] Storage tests pass: `cargo test --package codesearch-storage`
- [ ] Integration test: Index test repository, verify entity_metadata populated
- [ ] No entity_versions table referenced in code: `rg "entity_versions" crates/`

#### Manual Verification:
- [ ] Run full index, check entity_metadata has records
- [ ] Verify entity_data JSONB contains full entity
- [ ] Check file_entity_snapshots populated after indexing
- [ ] Re-index same file, verify UPDATE not INSERT
- [ ] Verify git_commit_hash stored in entity_metadata

---

## Phase 5: Qdrant Payload Reduction

### Overview

Change Qdrant payload to store only entity_id and display fields (name, qualified_name, entity_type, file_path, line_range). Update search flow to fetch full entities from Postgres after Qdrant returns IDs.

### Changes Required

#### 1. Minimal Payload Function

**File**: `crates/storage/src/qdrant/client.rs`
**Changes**: Replace entity_to_payload at lines 34-45

```rust
/// Convert entity to minimal Qdrant payload (display fields only)
fn entity_to_minimal_payload(entity: &CodeEntity) -> Payload {
    let mut map = serde_json::Map::new();

    // Core identifiers
    map.insert("entity_id".to_string(), json!(entity.entity_id));
    map.insert("repository_id".to_string(), json!(entity.repository_id));

    // Display fields for search results
    map.insert("name".to_string(), json!(entity.name));
    map.insert("qualified_name".to_string(), json!(entity.qualified_name));
    map.insert("entity_type".to_string(), json!(format!("{:?}", entity.entity_type)));
    map.insert("file_path".to_string(), json!(entity.file_path.display().to_string()));
    map.insert("line_range_start".to_string(), json!(entity.line_range.0));
    map.insert("line_range_end".to_string(), json!(entity.line_range.1));

    // DO NOT include: content, signature, dependencies, metadata, documentation_summary

    Payload::from(map)
}
```

Update `bulk_load_entities()` at line 140 to use minimal payload:
```rust
let point = PointStruct::new(
    PointId::from(point_id.to_string()),
    embedded.embedding,
    Self::entity_to_minimal_payload(&embedded.entity),  // Changed
);
```

#### 2. Minimal Entity Structure

**File**: `crates/storage/src/qdrant/client.rs`
**Changes**: Add struct for deserializing minimal payload

```rust
#[derive(Debug, serde::Deserialize)]
struct MinimalEntityPayload {
    entity_id: String,
    repository_id: String,
    name: String,
    qualified_name: String,
    entity_type: String,
    file_path: String,
    line_range_start: usize,
    line_range_end: usize,
}
```

#### 3. Update search_similar Return Type

**File**: `crates/storage/src/qdrant/client.rs`
**Changes**: Return minimal payloads + scores instead of full entities

Lines 167-199, change signature and implementation:

```rust
/// Search for similar entities by embedding (returns minimal payloads)
pub async fn search_similar_minimal(
    &self,
    query_embedding: Vec<f32>,
    limit: usize,
    filters: Option<SearchFilters>,
) -> Result<Vec<(MinimalEntityPayload, f32)>> {
    let filter = filters.map(|f| self.build_filter(&f));

    let search_result = self
        .client
        .search_points(SearchPoints::from(
            qdrant_client::qdrant::SearchPointsBuilder::new(
                self.collection_name.clone(),
                query_embedding,
                limit as u64,
            )
            .filter(filter.unwrap_or_default())
            .with_payload(true),
        ))
        .await?;

    let mut results = Vec::new();
    for point in search_result.result {
        if !point.payload.is_empty() {
            // Deserialize minimal payload
            let payload = Self::payload_to_minimal_entity(&point.payload)?;
            results.push((payload, point.score));
        }
    }

    Ok(results)
}

fn payload_to_minimal_entity(
    payload: &HashMap<String, QdrantValue>,
) -> Result<MinimalEntityPayload> {
    let mut json_map = serde_json::Map::new();
    for (key, value) in payload {
        if let Ok(json_value) = Self::qdrant_value_to_json(value) {
            json_map.insert(key.clone(), json_value);
        }
    }
    serde_json::from_value(serde_json::Value::Object(json_map))
        .map_err(|e| Error::storage(format!("Failed to deserialize minimal payload: {}", e)))
}
```

#### 4. Add Postgres Batch Fetch

**File**: `crates/storage/src/postgres/client.rs`
**Changes**: Add method to fetch entities by ID list

```rust
/// Batch fetch entities by (repository_id, entity_id) pairs
pub async fn get_entities_by_ids(
    &self,
    entity_refs: &[(uuid::Uuid, String)],
) -> Result<Vec<CodeEntity>> {
    if entity_refs.is_empty() {
        return Ok(Vec::new());
    }

    // Build VALUES clause for batch query
    let mut query = String::from(
        "SELECT entity_data FROM entity_metadata WHERE (repository_id, entity_id) IN ("
    );

    for (i, _) in entity_refs.iter().enumerate() {
        if i > 0 {
            query.push_str(", ");
        }
        query.push_str(&format!("(${}, ${})", i * 2 + 1, i * 2 + 2));
    }
    query.push_str(") AND deleted_at IS NULL");

    // Build query dynamically
    let mut sql_query = sqlx::query(&query);
    for (repo_id, entity_id) in entity_refs {
        sql_query = sql_query.bind(repo_id).bind(entity_id);
    }

    let rows = sql_query.fetch_all(&self.pool).await?;

    let mut entities = Vec::new();
    for row in rows {
        let entity_json: serde_json::Value = row.try_get("entity_data")?;
        let entity: CodeEntity = serde_json::from_value(entity_json)
            .map_err(|e| Error::storage(format!("Failed to deserialize entity: {}", e)))?;
        entities.push(entity);
    }

    Ok(entities)
}
```

#### 5. Update StorageClient Trait

**File**: `crates/storage/src/lib.rs`
**Changes**: Modify search_similar signature at line 43

```rust
#[async_trait]
pub trait StorageClient: Send + Sync {
    async fn bulk_load_entities(
        &self,
        embedded_entities: Vec<EmbeddedEntity>,
    ) -> Result<Vec<(String, uuid::Uuid)>>;

    /// Search returns (entity_id, repository_id, score) tuples
    /// Caller must fetch full entities from Postgres
    async fn search_similar(
        &self,
        query_embedding: Vec<f32>,
        limit: usize,
        filters: Option<SearchFilters>,
    ) -> Result<Vec<(String, String, f32)>>;  // (entity_id, repository_id, score)

    async fn get_entity(&self, entity_id: &str) -> Result<Option<CodeEntity>>;
}
```

#### 6. Update CLI Search Flow

**File**: `crates/cli/src/main.rs`
**Changes**: Update search command (around line 656-698)

```rust
// Search Qdrant for similar vectors
let search_results = storage_client
    .search_similar(query_embedding, limit, filters)
    .await?;

if search_results.is_empty() {
    println!("No similar entities found.");
    return Ok(());
}

// Extract (repository_id, entity_id) pairs
let entity_refs: Vec<(uuid::Uuid, String)> = search_results
    .iter()
    .filter_map(|(entity_id, repo_id, _score)| {
        uuid::Uuid::parse_str(repo_id).ok().map(|uuid| (uuid, entity_id.clone()))
    })
    .collect();

// Batch fetch full entities from Postgres
let full_entities = postgres_client
    .get_entities_by_ids(&entity_refs)
    .await?;

// Create map for lookup
let entity_map: std::collections::HashMap<String, CodeEntity> = full_entities
    .into_iter()
    .map(|e| (e.entity_id.clone(), e))
    .collect();

// Display results with scores
for (idx, (entity_id, _repo_id, score)) in search_results.iter().enumerate() {
    if let Some(entity) = entity_map.get(entity_id) {
        println!("{}. {} ({}% similarity)", idx + 1, entity.name, (score * 100.0) as u32);
        println!("   Type: {:?}", entity.entity_type);
        println!("   File: {}:{}", entity.file_path.display(), entity.line_range.0);
        if let Some(ref content) = entity.content {
            println!("   Preview: {}", &content[..content.len().min(100)]);
        }
    }
}
```

### Success Criteria

#### Automated Verification:
- [ ] Rust compiles: `cargo build --workspace`
- [ ] Search returns results: Test query returns entity_ids
- [ ] Postgres fetch works: Batch fetch returns full entities
- [ ] No content in Qdrant: Inspect payload size < 500 bytes per point

#### Manual Verification:
- [ ] Run search query, verify results displayed correctly
- [ ] Check Qdrant payload in database/admin UI - should be minimal
- [ ] Verify search latency acceptable (measure Qdrant → Postgres roundtrip)
- [ ] Test search with 50+ results, verify batch fetch performs well

---

## Phase 6: Entity Invalidation

### Overview

Implement stale entity detection by comparing file snapshots before/after re-indexing. Complete DELETE outbox operation. Soft-delete removed entities and propagate deletions to Qdrant.

### Changes Required

#### 1. Add Stale Detection to Indexer

**File**: `crates/indexer/src/repository_indexer.rs`
**Changes**: Add method to detect and mark stale entities

```rust
/// Detect and mark stale entities when re-indexing a file
async fn handle_file_change(
    &self,
    repository_id: uuid::Uuid,
    file_path: &Path,
    new_entity_ids: Vec<String>,
    git_commit: Option<String>,
) -> Result<()> {
    let file_path_str = file_path.to_str()
        .ok_or_else(|| Error::indexing("Invalid file path"))?;

    // Get previous snapshot
    let old_entity_ids = self
        .postgres_client
        .get_file_snapshot(repository_id, file_path_str)
        .await?
        .unwrap_or_default();

    // Find stale entities (in old but not in new)
    let stale_ids: Vec<String> = old_entity_ids
        .iter()
        .filter(|old_id| !new_entity_ids.contains(old_id))
        .cloned()
        .collect();

    if !stale_ids.is_empty() {
        info!("Found {} stale entities in {}", stale_ids.len(), file_path_str);

        // Mark entities as deleted
        self.postgres_client
            .mark_entities_deleted(repository_id, &stale_ids)
            .await?;

        // Write DELETE entries to outbox
        for entity_id in &stale_ids {
            let payload = json!({
                "entity_ids": [entity_id],
                "reason": "file_change"
            });

            self.postgres_client
                .write_outbox_entry(
                    repository_id,
                    entity_id,
                    OutboxOperation::Delete,
                    TargetStore::Qdrant,
                    payload,
                )
                .await?;
        }
    }

    // Update snapshot with current state
    self.postgres_client
        .update_file_snapshot(repository_id, file_path_str, new_entity_ids, git_commit)
        .await?;

    Ok(())
}
```

#### 2. Integrate Stale Detection into Indexing

**File**: `crates/indexer/src/repository_indexer.rs`
**Changes**: Call handle_file_change in process_batch

After line 341 (after storing entities), before returning:

```rust
// Detect and handle stale entities per file
for (file_path, entity_ids) in entities_by_file {
    self.handle_file_change(
        repository_id,
        Path::new(&file_path),
        entity_ids,
        git_commit.clone(),
    )
    .await?;
}
```

#### 3. Add mark_entities_deleted to PostgresClient

**File**: `crates/storage/src/postgres/client.rs`
**Changes**: Add method to soft-delete entities

```rust
/// Mark entities as deleted (soft delete)
pub async fn mark_entities_deleted(
    &self,
    repository_id: uuid::Uuid,
    entity_ids: &[String],
) -> Result<()> {
    if entity_ids.is_empty() {
        return Ok(());
    }

    // Build IN clause for batch update
    let mut query = String::from(
        "UPDATE entity_metadata SET deleted_at = NOW(), updated_at = NOW()
         WHERE repository_id = $1 AND entity_id IN ("
    );

    for (i, _) in entity_ids.iter().enumerate() {
        if i > 0 {
            query.push_str(", ");
        }
        query.push_str(&format!("${}", i + 2));
    }
    query.push(')');

    // Execute batch update
    let mut sql_query = sqlx::query(&query).bind(repository_id);
    for entity_id in entity_ids {
        sql_query = sql_query.bind(entity_id);
    }

    let result = sql_query.execute(&self.pool).await?;

    info!("Marked {} entities as deleted", result.rows_affected());

    Ok(())
}
```

#### 4. Implement DELETE Outbox Operation

**File**: `crates/storage/src/postgres/outbox_processor.rs`
**Changes**: Replace stub at lines 104-107

```rust
"DELETE" => {
    // Extract entity IDs from payload
    let entity_ids: Vec<String> = if let Some(ids) = entry.payload.get("entity_ids") {
        serde_json::from_value(ids.clone())
            .map_err(|e| Error::storage(format!("Invalid DELETE payload: {}", e)))?
    } else {
        vec![entry.entity_id.clone()]
    };

    // Delete from Qdrant by entity_id
    self.storage_client.delete_entities(&entity_ids).await?;

    info!("Deleted {} entities from Qdrant", entity_ids.len());
    Ok(())
}
```

#### 5. Add delete_entities to QdrantClient

**File**: `crates/storage/src/qdrant/client.rs`
**Changes**: Add deletion method

```rust
/// Delete entities from Qdrant by entity_id
pub async fn delete_entities(&self, entity_ids: &[String]) -> Result<()> {
    if entity_ids.is_empty() {
        return Ok(());
    }

    // Search for points by entity_id to get point_ids
    let mut point_ids_to_delete = Vec::new();

    for entity_id in entity_ids {
        let filter = Filter {
            must: vec![Condition {
                condition_one_of: Some(ConditionOneOf::Field(FieldCondition {
                    key: "entity_id".to_string(),
                    r#match: Some(Match {
                        match_value: Some(MatchValue::Keyword(entity_id.clone())),
                    }),
                    ..Default::default()
                })),
            }],
            ..Default::default()
        };

        let search_result = self
            .client
            .scroll(ScrollPoints {
                collection_name: self.collection_name.clone(),
                filter: Some(filter),
                limit: Some(10),
                with_payload: Some(false.into()),
                with_vectors: Some(false.into()),
                ..Default::default()
            })
            .await?;

        for point in search_result.result {
            point_ids_to_delete.push(point.id.clone());
        }
    }

    if !point_ids_to_delete.is_empty() {
        self.client
            .delete_points(
                self.collection_name.clone(),
                Some(PointsSelector {
                    points_selector_one_of: Some(PointsSelectorOneOf::Points(
                        PointsIdsList { ids: point_ids_to_delete },
                    )),
                }),
                None,
            )
            .await?;
    }

    Ok(())
}
```

#### 6. Update StorageClient Trait

**File**: `crates/storage/src/lib.rs`
**Changes**: Add delete method to trait at line 51

```rust
#[async_trait]
pub trait StorageClient: Send + Sync {
    // ... existing methods ...

    /// Delete entities from vector store
    async fn delete_entities(&self, entity_ids: &[String]) -> Result<()>;
}
```

### Success Criteria

#### Automated Verification:
- [ ] Rust compiles: `cargo build --workspace`
- [ ] Index file, modify it, re-index: Check stale entities marked deleted
- [ ] Query: `SELECT COUNT(*) FROM entity_metadata WHERE deleted_at IS NOT NULL` > 0
- [ ] Outbox processor runs: Verify DELETE entries processed
- [ ] Qdrant point count decreases after deletion

#### Manual Verification:
- [ ] Index test repository with 10 functions
- [ ] Remove 3 functions from a file, re-index
- [ ] Verify 3 entities have deleted_at timestamp
- [ ] Verify 3 DELETE outbox entries created
- [ ] Run outbox processor, verify entities removed from Qdrant
- [ ] Search doesn't return deleted entities

---

## Testing Strategy

### Unit Tests

**Per Phase Tests**:

1. **Phase 1**: Schema tests
   - Test repository insert/upsert
   - Test entity_metadata with composite PK
   - Test file_entity_snapshots with array field

2. **Phase 2**: Entity ID generation
   - Test `generate_entity_id(repo_id, qualified_name)` produces consistent IDs
   - Test parent traversal builds correct qualified names
   - Test Rust: `"mod::Struct::method"`
   - Test Python: `"ClassName.method_name"`
   - Test anonymous entities get unique IDs

3. **Phase 3**: Repository management
   - Test `ensure_repository()` creates and retrieves
   - Test duplicate init doesn't create duplicate repository

4. **Phase 4**: Storage layer
   - Test `store_entity_metadata()` upserts correctly
   - Test file snapshot tracking
   - Test outbox writing without version_id

5. **Phase 5**: Qdrant payloads
   - Test minimal payload serialization < 500 bytes
   - Test batch fetch from Postgres
   - Test search → fetch pipeline

6. **Phase 6**: Invalidation
   - Test stale detection logic
   - Test soft delete
   - Test DELETE outbox processing

### Integration Tests

**End-to-End Scenarios**:

1. **Fresh Index**:
   - Init new repository
   - Index files
   - Verify all entities in Postgres and Qdrant
   - Search returns correct results

2. **Re-Index (No Changes)**:
   - Index same files again
   - Verify no duplicates (UPDATEs not INSERTs)
   - Verify file snapshots updated

3. **Entity Move**:
   - Index file A with entity X
   - Move entity X to file B
   - Re-index both files
   - Verify entity_id unchanged
   - Verify file_path updated
   - Verify file snapshots correct

4. **Entity Removal**:
   - Index file with 5 entities
   - Remove 2 entities from file
   - Re-index file
   - Verify 2 entities soft-deleted
   - Verify search doesn't return deleted entities

5. **Multi-Repository**:
   - Init two repositories with same file structure
   - Index both
   - Verify entity_ids different (includes repo_id in hash)
   - Verify no collisions

### Manual Testing Steps

**Phase 1**:
1. Run migration: `sqlx migrate run`
2. Check tables exist: `psql -c "\dt"`
3. Insert test data manually, verify constraints

**Phase 2**:
1. Create test Rust file with nested module/impl
2. Run entity extraction
3. Print qualified names and entity IDs
4. Verify format and stability

**Phase 3**:
1. Run `codesearch init` in new repository
2. Check repositories table populated
3. Run `codesearch index`
4. Verify no errors about missing repository

**Phase 4**:
1. Index test repository
2. Query entity_metadata, verify entity_data populated
3. Query file_entity_snapshots, verify arrays
4. Re-index, verify UPDATEs not INSERTs

**Phase 5**:
1. Run search query
2. Measure latency (should be < 100ms for 10 results)
3. Inspect Qdrant payload in admin UI
4. Verify payload < 500 bytes per entity

**Phase 6**:
1. Index file with known entities
2. Remove entity from file
3. Re-index file
4. Check entity_metadata.deleted_at set
5. Run outbox processor
6. Verify entity removed from Qdrant
7. Search doesn't return deleted entity

## Performance Considerations

### Expected Impacts

**Phase 2 (Semantic Paths)**:
- Parent traversal adds ~10-20% to extraction time
- Acceptable tradeoff for stable IDs
- Mitigated by batch processing

**Phase 5 (Two-Phase Retrieval)**:
- Additional Postgres roundtrip after Qdrant search
- Batch fetch optimizes this (single query for all IDs)
- Expected latency increase: 20-50ms
- Consider adding caching layer if needed

**Phase 6 (Invalidation)**:
- File snapshot lookup adds minimal overhead
- DELETE operations are async (outbox pattern)
- No user-facing latency impact

### Optimization Strategies

1. **Batch Operations**: Already implemented (100 files per batch)
2. **Connection Pooling**: sqlx pool configured in storage layer
3. **Indexed Queries**: All lookups use indexed columns
4. **Async Processing**: Outbox pattern keeps main path fast

## Migration Notes

### Breaking Changes

This is a **complete schema rewrite**. No migration path from old schema.

**Users must**:
1. Back up important data (if any)
2. Drop existing database
3. Run new migrations
4. Re-index all repositories

**Migration Command**:
```bash
# Drop old database
psql -c "DROP DATABASE codesearch"

# Create new database
psql -c "CREATE DATABASE codesearch"

# Run new migrations
cd /path/to/codesearch
sqlx migrate run

# Re-initialize repositories
codesearch init

# Re-index
codesearch index
```

### Data Preservation

**Not preserved**:
- Old entity_ids (recalculated without file_path)
- Version history (no longer stored)

**Preserved via re-indexing**:
- Entity content (extracted fresh)
- Git commit correlation (captured during re-index)
- Embeddings (regenerated)

## References

- **Original Research**: `docs/research/data-model-consolidation-postgres.md`
- **GitHub Issue**: Issue #7 - Entity Invalidation
- **Tree-sitter Docs**: Structural path method described in research doc lines 607-630
- **Current Schema**: `migrations/001_initial_schema.sql`
- **Entity ID Logic**: `crates/core/src/entity_id.rs:73-79`
- **Storage Layer**: `crates/storage/src/postgres/client.rs:62-157`
- **Indexer Flow**: `crates/indexer/src/repository_indexer.rs:133-347`
