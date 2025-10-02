-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Entity metadata table (source of truth for current state)
CREATE TABLE entity_metadata (
    entity_id VARCHAR(64) PRIMARY KEY,
    file_path TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    entity_type VARCHAR(32) NOT NULL,
    language VARCHAR(32) NOT NULL,
    qdrant_point_id UUID,
    neo4j_node_id BIGINT,
    current_version_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX idx_entity_metadata_file_path ON entity_metadata(file_path) WHERE deleted_at IS NULL;
CREATE INDEX idx_entity_metadata_qualified_name ON entity_metadata(qualified_name);
CREATE INDEX idx_entity_metadata_entity_type ON entity_metadata(entity_type);
CREATE INDEX idx_entity_metadata_deleted_at ON entity_metadata(deleted_at) WHERE deleted_at IS NOT NULL;

-- Entity versions table (complete history)
CREATE TABLE entity_versions (
    version_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    entity_id VARCHAR(64) NOT NULL,
    version_number INT NOT NULL,
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    git_commit_hash VARCHAR(40),
    file_path TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    entity_type VARCHAR(32) NOT NULL,
    language VARCHAR(32) NOT NULL,
    entity_data JSONB NOT NULL,
    line_range INT4RANGE NOT NULL,
    UNIQUE (entity_id, version_number)
);

CREATE INDEX idx_entity_versions_entity_id ON entity_versions(entity_id);
CREATE INDEX idx_entity_versions_file_path ON entity_versions(file_path);
CREATE INDEX idx_entity_versions_indexed_at ON entity_versions(indexed_at DESC);

-- Outbox pattern table (transactional writes)
CREATE TABLE entity_outbox (
    outbox_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    entity_id VARCHAR(64) NOT NULL,
    operation VARCHAR(10) NOT NULL CHECK (operation IN ('INSERT', 'UPDATE', 'DELETE')),
    target_store VARCHAR(32) NOT NULL CHECK (target_store IN ('qdrant', 'neo4j')),
    payload JSONB NOT NULL,
    version_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMPTZ,
    retry_count INT NOT NULL DEFAULT 0,
    last_error TEXT
);

CREATE INDEX idx_entity_outbox_unprocessed ON entity_outbox(target_store, created_at)
    WHERE processed_at IS NULL;
CREATE INDEX idx_entity_outbox_entity_id ON entity_outbox(entity_id);

-- Add foreign key after entity_versions table exists
ALTER TABLE entity_metadata
    ADD CONSTRAINT fk_entity_metadata_current_version
    FOREIGN KEY (current_version_id)
    REFERENCES entity_versions(version_id);

ALTER TABLE entity_outbox
    ADD CONSTRAINT fk_entity_outbox_version
    FOREIGN KEY (version_id)
    REFERENCES entity_versions(version_id);
