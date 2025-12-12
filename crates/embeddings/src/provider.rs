//! Trait definition for embedding providers

use async_trait::async_trait;
use codesearch_core::error::Result;
use std::fmt;
use std::path::PathBuf;

/// Context information about an entity being embedded (for error logging)
#[derive(Clone, Debug)]
pub struct EmbeddingContext {
    pub qualified_name: String,
    pub file_path: PathBuf,
    pub line_number: u32,
    pub entity_type: String,
}

impl fmt::Display for EmbeddingContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Entity: {} | File: {}:{} | Type: {}",
            self.qualified_name,
            self.file_path.display(),
            self.line_number,
            self.entity_type
        )
    }
}

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
    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Option<Vec<f32>>>> {
        self.embed_with_context(texts, None).await
    }

    /// Generate embeddings for a list of texts with optional context for error logging
    ///
    /// # Arguments
    /// * `texts` - List of text strings to embed
    /// * `contexts` - Optional entity contexts for error logging (must match texts length if provided)
    async fn embed_with_context(
        &self,
        texts: Vec<String>,
        contexts: Option<Vec<EmbeddingContext>>,
    ) -> Result<Vec<Option<Vec<f32>>>>;

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
