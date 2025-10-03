---
date: 2025-10-03T09:34:47-05:00
git_commit: a444e2e6d0d56f750d28193a31a50b7946d20de3
branch: feat/issue-7-stale-entities
repository: codesearch
topic: "Data Model Consolidation - PostgreSQL as Source of Truth for Issue #7"
tags: [research, codebase, storage, postgres, qdrant, versioning, entity-invalidation]
status: complete
last_updated: 2025-10-03
last_updated_note: "Added simplified schema design eliminating version history"
---

# Research: Data Model Consolidation - PostgreSQL as Source of Truth

**Date**: 2025-10-03T09:34:47-05:00
**Git Commit**: a444e2e6d0d56f750d28193a31a50b7946d20de3
**Branch**: feat/issue-7-stale-entities
**Repository**: codesearch

## Research Question

To facilitate implementation of GH issue #7 (entity invalidation), we need to:

1. Consolidate core entity data and metadata into PostgreSQL table(s)
2. Make Qdrant responsible ONLY for searching/ranking based on similarity
3. Make Neo4j (future) responsible ONLY for graph traversals
4. Have query server (future) map entity IDs to canonical data in Postgres
5. Incorporate repository ID as part of entity identification
6. Rethink primary keys, foreign keys, and version handling (version_id, version_number, git_commit_hash)
7. Define what makes an entity unique (entity_id based on qualified name? content? both?)
8. Determine if moved entities should be treated as the same entity

## Summary

The current architecture has Postgres as the metadata/versioning store and Qdrant storing **full entity objects** alongside embeddings. Key findings:

- **Entity IDs are file-path-dependent**: Moving an entity to a different file creates a NEW entity_id
- **No repository scoping**: Entity IDs can collide across different repositories
- **Qdrant stores complete entities**: Not just embeddings - full CodeEntity objects in payload
- **Version complexity**: Three version identifiers (version_id, version_number, git_commit_hash) serve different purposes
- **No deletion logic**: Soft-delete schema exists but entity invalidation is not implemented

## Detailed Findings

### 1. Current PostgreSQL Schema

**Location**: `/home/logan/code/codesearch/migrations/001_initial_schema.sql`

#### entity_metadata Table (lines 5-17)

Primary source of truth for current entity state:

```sql
CREATE TABLE entity_metadata (
    entity_id VARCHAR(64) PRIMARY KEY,           -- Hash of file_path:qualified_name
    file_path TEXT NOT NULL,                     -- Source file location
    qualified_name TEXT NOT NULL,                -- Full qualified name
    entity_type VARCHAR(32) NOT NULL,            -- Function, Class, etc.
    language VARCHAR(32) NOT NULL,               -- Rust, Python, etc.
    qdrant_point_id UUID,                        -- Reference to Qdrant vector
    neo4j_node_id BIGINT,                        -- Reference to Neo4j (future)
    current_version_id UUID NOT NULL,            -- FK to latest version
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ                       -- Soft delete (NOT USED YET)
);
```

**Indexes**:
- `idx_entity_metadata_file_path` on file_path WHERE deleted_at IS NULL (line 19)
- `idx_entity_metadata_qualified_name` on qualified_name (line 20)
- `idx_entity_metadata_entity_type` on entity_type (line 21)
- `idx_entity_metadata_deleted_at` on deleted_at WHERE deleted_at IS NOT NULL (line 22)

**What's Missing**:
- No `repository_id` column
- `deleted_at` exists but is never set (no invalidation logic)

#### entity_versions Table (lines 25-38)

Complete version history with Git correlation:

```sql
CREATE TABLE entity_versions (
    version_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),  -- Unique version identifier
    entity_id VARCHAR(64) NOT NULL,                         -- Parent entity reference
    version_number INT NOT NULL,                            -- Sequential counter (1, 2, 3...)
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),          -- When indexed
    git_commit_hash VARCHAR(40),                            -- Git SHA-1 (nullable)
    file_path TEXT NOT NULL,                                -- Snapshot of file path
    qualified_name TEXT NOT NULL,                           -- Snapshot of qualified name
    entity_type VARCHAR(32) NOT NULL,                       -- Snapshot of type
    language VARCHAR(32) NOT NULL,                          -- Snapshot of language
    entity_data JSONB NOT NULL,                             -- Full CodeEntity as JSON
    line_range INT4RANGE NOT NULL,                          -- PostgreSQL range type
    UNIQUE (entity_id, version_number)
);
```

