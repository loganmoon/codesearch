-- Migration: Add content column and full-text search
-- Extracts content field from entity_data JSONB to dedicated TEXT column
-- Adds PostgreSQL full-text search with GIN index

-- Add content column (nullable for lazy migration)
ALTER TABLE entity_metadata ADD COLUMN content TEXT;

-- Create GIN index for full-text search
-- Using english configuration for stemming and stop words
CREATE INDEX idx_entity_metadata_content_fts
    ON entity_metadata
    USING GIN (to_tsvector('english', content))
    WHERE deleted_at IS NULL AND content IS NOT NULL;

-- Add comment explaining the column
COMMENT ON COLUMN entity_metadata.content IS
    'Raw content of the code entity. Extracted from entity_data JSONB for performance and full-text search. NULL for entities indexed before this migration (lazy migration).';
