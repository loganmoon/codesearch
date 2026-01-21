//! Types and helpers for PostgreSQL storage

use codesearch_core::entities::CodeEntity;
use codesearch_core::error::{Error, Result};
use serde::Serialize;
use std::str::FromStr;
use uuid::Uuid;

/// Maximum sparse embedding size to prevent memory exhaustion attacks
pub(crate) const MAX_SPARSE_EMBEDDING_SIZE: usize = 100_000;

/// Convert sparse embedding to separate indices and values arrays for PostgreSQL storage
/// PostgreSQL BIGINT[] can safely store all u32 values (0 to 4,294,967,295)
/// Returns an error if the sparse embedding exceeds MAX_SPARSE_EMBEDDING_SIZE
pub(crate) fn sparse_embedding_to_arrays(sparse: &[(u32, f32)]) -> Result<(Vec<i64>, Vec<f32>)> {
    if sparse.len() > MAX_SPARSE_EMBEDDING_SIZE {
        return Err(Error::storage(format!(
            "Sparse embedding size {} exceeds maximum allowed size {MAX_SPARSE_EMBEDDING_SIZE}",
            sparse.len()
        )));
    }

    let (indices, values): (Vec<u32>, Vec<f32>) = sparse.iter().copied().unzip();
    let indices_i64: Vec<i64> = indices.into_iter().map(|idx| idx as i64).collect();
    Ok((indices_i64, values))
}

/// Convert separate indices and values arrays from PostgreSQL back to sparse embedding
/// Returns an error if the arrays have mismatched lengths or exceed MAX_SPARSE_EMBEDDING_SIZE
pub(crate) fn arrays_to_sparse_embedding(
    indices: Vec<i64>,
    values: Vec<f32>,
) -> Result<Vec<(u32, f32)>> {
    if indices.len() != values.len() {
        return Err(Error::storage(format!(
            "Sparse embedding indices length {} does not match values length {}",
            indices.len(),
            values.len()
        )));
    }

    if indices.len() > MAX_SPARSE_EMBEDDING_SIZE {
        return Err(Error::storage(format!(
            "Sparse embedding size {} exceeds maximum allowed size {MAX_SPARSE_EMBEDDING_SIZE}",
            indices.len()
        )));
    }

    Ok(indices
        .into_iter()
        .zip(values)
        .map(|(idx, val)| (idx as u32, val))
        .collect())
}

/// Type alias for embedding cache entry: (content_hash, dense_embedding, sparse_embedding)
pub type EmbeddingCacheEntry = (String, Vec<f32>, Option<Vec<(u32, f32)>>);

/// Type alias for sparse embedding database row: (dense, sparse_indices, sparse_values)
pub(crate) type SparseEmbeddingRow = (Vec<f32>, Option<Vec<i64>>, Option<Vec<f32>>);

/// Type alias for validated sparse embedding arrays: (sparse_indices, sparse_values)
pub(crate) type ValidatedSparseArrays = (Option<Vec<i64>>, Option<Vec<f32>>);

/// Type alias for batch embedding row: (id, dense_embedding, sparse_indices, sparse_values)
pub(crate) type BatchEmbeddingRow = (i64, Vec<f32>, Option<Vec<i64>>, Option<Vec<f32>>);

/// Operation type for outbox pattern
///
/// Represents the type of operation to be performed on the target data store.
/// Used in the transactional outbox pattern to ensure eventual consistency
/// between PostgreSQL metadata and external stores like Qdrant.
#[derive(Debug, Clone, Copy)]
pub enum OutboxOperation {
    /// Insert a new entity into the target store
    Insert,
    /// Update an existing entity in the target store
    Update,
    /// Delete an entity from the target store
    Delete,
}

impl std::fmt::Display for OutboxOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Insert => write!(f, "INSERT"),
            Self::Update => write!(f, "UPDATE"),
            Self::Delete => write!(f, "DELETE"),
        }
    }
}

impl FromStr for OutboxOperation {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "INSERT" => Ok(Self::Insert),
            "UPDATE" => Ok(Self::Update),
            "DELETE" => Ok(Self::Delete),
            _ => Err(Error::storage(format!("Invalid operation: {s}"))),
        }
    }
}

/// Target data store for outbox pattern
///
/// Identifies which external data store should process the outbox entry.
/// Each target store has its own processing queue to enable parallel processing
/// and independent scaling of different storage backends.
#[derive(Debug, Clone, Copy)]
pub enum TargetStore {
    /// Qdrant vector database for semantic search
    Qdrant,
    /// Neo4j graph database for relationship queries
    Neo4j,
}

impl std::fmt::Display for TargetStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Qdrant => write!(f, "qdrant"),
            Self::Neo4j => write!(f, "neo4j"),
        }
    }
}

impl FromStr for TargetStore {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "qdrant" => Ok(Self::Qdrant),
            "neo4j" => Ok(Self::Neo4j),
            _ => Err(Error::storage(format!("Invalid target store: {s}"))),
        }
    }
}

/// Outbox entry for reliable event publishing
///
/// Represents a pending operation that needs to be applied to an external data store.
/// The outbox pattern ensures that database changes and external store updates happen
/// atomically by writing both to PostgreSQL in a transaction, then processing outbox
/// entries asynchronously to update external stores.
///
/// # Fields
///
/// * `outbox_id` - Unique identifier for this outbox entry
/// * `repository_id` - Repository this operation applies to
/// * `entity_id` - Identifier of the entity to be modified
/// * `operation` - Operation type (INSERT, UPDATE, DELETE)
/// * `target_store` - Which external store should process this (qdrant, neo4j)
/// * `payload` - JSON payload containing the data needed to perform the operation
/// * `created_at` - When this entry was created
/// * `processed_at` - When this entry was successfully processed (None if pending)
/// * `retry_count` - Number of times processing has been attempted
/// * `last_error` - Error message from the most recent failed processing attempt
/// * `collection_name` - Target collection name in the external store (e.g., Qdrant collection)
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct OutboxEntry {
    pub outbox_id: Uuid,
    pub repository_id: Uuid,
    pub entity_id: String,
    pub operation: String,
    pub target_store: String,
    pub payload: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub processed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub retry_count: i32,
    pub last_error: Option<String>,
    pub collection_name: String,
    pub embedding_id: Option<i64>,
}

/// Type alias for a single entity batch entry with outbox data
pub type EntityOutboxBatchEntry<'a> = (
    &'a CodeEntity,
    i64, // embedding_id (now includes both dense and sparse in entity_embeddings table)
    OutboxOperation,
    Uuid, // qdrant_point_id
    TargetStore,
    Option<String>, // git_commit_hash
    usize,          // bm25_token_count
);

/// Neo4j node properties for outbox payload
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Neo4jNodeProperties {
    pub id: String,
    pub repository_id: String,
    pub qualified_name: String,
    pub name: String,
    pub language: String,
    pub visibility: String,
    pub is_async: bool,
    pub is_generic: bool,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_const: bool,
}

/// Complete Neo4j outbox payload
#[derive(Debug, Serialize)]
pub(crate) struct Neo4jOutboxPayload<'a> {
    pub entity: &'a CodeEntity,
    pub node: Neo4jNodeProperties,
    pub labels: Vec<&'static str>,
    pub relationships: Vec<serde_json::Value>,
}
