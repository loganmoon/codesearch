-- Migration: Simplified Entity Storage
-- Consolidates PostgreSQL as single source of truth
-- Removes version history, adds repository scoping, enables entity invalidation

-- 1. Create repositories table
CREATE TABLE IF NOT EXISTS repositories (
    repository_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    repository_path TEXT UNIQUE NOT NULL,
    repository_name TEXT NOT NULL,
    collection_name VARCHAR(255) UNIQUE NOT NULL,
    git_remote_url TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_repositories_collection ON repositories(collection_name);

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

CREATE INDEX IF NOT EXISTS idx_entity_metadata_file_path
    ON entity_metadata(repository_id, file_path) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_entity_metadata_qualified_name
    ON entity_metadata(repository_id, qualified_name);
CREATE INDEX IF NOT EXISTS idx_entity_metadata_entity_type
    ON entity_metadata(entity_type);
CREATE INDEX IF NOT EXISTS idx_entity_metadata_deleted_at
    ON entity_metadata(deleted_at) WHERE deleted_at IS NOT NULL;

-- 4. Create file_entity_snapshots for stale detection
CREATE TABLE IF NOT EXISTS file_entity_snapshots (
    repository_id UUID NOT NULL,
    file_path TEXT NOT NULL,
    git_commit_hash VARCHAR(40),
    entity_ids TEXT[] NOT NULL,
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (repository_id, file_path),
    FOREIGN KEY (repository_id) REFERENCES repositories(repository_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_file_entity_snapshots_commit
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

CREATE INDEX IF NOT EXISTS idx_entity_outbox_unprocessed
    ON entity_outbox(target_store, created_at) WHERE processed_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_entity_outbox_entity_id
    ON entity_outbox(repository_id, entity_id);
