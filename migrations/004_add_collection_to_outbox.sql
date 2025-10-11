-- Migration: Add collection_name to entity_outbox for multi-repository support
-- Enables centralized outbox processor to route entries to correct Qdrant collection

-- 1. Add collection_name column (nullable initially for backfill)
ALTER TABLE entity_outbox
ADD COLUMN collection_name VARCHAR(255);

-- 2. Backfill existing entries with collection_name from repositories table
UPDATE entity_outbox
SET collection_name = r.collection_name
FROM repositories r
WHERE entity_outbox.repository_id = r.repository_id;

-- 3. Make column NOT NULL now that backfill is complete
ALTER TABLE entity_outbox
ALTER COLUMN collection_name SET NOT NULL;

-- 4. Create composite index for efficient unprocessed outbox queries
-- Replaces idx_entity_outbox_unprocessed with collection_name included
DROP INDEX IF EXISTS idx_entity_outbox_unprocessed;

CREATE INDEX idx_entity_outbox_unprocessed_by_collection
    ON entity_outbox(target_store, collection_name, created_at)
    WHERE processed_at IS NULL;

-- 5. Verify data integrity
DO $$
DECLARE
    null_count INTEGER;
BEGIN
    SELECT COUNT(*) INTO null_count
    FROM entity_outbox
    WHERE collection_name IS NULL;

    IF null_count > 0 THEN
        RAISE EXCEPTION 'Found % rows with NULL collection_name after migration', null_count;
    END IF;
END $$;
