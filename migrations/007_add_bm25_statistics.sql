-- Migration: Add BM25 Statistics for Hybrid Search
-- Adds columns to track BM25 avgdl (average document length) per repository
-- and token counts per entity for incremental avgdl maintenance

-- Add BM25 statistics columns to repositories table
ALTER TABLE repositories ADD COLUMN bm25_avgdl REAL;
ALTER TABLE repositories ADD COLUMN bm25_total_tokens BIGINT DEFAULT 0;
ALTER TABLE repositories ADD COLUMN bm25_entity_count BIGINT DEFAULT 0;

-- Create index for efficient statistics queries
CREATE INDEX idx_repositories_bm25_stats ON repositories(repository_id, bm25_avgdl) WHERE bm25_avgdl IS NOT NULL;

-- Add token count column to entity_metadata table
ALTER TABLE entity_metadata ADD COLUMN bm25_token_count INTEGER;

-- Create index for deletion performance (fetching old token counts)
CREATE INDEX idx_entity_metadata_token_count ON entity_metadata(repository_id, entity_id, bm25_token_count)
WHERE deleted_at IS NULL;
