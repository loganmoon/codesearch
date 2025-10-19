-- Migration: Entity embeddings - single source of truth for code entity embeddings
-- Stores embeddings with content-based deduplication via XxHash3_128 hashing
-- Referenced by entity_metadata and entity_outbox via foreign keys

CREATE TABLE entity_embeddings (
    -- Primary key for foreign key relationships
    id BIGSERIAL PRIMARY KEY,

    -- XxHash3_128 hash of extract_embedding_content() output (32 hex chars)
    -- Used for content-based deduplication across repositories
    content_hash CHAR(32) NOT NULL UNIQUE,

    -- Embedding vector as float array (1536 dimensions for BAAI/bge-code-v1)
    -- Note: Using REAL[] (float4) instead of pgvector VECTOR type for simplicity
    -- REAL[] matches Rust f32, FLOAT[] would be f64
    embedding REAL[] NOT NULL,

    -- Model version identifier (e.g., "BAAI/bge-code-v1")
    -- Enables cache invalidation when model changes
    model_version TEXT NOT NULL,

    -- Vector dimension (1536 for current model)
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

-- Composite index for content hash lookups by model version
-- Enables efficient lookups for the common access pattern: WHERE model_version = ? AND content_hash IN (...)
CREATE INDEX idx_entity_embeddings_lookup ON entity_embeddings(model_version, content_hash);

-- Index on created_at for TTL-based cleanup (90 day retention)
CREATE INDEX idx_entity_embeddings_created ON entity_embeddings(created_at);

-- Partial index on last_accessed_at for potential LRU eviction
-- (Only index rows that have been accessed, saves space)
CREATE INDEX idx_entity_embeddings_lru ON entity_embeddings(last_accessed_at)
    WHERE last_accessed_at IS NOT NULL;

-- Add foreign key from entity_metadata to entity_embeddings
-- This makes entity_embeddings the source of truth for embeddings
ALTER TABLE entity_metadata
    ADD COLUMN embedding_id BIGINT REFERENCES entity_embeddings(id) ON DELETE SET NULL;

-- Index for efficient joins and lookups
CREATE INDEX idx_entity_metadata_embedding_id ON entity_metadata(embedding_id);

-- Add foreign key from entity_outbox to entity_embeddings
-- Outbox processor will fetch embedding by ID instead of from payload
ALTER TABLE entity_outbox
    ADD COLUMN embedding_id BIGINT REFERENCES entity_embeddings(id) ON DELETE SET NULL;

-- Index for efficient outbox processing
CREATE INDEX idx_entity_outbox_embedding_id ON entity_outbox(embedding_id);
