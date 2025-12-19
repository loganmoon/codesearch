-- Migration: Remove full-text search infrastructure
-- FTS functionality has been replaced by Granite sparse embeddings

-- Drop the generated tsvector column used for FTS
ALTER TABLE entity_metadata DROP COLUMN IF EXISTS content_tsv;

-- The content column is kept as it's used for embedding generation and display
COMMENT ON COLUMN entity_metadata.content IS
    'Raw content of the code entity. Used for embedding generation and display.';
