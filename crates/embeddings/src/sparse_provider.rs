//! Trait definition for sparse embedding providers

use async_trait::async_trait;
use codesearch_core::error::Result;

/// Trait for sparse embedding providers
///
/// This trait defines the interface for providers that generate sparse embeddings
/// (e.g., BM25) as opposed to dense embeddings. Sparse embeddings are represented
/// as lists of (index, weight) pairs where only non-zero elements are stored.
#[async_trait]
pub trait SparseEmbeddingProvider: Send + Sync {
    /// Generate sparse embeddings for a list of texts
    ///
    /// # Arguments
    /// * `texts` - List of text string references to embed
    ///
    /// # Returns
    /// A vector of Option sparse embedding vectors, one for each input text.
    /// Each sparse embedding is represented as Vec<(u32, f32)> where:
    /// - u32 is the feature index
    /// - f32 is the weight/value
    /// Returns None for texts that cannot be processed.
    async fn embed_sparse(&self, texts: Vec<&str>) -> Result<Vec<Option<Vec<(u32, f32)>>>>;
}