**Indexes**:
- `idx_entity_versions_entity_id` on entity_id (line 40)
- `idx_entity_versions_file_path` on file_path (line 41)
- `idx_entity_versions_indexed_at` on indexed_at DESC (line 42)

**Design**: Full event sourcing pattern - every entity change creates new version record

#### entity_outbox Table (lines 45-56)

Transactional outbox pattern for eventual consistency:

```sql
CREATE TABLE entity_outbox (
    outbox_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    entity_id VARCHAR(64) NOT NULL,
    operation VARCHAR(10) NOT NULL CHECK (operation IN ('INSERT', 'UPDATE', 'DELETE')),
    target_store VARCHAR(32) NOT NULL CHECK (target_store IN ('qdrant', 'neo4j')),
    payload JSONB NOT NULL,
    version_id UUID NOT NULL,                               -- FK to entity_versions
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMPTZ,                               -- NULL = pending
    retry_count INT NOT NULL DEFAULT 0,
    last_error TEXT
);
```

**Purpose**: Ensures Postgres writes succeed before async sync to Qdrant/Neo4j

**Critical Finding**: DELETE operation exists in schema but not implemented (storage/src/postgres/outbox_processor.rs:104-107)

### 2. Entity Identity and Uniqueness

**Location**: `/home/logan/code/codesearch/crates/core/src/entity_id.rs`

#### Current Entity ID Algorithm (lines 73-79)

```rust
pub fn generate_entity_id_from_qualified_name(qualified_name: &str, file_path: &Path) -> String {
    let unique_str = format!("{}:{}", file_path.display(), qualified_name);
    format!("entity-{:032x}", XxHash3_128::oneshot(unique_str.as_bytes()))
}
```

**Hash Input**: `"{file_path}:{qualified_name}"`
**Output**: `"entity-" + 32 hex chars` (128-bit XXH3 hash)
**Example**: `src/main.rs:main` → `entity-a1b2c3d4e5f6...`

#### Critical Issue: File-Path Dependency

**When entity moves to different file**:
1. Old file: `src/foo.rs:MyStruct::method` → `entity-abc123...`
2. New file: `src/bar.rs:MyStruct::method` → `entity-def456...` (DIFFERENT!)

**Result**: System treats it as two separate entities:
- Old entity remains in database (appears deleted if file removed)
- New entity created with different ID
- Version history is NOT preserved across moves
- No detection that these are the same logical entity

#### No Repository Scoping

**Problem**: Same file path + qualified name in different repositories produces IDENTICAL entity_id

Example:
- Repo A: `src/lib.rs:main` → `entity-xyz...`
- Repo B: `src/lib.rs:main` → `entity-xyz...` (COLLISION!)

**Why**: Entity ID generation has no repository context (entity_id.rs:73-79)

### 3. Qdrant Data Duplication

**Location**: `/home/logan/code/codesearch/crates/storage/src/qdrant/client.rs`

#### Full Entity Storage in Qdrant (lines 34-45)

```rust
fn entity_to_payload(entity: &CodeEntity) -> Payload {
    // Serializes ENTIRE CodeEntity to JSON payload
    if let Ok(json) = serde_json::to_value(entity) {
        if let Ok(map) = serde_json::from_value::<...>(json) {
            return Payload::from(map);
        }
    }
    Payload::from(serde_json::Map::new())
}
```

**What's Stored in Qdrant**:
- Embedding vector (f32 array)
- **Complete CodeEntity** in payload:
  - entity_id, name, qualified_name, parent_scope
  - entity_type, dependencies, documentation_summary
  - file_path, location, line_range
  - visibility, language, signature
  - content (full source code), metadata

#### Search Results Return Full Entities (lines 167-199)

```rust
async fn search_similar(...) -> Result<Vec<(CodeEntity, f32)>> {
    // Search with .with_payload(true)
    // Deserialize entities directly from Qdrant payload
    // NO lookup to Postgres needed
}
```

**Current Pattern**: Qdrant is NOT just a vector index - it's a complete entity store

**Implication**: Query server would need to fetch from Qdrant OR Postgres (redundant data)

### 4. Version Handling Complexity

#### Three Version Identifiers

