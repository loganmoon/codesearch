//! Integration tests for EmbedAnythingProvider with real models
//!
//! These tests require downloading the actual models from Hugging Face
//! and are marked with #[ignore] by default.
//!
//! To run these tests:
//! ```bash
//! cargo test --test integration_tests -- --ignored
//! ```

use codesearch_embeddings::{
    create_embed_anything_provider, BackendType, DeviceType, EmbeddingConfigBuilder,
    EmbeddingProviderType,
};

#[tokio::test]
#[ignore] // Run with --ignored flag to test with actual model
async fn test_embed_anything_provider_real_model() {
    let config = EmbeddingConfigBuilder::new()
        .provider(EmbeddingProviderType::Local)
        .model("nomic-ai/modernbert-embed-base")
        .batch_size(32)
        .device(DeviceType::Cpu)
        .backend(BackendType::Candle)
        .max_workers(4)
        .model_cache_dir("./test_models")
        .build();

    let embeddings = create_embed_anything_provider(config).await.unwrap();

    // Test with real code samples
    let code_samples = vec![
        "def hello_world():\n    print('Hello, World!')".to_string(),
        "function fibonacci(n) { return n <= 1 ? n : fibonacci(n-1) + fibonacci(n-2); }"
            .to_string(),
    ];

    let results = embeddings.embed(code_samples).await.unwrap();

    assert_eq!(results.len(), 2);
    // Dynamic dimensions - just check they're consistent
    let dimensions = embeddings.embedding_dimension();

    // Unwrap the Option values - these should all be Some since text is small
    let embed1 = results[0].as_ref().unwrap();
    let embed2 = results[1].as_ref().unwrap();

    assert_eq!(embed1.len(), dimensions);
    assert_eq!(embed2.len(), dimensions);

    // Check that embeddings are different for different code
    let similarity = cosine_similarity(embed1, embed2);
    assert!(similarity < 0.99); // Should not be identical
    assert!(similarity > 0.0); // Should have some similarity (both are code)
}

#[tokio::test]
#[ignore]
async fn test_batch_processing() {
    let config = EmbeddingConfigBuilder::new()
        .provider(EmbeddingProviderType::Local)
        .model("nomic-ai/modernbert-embed-base")
        .batch_size(2) // Small batch size for testing
        .device(DeviceType::Cpu)
        .backend(BackendType::Candle)
        .max_workers(2)
        .model_cache_dir("./test_models")
        .build();

    let embeddings = create_embed_anything_provider(config).await.unwrap();

    // Test batch processing with larger input that will be chunked internally
    let texts = vec![
        "const x = 1;".to_string(),
        "let y = 2;".to_string(),
        "function test() {}".to_string(),
    ];

    let results = embeddings.embed(texts).await.unwrap();

    assert_eq!(results.len(), 3);

    // All embeddings should have correct dimension
    let dimensions = embeddings.embedding_dimension();
    for embedding_option in &results {
        let embedding = embedding_option.as_ref().unwrap();
        assert_eq!(embedding.len(), dimensions);
    }
}

#[tokio::test]
#[ignore]
async fn test_long_text_handling() {
    let config = EmbeddingConfigBuilder::new()
        .model("nomic-ai/modernbert-embed-base")
        .batch_size(32)
        .model_cache_dir("./test_models")
        .build();
    let embeddings = create_embed_anything_provider(config).await.unwrap();

    // Create a very long code sample
    let long_code = "x = 1\n".repeat(1000); // Very long text

    let result = embeddings.embed(vec![long_code]).await;

    // Should handle long text gracefully (truncation or error)
    assert!(
        result.is_ok()
            || matches!(
                result,
                Err(e) if e.to_string().contains("sequence")
            )
    );
}

