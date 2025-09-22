//! Trait definition for embedding providers

use async_trait::async_trait;
use codesearch_core::error::Result;

/// Trait for embedding providers
///
/// This trait defines the interface that all embedding providers must implement,
/// whether they are local (Candle-based) or remote (API-based).
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate embeddings for a list of texts
    ///
    /// # Arguments
    /// * `texts` - List of text strings to embed
    ///
    /// # Returns
    /// A vector of Option embedding vectors (f32 for Qdrant compatibility), one for each input text.
    /// Returns None for texts that exceed the model's context window.
    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Option<Vec<f32>>>>;

    /// Get the embedding dimension
    ///
    /// # Returns
    /// The size of the embedding vectors produced by this provider
    fn embedding_dimension(&self) -> usize;

    /// Get the maximum sequence length supported
    ///
    /// # Returns
    /// The maximum number of tokens that can be processed in a single text
    fn max_sequence_length(&self) -> usize;
}