**Location**: `/home/logan/code/codesearch/crates/storage/src/postgres/client.rs:62-157`

1. **version_id** (UUID):
   - Generated by Postgres via uuid_generate_v4()
   - Primary key in entity_versions table
   - Used as foreign key in entity_metadata and entity_outbox
   - Globally unique across all entities

2. **version_number** (INT):
   - Calculated at storage time: `SELECT COALESCE(MAX(version_number), 0) + 1 WHERE entity_id = $1` (lines 82-88)
   - Sequential counter per entity (1, 2, 3, ...)
   - Human-readable version identifier
   - Unique per entity via constraint

3. **git_commit_hash** (VARCHAR(40)):
   - Git SHA-1 hash at indexing time
   - Nullable (supports non-Git repos)
   - Ties entity versions to source control state
   - Enables point-in-time queries

**Why Three?**
- **version_id**: Database internal reference (joins, foreign keys)
- **version_number**: User-facing ordered history per entity
- **git_commit_hash**: External correlation to source control

**Is This Confusing?** Potentially, but each serves a distinct purpose:
- version_id = system identifier
- version_number = sequence identifier
- git_commit_hash = source control identifier

### 5. Storage Flow Analysis

#### Write Path (indexer/src/repository_indexer.rs:300-341)

```
Entity Extraction
    ↓
store_entity_metadata(entity, uuid, git_commit)  [Postgres]
    ↓ (lines 309-320)
- Calculate version_number (MAX + 1)
- Insert entity_versions record
- Upsert entity_metadata (ON CONFLICT update current_version_id)
    ↓
write_outbox_entry(INSERT, Qdrant, payload, version_id)
    ↓ (async via outbox processor)
bulk_load_entities(embedded_entities)  [Qdrant]
    ↓ (lines 132-144)
- Generate random point_id (UUID)
- Store embedding vector + FULL entity payload
```

**Key Finding**: Postgres stores entity_data in JSONB, then SAME data sent to Qdrant payload

#### Read Path (cli/src/main.rs:656-698)

```
Query Embedding Generated
    ↓
search_similar(query_embedding, limit, filters)  [Qdrant]
    ↓
Returns Vec<(CodeEntity, f32)>  [from Qdrant payload]
    ↓
Display results (NO Postgres lookup needed)
```

**Current Reality**: Search results don't touch Postgres at all

### 6. Missing Entity Invalidation

**Issue #7**: "The indexer must remove modified/removed entities from storage when a file changes"

#### What Exists

- **Soft delete column**: entity_metadata.deleted_at (schema line 16)
- **Query filtering**: `WHERE deleted_at IS NULL` (storage/src/postgres/client.rs:162)
- **File change events**: FileChange::Deleted enum (watcher/src/events.rs:19)
- **Outbox DELETE operation**: Schema supports it (schema line 48)

#### What's Missing

1. **No deletion logic**: Nothing sets deleted_at timestamp
2. **No entity diffing**: No comparison of old AST vs new AST by ID
3. **No AST cache**: Issue #7 mentions need for AST cache to detect removals
4. **DELETE outbox stubbed**: outbox_processor.rs:104-107 warns "not yet implemented"
5. **No watch command**: File change events detected but not processed

**Gap for Issue #7**: When file changes:
- Old entities in file are NOT identified
- No lookup of previous entity_ids for file
- No marking of removed entities as deleted
- No removal from Qdrant

## Architecture Insights

### Current State: Dual-Storage Pattern

- **Postgres**: Source of truth for metadata, versions, Git history
- **Qdrant**: Vector search + complete entity storage (duplicated data)
- **Pattern**: Transactional outbox for eventual consistency

### Proposed State: Separation of Concerns

**Goal from research prompt**:
- **Postgres**: ALL canonical entity data and metadata
- **Qdrant**: ONLY embeddings + entity_id for mapping back
- **Neo4j**: ONLY graph relationships (future)
- **Query Server**: Maps IDs from Qdrant/Neo4j → entity data in Postgres

### Key Design Decisions Needed

#### 1. Repository Scoping

**Current**: No repository_id in schema or entity_id generation

**Options**:
- Add `repository_id` column to entity_metadata
- Change entity_id formula: hash(`repository_id:file_path:qualified_name`)
- Use database schemas per repository (PostgreSQL namespaces)

**Implication**: This is a BREAKING CHANGE - all existing entity_ids would be invalid

