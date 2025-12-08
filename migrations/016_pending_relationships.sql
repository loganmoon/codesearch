-- Pending relationships table for tracking unresolved entity relationships
-- This replaces the Neo4j property-based tracking (unresolved_contains_parent, etc.)
-- and enables efficient PostgreSQL JOIN-based resolution

CREATE TABLE IF NOT EXISTS pending_relationships (
    id SERIAL PRIMARY KEY,
    repository_id UUID NOT NULL REFERENCES repositories(repository_id) ON DELETE CASCADE,
    source_entity_id VARCHAR(255) NOT NULL,
    relationship_type VARCHAR(50) NOT NULL,  -- 'CONTAINS', 'CALLS', 'IMPLEMENTS', etc.
    target_qualified_name TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(repository_id, source_entity_id, relationship_type, target_qualified_name)
);

-- Index for querying pending relationships by repository
CREATE INDEX IF NOT EXISTS idx_pending_rel_repo ON pending_relationships(repository_id);

-- Index for efficient JOIN resolution (find entities matching target_qualified_name)
CREATE INDEX IF NOT EXISTS idx_pending_rel_target ON pending_relationships(repository_id, target_qualified_name);

COMMENT ON TABLE pending_relationships IS
    'Tracks unresolved entity relationships that will be resolved when target entities become available. Resolution queries PostgreSQL entities table via JOIN on qualified_name.';

COMMENT ON COLUMN pending_relationships.source_entity_id IS
    'Entity ID of the source node for the relationship';

COMMENT ON COLUMN pending_relationships.relationship_type IS
    'Neo4j relationship type: CONTAINS, CALLS, IMPLEMENTS, INHERITS_FROM, USES, IMPORTS, etc.';

COMMENT ON COLUMN pending_relationships.target_qualified_name IS
    'Qualified name of the target entity. Used for JOIN-based resolution against entities.qualified_name';
