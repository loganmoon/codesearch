-- Migration 009: Fix sparse embedding schema to prevent u32 → f32 precision loss
--
-- Problem: The previous schema stored sparse embeddings as REAL[] with flattened
-- (index, value) pairs, converting u32 indices to f32. This causes precision loss
-- for indices > 16,777,216 (f32's 24-bit mantissa limit).
--
-- Solution: Replace single REAL[] column with two separate columns matching
-- Qdrant's native sparse vector format:
-- - sparse_indices BIGINT[] (stores u32 indices, BIGINT can safely hold all u32 values)
-- - sparse_values REAL[] (stores f32 weights)

-- Step 1: Drop the old sparse_embedding column
ALTER TABLE entity_embeddings
    DROP COLUMN sparse_embedding;

-- Step 2: Add separate columns for indices and values
ALTER TABLE entity_embeddings
    ADD COLUMN sparse_indices BIGINT[],
    ADD COLUMN sparse_values REAL[];

-- Note: Existing sparse embedding data is lost, but this is acceptable since
-- the old data was already corrupted for large vocabulary indices.
