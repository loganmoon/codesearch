-- Migration 008: Add sparse embeddings and repository scoping to entity_embeddings
-- Implements repository-aware deduplication for both dense and sparse embeddings

-- Step 1: Add repository_id column (nullable initially for existing data)
ALTER TABLE entity_embeddings
    ADD COLUMN repository_id UUID;

-- Step 2: Add foreign key constraint to repositories table
ALTER TABLE entity_embeddings
    ADD CONSTRAINT fk_entity_embeddings_repository
    FOREIGN KEY (repository_id) REFERENCES repositories(repository_id) ON DELETE CASCADE;

-- Step 3: Add sparse_embedding column (nullable, variable length)
ALTER TABLE entity_embeddings
    ADD COLUMN sparse_embedding REAL[];

-- Step 4: Drop old unique constraint on content_hash alone
ALTER TABLE entity_embeddings
    DROP CONSTRAINT entity_embeddings_content_hash_key;

-- Step 5: Create new unique constraint on (repository_id, content_hash)
ALTER TABLE entity_embeddings
    ADD CONSTRAINT entity_embeddings_repository_content_unique
    UNIQUE (repository_id, content_hash);

-- Step 6: Create index on repository_id for efficient lookups
CREATE INDEX idx_entity_embeddings_repository_id ON entity_embeddings(repository_id);

-- Step 7: Update lookup index to include repository_id
DROP INDEX idx_entity_embeddings_lookup;
CREATE INDEX idx_entity_embeddings_lookup ON entity_embeddings(repository_id, model_version, content_hash);

-- Note: Existing rows will have NULL repository_id and sparse_embedding.
-- Old data can be cleaned up or re-indexed as needed.
