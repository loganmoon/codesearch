//! BM25 sparse embedding provider

use crate::code_tokenizer::CodeTokenizer;
use crate::sparse_provider::SparseEmbeddingProvider;
use async_trait::async_trait;
use bm25::{Embedder, EmbedderBuilder};
use codesearch_core::error::Result;

/// BM25-based sparse embedding provider
///
/// Uses the bm25 crate with a custom code tokenizer to generate
/// sparse BM25 embeddings for code search.
///
/// # Vocabulary Size Constraint
///
/// The BM25 tokenizer generates unbounded vocabulary (any token can appear),
/// but Qdrant sparse vectors are typically configured with a fixed vocabulary size
/// (default: 100,000). The `bm25` crate uses a hash-based approach to constrain
/// token indices to fit within the configured vocabulary size.
pub struct Bm25SparseProvider {
    embedder: Embedder<u32, CodeTokenizer>,
}

impl Bm25SparseProvider {
    /// Create a new BM25 sparse provider with the specified average document length
    ///
    /// # Arguments
    /// * `avgdl` - Average document length in tokens (used for BM25 length normalization)
    pub fn new(avgdl: f32) -> Self {
        let embedder = EmbedderBuilder::with_avgdl(avgdl)
            .tokenizer(CodeTokenizer::new())
            .build();

        Self { embedder }
    }
}

#[async_trait]
impl SparseEmbeddingProvider for Bm25SparseProvider {
    async fn embed_sparse(&self, texts: Vec<String>) -> Result<Vec<Option<Vec<(u32, f32)>>>> {
        let mut results = Vec::with_capacity(texts.len());

        for text in texts {
            if text.is_empty() {
                results.push(None);
                continue;
            }

            // Generate BM25 embedding
            let embedding = self.embedder.embed(&text);

            // Convert bm25::Embedding to Vec<(u32, f32)>
            let sparse_vec: Vec<(u32, f32)> = embedding
                .iter()
                .map(|token_embedding| (token_embedding.index, token_embedding.value))
                .collect();

            if sparse_vec.is_empty() {
                results.push(None);
            } else {
                results.push(Some(sparse_vec));
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bm25_embedding_non_empty() {
        let provider = Bm25SparseProvider::new(50.0);
        let result = provider
            .embed_sparse(vec!["fn calculate_sum(a: i32, b: i32) -> i32".to_string()])
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].is_some());
        let sparse_vec = result[0].as_ref().unwrap();
        assert!(!sparse_vec.is_empty(), "Sparse vector should not be empty");
    }

    #[tokio::test]
    async fn test_bm25_empty_input() {
        let provider = Bm25SparseProvider::new(50.0);
        let result = provider.embed_sparse(vec!["".to_string()]).await.unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].is_none(), "Empty input should return None");
    }

    #[tokio::test]
    async fn test_bm25_deterministic() {
        let provider = Bm25SparseProvider::new(50.0);
        let text = "fn calculate_sum(a: i32, b: i32) -> i32".to_string();

        let result1 = provider.embed_sparse(vec![text.clone()]).await.unwrap();
        let result2 = provider.embed_sparse(vec![text]).await.unwrap();

        assert_eq!(result1, result2, "BM25 should be deterministic");
    }

    #[tokio::test]
    async fn test_bm25_different_avgdl() {
        let provider1 = Bm25SparseProvider::new(50.0);
        let provider2 = Bm25SparseProvider::new(100.0);
        let text = "fn calculate_sum(a: i32, b: i32) -> i32".to_string();

        let result1 = provider1.embed_sparse(vec![text.clone()]).await.unwrap();
        let result2 = provider2.embed_sparse(vec![text]).await.unwrap();

        // Different avgdl should produce different scores
        assert_ne!(
            result1, result2,
            "Different avgdl values should produce different embeddings"
        );
    }

    #[tokio::test]
    async fn test_bm25_sparse_format() {
        let provider = Bm25SparseProvider::new(50.0);
        let result = provider
            .embed_sparse(vec!["test function".to_string()])
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].is_some());

        let sparse_vec = result[0].as_ref().unwrap();
        // Check format: each element is (index: u32, value: f32)
        for (index, value) in sparse_vec {
            assert!(*value > 0.0, "BM25 values should be positive");
            // Just verify the types are correct
            let _: u32 = *index;
            let _: f32 = *value;
        }
    }
}
