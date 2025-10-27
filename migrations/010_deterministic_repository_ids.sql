-- Migration: Make Repository IDs Deterministic
--
-- Changes repository_id from random UUID v4 to deterministic UUID v5
-- based on the normalized repository path. This ensures:
-- 1. Same repository path always gets the same UUID
-- 2. Entity IDs remain stable across re-indexing
-- 3. Ground truth evaluation labels remain valid

-- Remove the DEFAULT clause from repository_id
-- The application will now compute and provide the repository_id
ALTER TABLE repositories
    ALTER COLUMN repository_id DROP DEFAULT;

-- Note: Existing repositories will keep their current random UUIDs
-- To get deterministic UUIDs, repositories need to be dropped and re-indexed
-- The application code will generate deterministic UUIDs for new repositories
