-- Migration: Optimize outbox index for global ordering
-- Replace per-collection index with global ordering index to support
-- single-query batch processing across all collections

-- Drop old per-collection index
DROP INDEX IF EXISTS idx_entity_outbox_unprocessed_by_collection;

-- Create new global ordering index
CREATE INDEX idx_entity_outbox_unprocessed_global
    ON entity_outbox(target_store, created_at)
    WHERE processed_at IS NULL;
