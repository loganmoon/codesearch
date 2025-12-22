-- Migration: Add path_entity_identifier column for import resolution lookups
-- This column stores the file-path-based identifier for entities,
-- used for resolving imports and cross-file references.

-- 1. Add the column (nullable for backward compatibility with existing data)
ALTER TABLE entity_metadata
ADD COLUMN IF NOT EXISTS path_entity_identifier TEXT;

-- 2. Create an index for lookups (important for import resolution)
CREATE INDEX IF NOT EXISTS idx_entity_metadata_path_entity_identifier
    ON entity_metadata(repository_id, path_entity_identifier)
    WHERE deleted_at IS NULL AND path_entity_identifier IS NOT NULL;

-- 3. Comment explaining the column
COMMENT ON COLUMN entity_metadata.path_entity_identifier IS
    'File-path-based identifier for import resolution (e.g., "src.utils.helpers.formatNumber")';