#### 2. Entity Identity Across File Moves

**Current**: Moving entity to new file = new entity_id (file_path in hash)

**Options**:
- Hash based ONLY on qualified_name (not file_path)
  - Pro: Same entity across moves
  - Con: Collisions possible (same name in different files)
- Add `moved_from_entity_id` field to track renames
  - Pro: Preserves history
  - Con: Complex migration logic needed
- Content-based hashing (hash of AST structure)
  - Pro: Detects identical implementations
  - Con: Different ID when implementation changes (confusing)

**Recommendation**: Keep file_path in hash BUT add entity move detection logic

#### 3. Qdrant Payload Reduction

**Current**: Complete CodeEntity stored in Qdrant payload

**Options**:
- Store ONLY entity_id in payload
  - Pro: Single source of truth in Postgres
  - Con: Every search requires Postgres lookup (performance impact)
- Store minimal metadata (entity_id, type, file_path)
  - Pro: Basic filtering without Postgres lookup
  - Con: Still some duplication
- Hybrid: Store display fields only (name, type, file_path) but not content/metadata
  - Pro: Fast display of results, fetch details on demand
  - Con: Two-phase retrieval for full data

**Recommendation**: Store entity_id + minimal display metadata, fetch full data from Postgres on demand

#### 4. Version Handling Simplification

**Current**: version_id (UUID) + version_number (INT) + git_commit_hash (VARCHAR)

**Options**:
- Keep all three
  - Pro: Each serves distinct purpose
  - Con: Perceived complexity
- Remove version_id, use (entity_id, version_number) as composite key
  - Pro: Simpler schema
  - Con: Foreign keys become composite (more complex queries)
- Use git_commit_hash as version identifier
  - Pro: Direct Git correlation
  - Con: Doesn't work for non-Git repos, nullability issues

**Recommendation**: KEEP all three - they serve different purposes:
- version_id: Internal database reference (efficient joins)
- version_number: User-facing sequence (easy to understand)
- git_commit_hash: External system correlation (Git integration)

## Code References

### Entity ID Generation
- `core/src/entity_id.rs:73-79` - Named entity ID generation
- `core/src/entity_id.rs:96-113` - Anonymous entity ID generation
- `core/src/entities.rs:92-146` - CodeEntity struct definition

### PostgreSQL Schema & Storage
- `migrations/001_initial_schema.sql:5-17` - entity_metadata table
- `migrations/001_initial_schema.sql:25-38` - entity_versions table
- `migrations/001_initial_schema.sql:45-56` - entity_outbox table
- `storage/src/postgres/client.rs:62-157` - store_entity_metadata()
- `storage/src/postgres/client.rs:160-170` - get_entities_for_file()
- `storage/src/postgres/client.rs:173-188` - get_entity_versions()

### Qdrant Storage
- `storage/src/qdrant/client.rs:34-45` - entity_to_payload() (full entity)
- `storage/src/qdrant/client.rs:124-157` - bulk_load_entities()
- `storage/src/qdrant/client.rs:167-199` - search_similar() (returns full entities)

### Version Management
- `storage/src/postgres/client.rs:82-88` - Version number calculation
- `indexer/src/repository_indexer.rs:300-341` - Indexing with Git commit capture
- `watcher/src/git.rs:74-83` - Git commit hash retrieval

### Missing Entity Invalidation
- `watcher/src/events.rs:13-24` - FileChange enum (includes Deleted)
- `storage/src/postgres/outbox_processor.rs:104-107` - DELETE operation stubbed out

## Implementation Recommendations for Issue #7

Based on the research, here's the recommended approach for entity invalidation:

### Phase 1: Add Repository Scoping

