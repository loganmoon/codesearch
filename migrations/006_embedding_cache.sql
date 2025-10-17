-- Migration: Embedding cache for content-based deduplication
-- Stores embeddings keyed by content hash (XxHash3_128) to enable cache reuse
-- across indexing runs and repositories with identical code

CREATE TABLE embedding_cache (
    -- XxHash3_128 hash of extract_embedding_content() output (32 hex chars)
    content_hash TEXT PRIMARY KEY,

    -- Embedding vector as float array (768 dimensions for BAAI/bge-code-v1)
    -- Note: Using FLOAT[] instead of pgvector VECTOR type for simplicity
    -- Can migrate to pgvector later if needed for performance
    embedding FLOAT[] NOT NULL,

    -- Model version identifier (e.g., "BAAI/bge-code-v1")
    -- Enables cache invalidation when model changes
    model_version TEXT NOT NULL,

    -- Vector dimension (768 for current model)
    -- Validation check to prevent dimension mismatches
    dimension INTEGER NOT NULL,

    -- Timestamps for TTL cleanup
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_accessed_at TIMESTAMPTZ,

    -- Access counter for potential future LRU eviction
    access_count BIGINT DEFAULT 1,

    -- Validate embedding array dimension
    CONSTRAINT check_embedding_dimension CHECK (array_length(embedding, 1) = dimension)
);

-- Index on model_version for cache invalidation queries
CREATE INDEX idx_embedding_cache_model ON embedding_cache(model_version);

-- Index on created_at for TTL-based cleanup (90 day retention)
CREATE INDEX idx_embedding_cache_created ON embedding_cache(created_at);

-- Partial index on last_accessed_at for potential LRU eviction
-- (Only index rows that have been accessed, saves space)
CREATE INDEX idx_embedding_cache_lru ON embedding_cache(last_accessed_at)
    WHERE last_accessed_at IS NOT NULL;
