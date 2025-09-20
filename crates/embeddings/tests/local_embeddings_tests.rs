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
        async fn embed(&self, texts: Vec<String>) -> codesearch_core::error::Result<Vec<Vec<f32>>> {
            if texts.len() > self.max_batch {
                return Err(EmbeddingError::BatchSizeExceeded {
                    requested: texts.len(),
                    max: self.max_batch,
                }
                .into());
            }
            Ok(texts.iter().map(|_| vec![0.0f32; 768]).collect())
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