1. **Add repositories table**:
```sql
CREATE TABLE repositories (
    repository_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    repository_path TEXT UNIQUE NOT NULL,
    repository_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

2. **Add repository_id to entity_metadata**:
```sql
ALTER TABLE entity_metadata ADD COLUMN repository_id UUID NOT NULL REFERENCES repositories(repository_id);
-- Add to primary key or create unique index on (repository_id, entity_id)
```

3. **Update entity_id generation** to include repository_id:
```rust
pub fn generate_entity_id(repository_id: &str, qualified_name: &str, file_path: &Path) -> String {
    let unique_str = format!("{}:{}:{}", repository_id, file_path.display(), qualified_name);
    format!("entity-{:032x}", XxHash3_128::oneshot(unique_str.as_bytes()))
}
```

### Phase 2: Implement Entity Invalidation

1. **Add entity tracking per file**:
```sql
CREATE TABLE file_entity_snapshots (
    file_path TEXT NOT NULL,
    repository_id UUID NOT NULL,
    git_commit_hash VARCHAR(40),
    entity_ids TEXT[] NOT NULL,  -- Array of entity_ids in file
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (repository_id, file_path, git_commit_hash)
);
```

2. **Implement entity diffing logic**:
```rust
async fn invalidate_stale_entities(&self, file_path: &Path, new_entity_ids: &[String]) -> Result<()> {
    // Get previous entity_ids for file
    let old_entity_ids = self.get_entities_for_file(file_path).await?;

    // Identify removed entities
    let removed: Vec<_> = old_entity_ids.iter()
        .filter(|id| !new_entity_ids.contains(id))
        .collect();

    // Mark as deleted
    for entity_id in removed {
        self.soft_delete_entity(entity_id).await?;
        self.write_outbox_entry(entity_id, OutboxOperation::Delete, TargetStore::Qdrant, ...).await?;
    }
}
```

3. **Implement soft_delete_entity()**:
```rust
async fn soft_delete_entity(&self, entity_id: &str) -> Result<()> {
    sqlx::query("UPDATE entity_metadata SET deleted_at = NOW() WHERE entity_id = $1")
        .bind(entity_id)
        .execute(&self.pool)
        .await?;
    Ok(())
}
```

4. **Implement Qdrant DELETE operation** (outbox_processor.rs:104-107):
```rust
"DELETE" => {
    let entity_id = payload.get("entity_id").ok_or(...)?;
    let point_id = payload.get("qdrant_point_id").ok_or(...)?;
    self.qdrant_client.delete_point(point_id).await?;
    Ok(())
}
```

### Phase 3: Reduce Qdrant Payload

1. **Change entity_to_payload() to store minimal data**:
```rust
fn entity_to_minimal_payload(entity: &CodeEntity) -> Payload {
    let mut map = serde_json::Map::new();
    map.insert("entity_id".to_string(), json!(entity.entity_id));
    map.insert("name".to_string(), json!(entity.name));
    map.insert("entity_type".to_string(), json!(entity.entity_type));
    map.insert("file_path".to_string(), json!(entity.file_path));
    // Don't include: content, signature, metadata, dependencies
    Payload::from(map)
}
```

2. **Update search_similar() to fetch full data from Postgres**:
```rust
async fn search_similar_with_details(...) -> Result<Vec<(CodeEntity, f32)>> {
    let qdrant_results = self.qdrant_client.search_similar(...).await?;
    let entity_ids: Vec<_> = qdrant_results.iter().map(|(e, _)| &e.entity_id).collect();

    // Fetch full entities from Postgres
    let full_entities = self.postgres_client.get_entities_by_ids(&entity_ids).await?;

    // Merge scores with full data
    Ok(merge_results(full_entities, qdrant_results))
}
```

### Phase 4: File Change Integration

Implement watch command that:
1. Listens for FileChange events
2. On Modified: Re-index file, call invalidate_stale_entities()
3. On Deleted: Mark all entities in file as deleted
4. On Renamed: Update file_path for all entities (preserve entity_ids)

## Open Questions

1. **Migration strategy**: How to migrate existing entity_ids when adding repository_id to hash?
   - Option A: Re-generate all IDs (breaks external references)
   - Option B: Add repository_id column but keep old entity_id format (partial solution)
   - Option C: Create entity_id_v2 column, gradually migrate

2. **Performance impact**: How much latency does Postgres lookup add to search results?
   - Need benchmarks: Qdrant-only vs Qdrant→Postgres
   - Consider caching layer for frequently accessed entities

3. **Entity move detection**: Should the system detect when entities move between files?
   - If yes: Need content-based matching or qualified-name tracking
   - If no: Moves appear as delete + create (version history lost)

4. **Neo4j data model**: What relationships will be stored?
   - Calls-to, inherits-from, imports, etc.
   - How to keep graph in sync with Postgres canonical data

5. **Composite keys**: Should (repository_id, entity_id) become the primary key?
   - Pro: Explicit repository scoping
   - Con: More complex foreign keys and queries

## Related Research

- None (initial research document)

## Next Steps

1. Decide on repository scoping approach (add repository_id to entity_id hash or separate column)
2. Create migration for adding repositories table and repository_id column
3. Implement soft_delete_entity() and invalidate_stale_entities() logic
4. Complete DELETE outbox operation for Qdrant
5. Reduce Qdrant payload to minimal metadata
6. Implement watch command for real-time entity invalidation
7. Add entity move detection if desired (optional enhancement)

---

## Follow-up Research 2025-10-03T10:00:05-05:00

### Design Decisions Confirmed

#### 1. Eliminate Version History

**Decision**: Remove entity_versions table and all version tracking (version_id, version_number, git_commit_hash).

**Rationale**:
- No real value in querying "how did function X look 5 commits ago"
- Git history already provides this capability
- Version history not needed for issue #7 (only need to detect stale entities)
- Significant complexity reduction

**What's Needed Instead**:
- Current entity state only (entity_metadata)
- File snapshots for diffing (file_entity_snapshots)
- Git commit correlation for traceability (single field in entity_metadata)

#### 2. Semantic Structural Path for Entity IDs

**Decision**: Use **semantic structural path only** (repository_id + qualified_name) without file_path.

**Rationale**:
- Tree-sitter provides tools to build semantic identifiers via parent traversal
- Structural path remains stable even when code moves between files
- Qualified name like `geometry::Point::new` is the true entity identity
- File path is just an attribute (location), not part of identity

**Algorithm** (from Tree-sitter best practices):

```rust
// Build qualified name by traversing up AST tree
// Example: function_item -> impl_item -> struct_item -> mod_item -> source_file
// Result: "geometry::Point::new"

