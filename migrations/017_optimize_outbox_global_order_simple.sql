-- Migration: Add optimized index for global ordering by created_at
-- This allows efficient fetching of the oldest outbox entries regardless of target_store
-- The previous index on (target_store, created_at) was inefficient for the global ORDER BY created_at query

CREATE INDEX IF NOT EXISTS idx_entity_outbox_created_at_unprocessed
    ON entity_outbox(created_at)
    WHERE processed_at IS NULL;
