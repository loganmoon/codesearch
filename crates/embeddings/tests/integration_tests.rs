//! Integration tests for embedding providers
//!
//! These tests require a running vLLM instance at http://localhost:8000
//! Start with: docker compose up -d vllm-embeddings
//! Run with: cargo test --package codesearch-embeddings --test integration_tests -- --ignored

use codesearch_embeddings::{
    create_api_provider, EmbeddingConfigBuilder, EmbeddingProviderType,
};

#[tokio::test]
#[ignore] // Requires vLLM running locally
async fn test_vllm_api_provider_basic() {
    let config = EmbeddingConfigBuilder::new()
        .provider(EmbeddingProviderType::LocalApi)
        .model("BAAI/bge-code-v1")
        .api_base_url("http://localhost:8000/v1")
        .embedding_dimension(1536)
        .batch_size(32)
        .max_workers(4)
        .build();

    let provider = create_api_provider(config)
        .await
        .expect("Failed to create API provider");

    // Test with real code samples
    let code_samples = vec![
        "def hello_world():\n    print('Hello, World!')".to_string(),
        "fn main() { println!(\"Hello, World!\"); }".to_string(),
    ];

    let results = provider.embed(code_samples).await.unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(provider.embedding_dimension(), 1536);

    // Check embeddings are valid
    let embed1 = results[0].as_ref().expect("First embedding should succeed");
    let embed2 = results[1]
        .as_ref()
        .expect("Second embedding should succeed");

    assert_eq!(embed1.len(), 1536);
    assert_eq!(embed2.len(), 1536);

    // Check embeddings are different
    let similarity = cosine_similarity(embed1, embed2);
    assert!(
        similarity < 0.99,
        "Different code should have different embeddings"
    );
    assert!(similarity > 0.0, "Code samples should have some similarity");
}

#[tokio::test]
#[ignore]
async fn test_api_provider_batch_processing() {
    let config = EmbeddingConfigBuilder::new()
        .provider(EmbeddingProviderType::LocalApi)
        .model("BAAI/bge-code-v1")
        .api_base_url("http://localhost:8000/v1")
        .embedding_dimension(1536)
        .batch_size(2) // Small batch for testing
        .max_workers(2)
        .build();

    let provider = create_api_provider(config).await.unwrap();

    // Test with multiple samples that will be batched
    let texts = vec![
        "const x = 1;".to_string(),
        "let y = 2;".to_string(),
        "var z = 3;".to_string(),
        "int w = 4;".to_string(),
    ];

    let results = provider.embed(texts).await.unwrap();

    assert_eq!(results.len(), 4);

    for (i, result) in results.iter().enumerate() {
        assert!(result.is_some(), "Embedding {i} should succeed");
        assert_eq!(
            result.as_ref().unwrap().len(),
            1536,
            "Embedding {i} should have correct dimension"
        );
    }
}

#[tokio::test]
#[ignore]
async fn test_api_provider_long_text() {
    let config = EmbeddingConfigBuilder::new()
        .provider(EmbeddingProviderType::LocalApi)
        .model("BAAI/bge-code-v1")
        .api_base_url("http://localhost:8000/v1")
        .embedding_dimension(1536)
        .batch_size(32)
        .build();

    let provider = create_api_provider(config).await.unwrap();

    // Create a very long code sample (exceeds context window)
    let long_code = "x = 1\n".repeat(10000);

    let results = provider.embed(vec![long_code]).await.unwrap();

    assert_eq!(results.len(), 1);
    // Should return None for text exceeding context window
    assert!(results[0].is_none(), "Long text should return None");
}

#[tokio::test]
#[ignore]
async fn test_api_provider_consistency() {
    let config = EmbeddingConfigBuilder::new()
        .provider(EmbeddingProviderType::LocalApi)
        .model("BAAI/bge-code-v1")
        .api_base_url("http://localhost:8000/v1")
        .embedding_dimension(1536)
        .batch_size(32)
        .build();

    let provider = create_api_provider(config).await.unwrap();

    let code = "def add(a, b): return a + b".to_string();

    // Generate embeddings twice
    let result1 = provider.embed(vec![code.clone()]).await.unwrap();
    let result2 = provider.embed(vec![code]).await.unwrap();

    let embed1 = result1[0].as_ref().unwrap();
    let embed2 = result2[0].as_ref().unwrap();

    let similarity = cosine_similarity(embed1, embed2);
    assert!(
        similarity > 0.9999,
        "Same input should produce nearly identical embeddings, got similarity: {similarity}"
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