#[tokio::test]
#[ignore]
async fn test_embedding_consistency() {
    let config = EmbeddingConfigBuilder::new()
        .model("nomic-ai/modernbert-embed-base")
        .batch_size(32)
        .model_cache_dir("./test_models")
        .build();
    let embeddings = create_embed_anything_provider(config).await.unwrap();

    let code = "def add(a, b): return a + b".to_string();

    // Generate embeddings twice for the same code
    let result1 = embeddings.embed(vec![code.clone()]).await.unwrap();
    let result2 = embeddings.embed(vec![code]).await.unwrap();

    // Should produce identical embeddings for identical input
    let embed1 = result1[0].as_ref().unwrap();
    let embed2 = result2[0].as_ref().unwrap();
    let similarity = cosine_similarity(embed1, embed2);
    assert!(
        similarity > 0.9999,
        "Embeddings should be nearly identical for same input"
    );
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}

#[tokio::test]
#[ignore] // Run with --ignored flag to test with actual Jina model
async fn test_jina_v3_embeddings() {
    let config = EmbeddingConfigBuilder::new()
        .provider(EmbeddingProviderType::Local)
        .model("jinaai/jina-embeddings-v3")
        .batch_size(32)
        .device(DeviceType::Cpu)
        .backend(BackendType::Candle)
        .max_workers(4)
        .model_cache_dir("./test_models")
        .build();

    let embeddings = create_embed_anything_provider(config).await.unwrap();

    // Test with real code samples
    let code_samples = vec![
        "def hello_world():\n    print('Hello, World!')".to_string(),
        "function fibonacci(n) { return n <= 1 ? n : fibonacci(n-1) + fibonacci(n-2); }"
            .to_string(),
    ];

    let results = embeddings.embed(code_samples).await.unwrap();

    assert_eq!(results.len(), 2);
    // Jina v3 has 1024 dimensions
    let dimensions = embeddings.embedding_dimension();
    assert_eq!(dimensions, 1024);

    // Unwrap the Option values - these should all be Some since text is small
    let embed1 = results[0].as_ref().unwrap();
    let embed2 = results[1].as_ref().unwrap();

    assert_eq!(embed1.len(), 1024);
    assert_eq!(embed2.len(), 1024);

    // Check that embeddings are different for different code
    let similarity = cosine_similarity(embed1, embed2);
    assert!(similarity < 0.99); // Should not be identical
    assert!(similarity > 0.0); // Should have some similarity (both are code)
}

#[tokio::test]
#[ignore] // Run with --ignored flag to test with actual models
async fn test_model_switching() {
    // First test with ModernBert model
    let config_modernbert = EmbeddingConfigBuilder::new()
        .provider(EmbeddingProviderType::Local)
        .model("nomic-ai/modernbert-embed-base")
        .batch_size(32)
        .device(DeviceType::Cpu)
        .backend(BackendType::Candle)
        .max_workers(4)
        .model_cache_dir("./test_models")
        .build();

    let embeddings_modernbert = create_embed_anything_provider(config_modernbert)
        .await
        .unwrap();
    let modernbert_dim = embeddings_modernbert.embedding_dimension();

    // Now test with Jina model
    let config_jina = EmbeddingConfigBuilder::new()
        .provider(EmbeddingProviderType::Local)
        .model("jinaai/jina-embeddings-v3")
        .batch_size(32)
        .device(DeviceType::Cpu)
        .backend(BackendType::Candle)
        .max_workers(4)
        .model_cache_dir("./test_models")
        .build();

    let embeddings_jina = create_embed_anything_provider(config_jina).await.unwrap();
    let jina_dim = embeddings_jina.embedding_dimension();

    // Verify dimensions are as expected
    assert_eq!(jina_dim, 1024); // Jina v3 has fixed 1024 dimensions
                                // ModernBert dimensions vary by model, just check it's not 1024
    assert_ne!(modernbert_dim, jina_dim);

    // Test both can generate embeddings
    let test_code = vec!["print('test')".to_string()];

    let modernbert_result = embeddings_modernbert
        .embed(test_code.clone())
        .await
        .unwrap();
    let jina_result = embeddings_jina.embed(test_code).await.unwrap();

    assert_eq!(modernbert_result[0].as_ref().unwrap().len(), modernbert_dim);
    assert_eq!(jina_result[0].as_ref().unwrap().len(), jina_dim);
}
