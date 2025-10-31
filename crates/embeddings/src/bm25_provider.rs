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
    async fn embed_sparse(&self, texts: Vec<&str>) -> Result<Vec<Option<Vec<(u32, f32)>>>> {
        use std::collections::HashSet;

        let mut results = Vec::with_capacity(texts.len());

        for text in texts {
            if text.is_empty() {
                results.push(None);
                continue;
            }

            // Generate BM25 embedding
            let embedding = self.embedder.embed(text);

            // Deduplicate indices - the bm25 crate emits one entry per token occurrence,
            // but Qdrant requires unique indices in sparse vectors.
            // Since duplicate indices have identical values, we keep only the first occurrence.
            let mut seen_indices = HashSet::new();
            let mut sparse_vec = Vec::new();

            for token_embedding in embedding.iter() {
                if seen_indices.insert(token_embedding.index) {
                    sparse_vec.push((token_embedding.index, token_embedding.value));
                }
            }

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
            .embed_sparse(vec!["fn calculate_sum(a: i32, b: i32) -> i32"])
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
        let result = provider.embed_sparse(vec![""]).await.unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].is_none(), "Empty input should return None");
    }

    #[tokio::test]
    async fn test_bm25_deterministic() {
        let provider = Bm25SparseProvider::new(50.0);
        let text = "fn calculate_sum(a: i32, b: i32) -> i32";

        let result1 = provider.embed_sparse(vec![text]).await.unwrap();
        let result2 = provider.embed_sparse(vec![text]).await.unwrap();

        assert_eq!(result1, result2, "BM25 should be deterministic");
    }

    #[tokio::test]
    async fn test_bm25_different_avgdl() {
        let provider1 = Bm25SparseProvider::new(50.0);
        let provider2 = Bm25SparseProvider::new(100.0);
        let text = "fn calculate_sum(a: i32, b: i32) -> i32";

        let result1 = provider1.embed_sparse(vec![text]).await.unwrap();
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
        let result = provider.embed_sparse(vec!["test function"]).await.unwrap();

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

    #[tokio::test]
    async fn test_bm25_unique_indices() {
        use std::collections::HashSet;

        let provider = Bm25SparseProvider::new(50.0);
        let result = provider
            .embed_sparse(vec!["fn calculate_sum(a: i32, b: i32) -> i32"])
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].is_some());

        let sparse_vec = result[0].as_ref().unwrap();

        // Verify all indices are unique
        let indices: Vec<u32> = sparse_vec.iter().map(|(idx, _)| *idx).collect();
        let unique_indices: HashSet<u32> = indices.iter().copied().collect();

        assert_eq!(
            indices.len(),
            unique_indices.len(),
            "All indices should be unique after deduplication"
        );

        // Verify we have 6 unique tokens (fn, calculate, sum, a, i32, b)
        // Note: "i32" appears 3 times in input but should only appear once in output
        assert_eq!(sparse_vec.len(), 6, "Should have 6 unique indices");
    }

    #[tokio::test]
    async fn test_token_count_consistency() {
        use crate::code_tokenizer::CodeTokenizer;
        use bm25::Tokenizer;

        let tokenizer = CodeTokenizer::new();
        let text = "fn calculate_sum(a: i32, b: i32) -> i32";

        // Count tokens using the tokenizer directly
        let tokens = tokenizer.tokenize(text);
        let token_count = tokens.len();

        // Generate BM25 embedding
        let provider = Bm25SparseProvider::new(50.0);
        let result = provider.embed_sparse(vec![text]).await.unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].is_some());

        let sparse_vec = result[0].as_ref().unwrap();
        let unique_token_count = sparse_vec.len();

        // Token count from tokenizer should equal the number of unique indices in sparse embedding
        // This verifies that bm25_token_count stored in DB would match tokenizer output
        assert_eq!(
            token_count, 8,
            "Tokenizer should produce 8 tokens (with duplicates)"
        );
        assert_eq!(
            unique_token_count, 6,
            "BM25 sparse embedding should have 6 unique indices (after deduplication)"
        );
    }

    #[tokio::test]
    async fn test_token_count_with_special_characters() {
        use crate::code_tokenizer::CodeTokenizer;
        use bm25::Tokenizer;

        let tokenizer = CodeTokenizer::new();
        let text = "parse_HTTPRequest!@#$%getUserName";

        let tokens = tokenizer.tokenize(text);
        // Should tokenize to: ["parse", "http", "request", "get", "user", "name"]
        assert_eq!(
            tokens.len(),
            6,
            "Should handle special characters correctly"
        );

        // Verify BM25 embedding generates same number of unique tokens
        let provider = Bm25SparseProvider::new(50.0);
        let result = provider.embed_sparse(vec![text]).await.unwrap();
        let sparse_vec = result[0].as_ref().unwrap();
        assert_eq!(
            sparse_vec.len(),
            6,
            "BM25 should generate 6 unique indices matching tokenizer"
        );
    }

    #[tokio::test]
    async fn test_token_count_with_unicode() {
        use crate::code_tokenizer::CodeTokenizer;
        use bm25::Tokenizer;

        let tokenizer = CodeTokenizer::new();
        // Unicode characters should be handled properly
        let text = "fn calculateSum(données: i32) → Result";

        let tokens = tokenizer.tokenize(text);
        let token_count = tokens.len();

        let provider = Bm25SparseProvider::new(50.0);
        let result = provider.embed_sparse(vec![text]).await.unwrap();
        let sparse_vec = result[0].as_ref().unwrap();

        // Verify token count consistency with Unicode input
        assert!(token_count > 0, "Should tokenize Unicode text");
        assert_eq!(
            sparse_vec.len(),
            tokens
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            "Unique BM25 indices should match unique tokens from tokenizer"
        );
    }
}
