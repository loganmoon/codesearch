//! Unit tests for EmbedAnythingProvider that don't require model downloads

use codesearch_embeddings::{
    create_embed_anything_provider, BackendType, DeviceType, EmbeddingConfigBuilder,
    EmbeddingError, EmbeddingProvider, EmbeddingProviderType,
};

#[tokio::test]
#[ignore] // Model download required
async fn test_invalid_model_path_error() {
    let config = EmbeddingConfigBuilder::new()
        .provider(EmbeddingProviderType::Local)
        .model("nonexistent/model")
        .batch_size(32)
        .device(DeviceType::Cpu)
        .backend(BackendType::Candle)
        .max_workers(4)
        .model_cache_dir("/tmp/nonexistent")
        .build();

    // Should fail when trying to load non-existent model
    let result = create_embed_anything_provider(config).await;
    assert!(result.is_err());

    if let Err(error) = result {
        let error_str = error.to_string();
        assert!(
            error_str.contains("model")
                || error_str.contains("download")
                || error_str.contains("Failed")
        );
    }
}

#[tokio::test]
async fn test_batch_size_validation() {
    use codesearch_embeddings::EmbeddingManager;

    // Create a simple test provider that checks batch size
    struct TestProvider {
        max_batch: usize,
    }

    #[async_trait::async_trait]
    impl EmbeddingProvider for TestProvider {
        async fn embed(
            &self,
            texts: Vec<String>,
        ) -> codesearch_core::error::Result<Vec<Option<Vec<f32>>>> {
            if texts.len() > self.max_batch {
                return Err(EmbeddingError::BatchSizeExceeded {
                    requested: texts.len(),
                    max: self.max_batch,
                }
                .into());
            }
            Ok(texts.iter().map(|_| Some(vec![0.0f32; 768])).collect())
        }

        fn embedding_dimension(&self) -> usize {
            768
        }
        fn max_sequence_length(&self) -> usize {
            512
        }
    }

    let provider = TestProvider { max_batch: 2 };
    let manager = EmbeddingManager::new(std::sync::Arc::new(provider));

    // Should succeed with small batch
    let small_batch = vec!["text1".to_string(), "text2".to_string()];
    assert!(manager.embed(small_batch).await.is_ok());

    // Should fail with large batch
    let large_batch = vec!["t1".to_string(), "t2".to_string(), "t3".to_string()];
    let result = manager.embed(large_batch).await;
    assert!(result.is_err());

    let error = result.unwrap_err();
    assert!(error.to_string().contains("Batch size 3 exceeds maximum 2"));
}

#[tokio::test]
async fn test_embedding_size_limits() {
    use codesearch_core::error::Result;
    use codesearch_embeddings::{EmbeddingManager, EmbeddingProvider};

    struct SizeLimitProvider {
        max_context: usize,
    }

    #[async_trait::async_trait]
    impl EmbeddingProvider for SizeLimitProvider {
        async fn embed(&self, texts: Vec<String>) -> Result<Vec<Option<Vec<f32>>>> {
            Ok(texts
                .iter()
                .map(|text| {
                    if text.len() <= self.max_context {
                        Some(vec![0.0f32; 768])
                    } else {
                        None
                    }
                })
                .collect())
        }

        fn embedding_dimension(&self) -> usize {
            768
        }

        fn max_sequence_length(&self) -> usize {
            self.max_context
        }
    }

    let provider = SizeLimitProvider { max_context: 100 };
    let manager = EmbeddingManager::new(std::sync::Arc::new(provider));

    // Test text under limit returns Some
    let small_text = vec!["Small text".to_string()];
    let result = manager.embed(small_text).await.unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].is_some());
    assert_eq!(result[0].as_ref().unwrap().len(), 768);

    // Test text over limit returns None
    let large_text = vec!["x".repeat(150)];
    let result = manager.embed(large_text).await.unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].is_none());

    // Test batch with mixed sizes
    let mixed_texts = vec![
        "Small".to_string(),
        "x".repeat(150),
        "Another small text".to_string(),
        "y".repeat(200),
    ];
    let result = manager.embed(mixed_texts).await.unwrap();
    assert_eq!(result.len(), 4);
    assert!(result[0].is_some()); // Small
    assert!(result[1].is_none()); // Large
    assert!(result[2].is_some()); // Small
    assert!(result[3].is_none()); // Large
}