pub fn generate_entity_id(repository_id: &str, qualified_name: &str) -> String {
    let unique_str = format!("{}:{}", repository_id, qualified_name);
    format!("entity-{:032x}", XxHash3_128::oneshot(unique_str.as_bytes()))
}
```

**Benefits**:
- Entity keeps same ID when moved to different file
- Version history preserved across refactoring (if we add it back later)
- True semantic identity (not location-based)
- Cleaner mental model

**Edge Cases**:
- Anonymous constructs: Use structural path + index (e.g., `MyClass::my_method.closure[0]`)
- Function overloading: Include parameter types if language supports it
- True collisions: Rare in well-formed code, error or use content hash as tiebreaker

### Simplified Schema Design

#### Core Tables

**1. repositories**
```sql
CREATE TABLE repositories (
    repository_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    repository_path TEXT UNIQUE NOT NULL,
    repository_name TEXT NOT NULL,
    git_remote_url TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

**2. entity_metadata** (simplified - no version fields)
```sql
CREATE TABLE entity_metadata (
    entity_id VARCHAR(64) NOT NULL,                      -- Hash of repository_id:qualified_name
    repository_id UUID NOT NULL,
    qualified_name TEXT NOT NULL,
    name TEXT NOT NULL,                                  -- Simple name
    parent_scope TEXT,                                   -- Parent qualified name
    entity_type VARCHAR(32) NOT NULL,
    language VARCHAR(32) NOT NULL,
    file_path TEXT NOT NULL,                             -- Current location (not part of ID!)
    line_range INT4RANGE NOT NULL,
    visibility VARCHAR(32) NOT NULL,
    entity_data JSONB NOT NULL,                          -- Full CodeEntity
    git_commit_hash VARCHAR(40),                         -- Current commit (for traceability)
    qdrant_point_id UUID,
    neo4j_node_id BIGINT,
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ,
    PRIMARY KEY (repository_id, entity_id),
    FOREIGN KEY (repository_id) REFERENCES repositories(repository_id) ON DELETE CASCADE
);

CREATE INDEX idx_entity_metadata_file_path ON entity_metadata(repository_id, file_path) WHERE deleted_at IS NULL;
CREATE INDEX idx_entity_metadata_qualified_name ON entity_metadata(repository_id, qualified_name);
CREATE INDEX idx_entity_metadata_entity_type ON entity_metadata(entity_type);
CREATE INDEX idx_entity_metadata_deleted_at ON entity_metadata(deleted_at) WHERE deleted_at IS NOT NULL;
```

**Key Changes**:
- Composite PK: (repository_id, entity_id)
- No current_version_id field
- Single git_commit_hash for traceability (not versioning)
- file_path is just an attribute (indexed for lookup, but not part of entity identity)

**3. file_entity_snapshots** (for stale entity detection)
```sql
CREATE TABLE file_entity_snapshots (
    repository_id UUID NOT NULL,
    file_path TEXT NOT NULL,
    git_commit_hash VARCHAR(40),
    entity_ids TEXT[] NOT NULL,                          -- Array of entity_ids in this file
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (repository_id, file_path),
    FOREIGN KEY (repository_id) REFERENCES repositories(repository_id) ON DELETE CASCADE
);

CREATE INDEX idx_file_entity_snapshots_commit ON file_entity_snapshots(repository_id, git_commit_hash);
```

**Purpose**: Track which entities exist in each file for diffing. When file changes:
1. Load previous entity_ids from this table
2. Extract new entities from updated file
3. Compare: removed = old - new, added = new - old, updated = intersection
4. Mark removed entities as deleted

**4. entity_outbox** (simplified - no version_id)
```sql
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
    FOREIGN KEY (repository_id, entity_id) REFERENCES entity_metadata(repository_id, entity_id) ON DELETE CASCADE
);

CREATE INDEX idx_entity_outbox_unprocessed ON entity_outbox(target_store, created_at) WHERE processed_at IS NULL;
CREATE INDEX idx_entity_outbox_entity_id ON entity_outbox(repository_id, entity_id);
```

**Key Changes**:
- No version_id field
- Foreign key to (repository_id, entity_id)

### Updated Implementation Flow

#### Entity Extraction & Storage

```rust
// 1. Build qualified name via Tree-sitter parent traversal
let qualified_name = build_qualified_name_from_ast(node, source);
// Result: "geometry::Point::new"

// 2. Generate stable entity_id (no file_path!)
let entity_id = generate_entity_id(repository_id, &qualified_name);
// Result: "entity-abc123..." (stable even if file moves)

// 3. Store entity with current file_path as attribute
postgres_client.store_entity(
    repository_id,
    entity_id,
    CodeEntity {
        entity_id: entity_id.clone(),
        qualified_name: qualified_name.clone(),
        file_path: current_file_path,  // Just an attribute
        // ... other fields
    },
    git_commit_hash,
).await?;

// 4. Update file snapshot
postgres_client.update_file_snapshot(
    repository_id,
    current_file_path,
    vec![entity_id],  // All entity_ids in this file
    git_commit_hash,
).await?;

// 5. Queue for Qdrant sync
postgres_client.write_outbox_entry(
    repository_id,
    entity_id,
    OutboxOperation::Insert,
    TargetStore::Qdrant,
    minimal_payload,  // Only entity_id + display fields
).await?;
```

#### Stale Entity Detection (Issue #7)

```rust
async fn invalidate_stale_entities(
    &self,
    repository_id: Uuid,
    file_path: &Path,
    new_entity_ids: &[String],
) -> Result<()> {
    // 1. Get previous snapshot
    let snapshot = self.get_file_snapshot(repository_id, file_path).await?;
    let old_entity_ids = snapshot.map(|s| s.entity_ids).unwrap_or_default();

    // 2. Identify removed entities
    let removed: Vec<_> = old_entity_ids
        .iter()
        .filter(|id| !new_entity_ids.contains(id))
        .collect();

    // 3. Mark as deleted
    for entity_id in removed {
        sqlx::query(
            "UPDATE entity_metadata
             SET deleted_at = NOW()
             WHERE repository_id = $1 AND entity_id = $2"
        )
        .bind(repository_id)
        .bind(entity_id)
        .execute(&self.pool)
        .await?;

        // Queue for Qdrant deletion
        self.write_outbox_entry(
            repository_id,
            entity_id,
            OutboxOperation::Delete,
            TargetStore::Qdrant,
            json!({"entity_id": entity_id, "qdrant_point_id": ...}),
        ).await?;
    }

    // 4. Update snapshot with new state
    self.update_file_snapshot(repository_id, file_path, new_entity_ids, git_commit_hash).await?;

    Ok(())
}
```

#### File Move Handling (Bonus)

When entity moves from `src/foo.rs` to `src/bar.rs`:

```rust
// Old file: src/foo.rs previously contained entity_id "entity-abc123..."
// New file: src/bar.rs now contains same qualified_name "geometry::Point::new"

// 1. Extract entities from new file
let entities = extract_entities("src/bar.rs");
let new_entity = entities.iter().find(|e| e.qualified_name == "geometry::Point::new");

// 2. Generate entity_id (SAME as before, no file_path in hash!)
let entity_id = generate_entity_id(repository_id, "geometry::Point::new");
// Result: "entity-abc123..." (unchanged!)

// 3. Upsert entity metadata (updates file_path)
sqlx::query(
    "INSERT INTO entity_metadata (..., file_path, ...)
     VALUES (..., $1, ...)
     ON CONFLICT (repository_id, entity_id)
     DO UPDATE SET file_path = $1, updated_at = NOW()"
)
.bind("src/bar.rs")  // New file path
.execute(&pool)
.await?;

// 4. Update both file snapshots
update_file_snapshot(repository_id, "src/foo.rs", []); // No longer contains entity
update_file_snapshot(repository_id, "src/bar.rs", [entity_id]); // Now contains entity
```

**Result**: Entity keeps same ID, just updates its location. No deletion, no re-creation, history preserved!

### Qdrant Minimal Payload

Store only what's needed for search result display:

```rust
fn entity_to_minimal_payload(entity: &CodeEntity) -> Payload {
    let mut map = serde_json::Map::new();
    map.insert("entity_id".to_string(), json!(entity.entity_id));
    map.insert("repository_id".to_string(), json!(entity.repository_id));
    map.insert("name".to_string(), json!(entity.name));
    map.insert("qualified_name".to_string(), json!(entity.qualified_name));
    map.insert("entity_type".to_string(), json!(entity.entity_type));
    map.insert("file_path".to_string(), json!(entity.file_path.display().to_string()));
    map.insert("line_range_start".to_string(), json!(entity.line_range.0));
    map.insert("line_range_end".to_string(), json!(entity.line_range.1));

    // DO NOT include: content, signature, dependencies, metadata
    Payload::from(map)
}
```

For full entity details, fetch from Postgres:

```rust
async fn search_with_details(
    query_embedding: Vec<f32>,
    limit: usize,
) -> Result<Vec<(CodeEntity, f32)>> {
    // 1. Search Qdrant (returns minimal payload + scores)
    let search_results = qdrant_client.search_similar(query_embedding, limit).await?;

    // 2. Extract entity IDs
    let entity_ids: Vec<(Uuid, String)> = search_results
        .iter()
        .map(|(payload, _)| (payload.repository_id, payload.entity_id))
        .collect();

    // 3. Batch fetch from Postgres
    let full_entities = postgres_client.get_entities_by_ids(&entity_ids).await?;

    // 4. Merge with scores
    let results: Vec<(CodeEntity, f32)> = full_entities
        .into_iter()
        .zip(search_results.iter().map(|(_, score)| *score))
        .collect();

    Ok(results)
}
```

### Migration Strategy

**Breaking change is acceptable**, so:

1. Create new schema with new tables
2. Create new migration: `002_simplified_entity_storage.sql`
3. Re-index all repositories to populate new schema
4. Drop old entity_versions table
5. All existing entity_ids will be regenerated (repository_id:qualified_name only)

### Summary of Changes

**Eliminated**:
- ❌ entity_versions table
- ❌ version_id field
- ❌ version_number field
- ❌ current_version_id foreign key
- ❌ Version calculation logic
- ❌ File path in entity_id hash

**Added**:
- ✅ repositories table
- ✅ repository_id in entity_id hash
- ✅ file_entity_snapshots table for diffing
- ✅ Composite PK (repository_id, entity_id)
- ✅ Single git_commit_hash for traceability
- ✅ Semantic structural path approach

**Simplified**:
- Single source of truth: entity_metadata
- No version history complexity
- File path is attribute, not identity
- Cleaner schema with fewer tables
- Entity moves don't break identity

**Next Implementation Steps**:
1. Update entity_id generation to use repository_id:qualified_name
2. Implement Tree-sitter structural path building for qualified names
3. Create new migration with simplified schema
4. Implement file_entity_snapshots tracking
5. Implement invalidate_stale_entities() logic
6. Complete DELETE outbox operation
7. Update Qdrant payload to minimal fields
8. Add Postgres lookup after Qdrant search
