//! Pipeline batch data structures for multi-stage indexing

use codesearch_core::CodeEntity;
use std::collections::HashMap;
use std::path::PathBuf;

/// Batch of discovered file paths
pub(crate) struct FileBatch {
    pub paths: Vec<PathBuf>,
}

/// Batch of extracted entities with file tracking
pub(crate) struct EntityBatch {
    pub entities: Vec<CodeEntity>,
    /// Track which files produced which entities for snapshot updates
    /// (file_path, entity_indices_in_batch)
    pub file_indices: Vec<(PathBuf, Vec<usize>)>,
    pub repo_id: uuid::Uuid,
    pub git_commit: Option<String>,
    pub collection_name: String,
}

/// Triple of (entity, embedding_id, sparse_embedding) for entities that have been embedded
pub(crate) type EntityEmbeddingTriple = (CodeEntity, i64, Vec<(u32, f32)>);

/// Batch of entities with their embeddings
pub(crate) struct EmbeddedBatch {
    /// Entities paired with embedding IDs and sparse embeddings (skipped entities filtered out)
    pub entity_embedding_id_sparse_triples: Vec<EntityEmbeddingTriple>,
    pub file_indices: Vec<(PathBuf, Vec<usize>)>,
    pub repo_id: uuid::Uuid,
    pub git_commit: Option<String>,
    pub collection_name: String,
}

/// Batch of stored entities ready for snapshot updates
pub(crate) struct StoredBatch {
    /// Metadata for snapshot updates (entities already stored)
    pub file_entity_map: HashMap<PathBuf, Vec<String>>,
    pub repo_id: uuid::Uuid,
    pub collection_name: String,
    pub git_commit: Option<String>,
}
