-- Add pending_relationship_resolution flag to track when relationships need to be resolved
ALTER TABLE repositories ADD COLUMN IF NOT EXISTS pending_relationship_resolution BOOLEAN DEFAULT FALSE;

-- Create index for pending resolution queries
CREATE INDEX IF NOT EXISTS idx_repositories_pending_relationship_resolution
    ON repositories(pending_relationship_resolution)
    WHERE pending_relationship_resolution = TRUE;

-- Add comment explaining the column
COMMENT ON COLUMN repositories.pending_relationship_resolution IS
    'True when entities have been added to Neo4j but relationships need to be resolved. Set by outbox processor after entity creation, cleared after resolution completes.';
