//! Mock embedding provider for testing

use crate::provider::EmbeddingProvider;
use async_trait::async_trait;
use codesearch_core::error::Result;

/// Mock embedding provider that returns dummy embeddings
pub struct MockEmbeddingProvider {
    embedding_dim: usize,
}

impl MockEmbeddingProvider {
    /// Create a new mock provider with specified embedding dimension
    pub fn new(embedding_dim: usize) -> Self {
        Self { embedding_dim }
    }
}

#[async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Option<Vec<f32>>>> {
        // Return mock embeddings - just zeros for simplicity
        Ok(texts
            .into_iter()
            .map(|_| Some(vec![0.0; self.embedding_dim]))
            .collect())
    }

    fn embedding_dimension(&self) -> usize {
        self.embedding_dim
    }

    fn max_sequence_length(&self) -> usize {
        512 // Mock value
    }
}
