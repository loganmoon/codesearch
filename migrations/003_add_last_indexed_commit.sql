-- Migration: Add last_indexed_commit tracking
-- Enables efficient catch-up indexing by tracking the last git commit that was indexed

ALTER TABLE repositories
ADD COLUMN last_indexed_commit VARCHAR(40);

CREATE INDEX IF NOT EXISTS idx_repositories_last_indexed_commit
    ON repositories(last_indexed_commit) WHERE last_indexed_commit IS NOT NULL;

COMMENT ON COLUMN repositories.last_indexed_commit IS 'Git commit SHA that was last indexed, used for catch-up indexing on server restart';
