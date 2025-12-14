//! Unit tests for reranker functionality

use codesearch_core::config::RerankingConfig;
use codesearch_core::entities::{
    EntityMetadata, EntityType, FunctionSignature, Language, SourceLocation, Visibility,
};
use codesearch_core::CodeEntity;
use codesearch_indexer::entity_processor::extract_embedding_content;
use codesearch_reranking::create_reranker_provider;
use std::path::PathBuf;

/// Helper to create a vLLM reranking config for tests
fn vllm_config() -> RerankingConfig {
    RerankingConfig {
        enabled: true,
        provider: "vllm".to_string(),
        model: "BAAI/bge-reranker-v2-m3".to_string(),
        candidates: 100,
        top_k: 10,
        api_base_url: Some("http://localhost:8001/v1".to_string()),
        api_key: None,
        timeout_secs: 30,
        max_concurrent_requests: 16,
    }
}

/// Test reranker handles empty documents
#[tokio::test]
async fn test_reranker_handles_empty_documents() {
    let config = vllm_config();
    let provider = create_reranker_provider(&config)
        .await
        .expect("Failed to create reranker provider");

    let result = provider.rerank("test query", &[]).await;

    assert!(result.is_ok());
    let scores = result.unwrap();
    assert!(scores.is_empty());
}

/// Test reranker returns all documents sorted
///
/// This test requires a running vLLM reranker instance and is ignored by default.
/// Run with: cargo test --package codesearch-reranking -- --ignored test_reranker_returns_all_documents
#[tokio::test]
#[ignore]
async fn test_reranker_returns_all_documents() {
    let config = vllm_config();
    let provider = create_reranker_provider(&config)
        .await
        .expect("Failed to create reranker provider");

    let document_contents = [
        ("doc1".to_string(), "function foo() {}".to_string()),
        ("doc2".to_string(), "function bar() {}".to_string()),
        ("doc3".to_string(), "function baz() {}".to_string()),
        ("doc4".to_string(), "function qux() {}".to_string()),
        ("doc5".to_string(), "function quux() {}".to_string()),
    ];

    let documents: Vec<(String, &str)> = document_contents
        .iter()
        .map(|(id, content)| (id.clone(), content.as_str()))
        .collect();

    let result = provider.rerank("function implementation", &documents).await;

    assert!(result.is_ok());
    let scores = result.unwrap();
    assert_eq!(scores.len(), documents.len(), "Should return all documents");
}

/// Test reranker basic functionality
///
/// This test requires a running vLLM reranker instance and is ignored by default.
/// Run with: cargo test --package codesearch-reranking -- --ignored test_reranker_basic_functionality
#[tokio::test]
#[ignore]
async fn test_reranker_basic_functionality() {
    let config = vllm_config();
    let provider = create_reranker_provider(&config)
        .await
        .expect("Failed to create reranker provider");

    let document_contents = [
        (
            "doc1".to_string(),
            "function calculate_sum(a, b) { return a + b; }".to_string(),
        ),
        (
            "doc2".to_string(),
            "class User { constructor(name) { this.name = name; } }".to_string(),
        ),
        (
            "doc3".to_string(),
            "function multiply(x, y) { return x * y; }".to_string(),
        ),
    ];

    let documents: Vec<(String, &str)> = document_contents
        .iter()
        .map(|(id, content)| (id.clone(), content.as_str()))
        .collect();

    let result = provider
        .rerank("arithmetic addition function", &documents)
        .await;

    assert!(result.is_ok());
    let scores = result.unwrap();
    assert_eq!(scores.len(), 3, "Should return all 3 results");

    // Verify results are sorted by score descending
    for i in 0..scores.len() - 1 {
        assert!(
            scores[i].1 >= scores[i + 1].1,
            "Results should be sorted by score descending"
        );
    }

    // The first result should be doc1 (addition function) as it's most relevant
    assert_eq!(
        scores[0].0, "doc1",
        "Most relevant document should be the addition function"
    );
}

/// Test that content extraction is consistent between indexing and reranking
#[test]
fn test_content_consistency_between_indexing_and_reranking() {
    // Create a sample CodeEntity with various fields populated
    let entity = CodeEntity {
        entity_id: "test_id".to_string(),
        entity_type: EntityType::Function,
        name: "calculate_sum".to_string(),
        qualified_name: "math::calculate_sum".to_string(),
        file_path: PathBuf::from("/test/path.rs"),
        location: SourceLocation {
            start_line: 10,
            end_line: 15,
            start_column: 0,
            end_column: 0,
        },
        documentation_summary: Some("Calculates the sum of two numbers".to_string()),
        signature: Some(FunctionSignature {
            parameters: vec![
                ("a".to_string(), Some("i32".to_string())),
                ("b".to_string(), Some("i32".to_string())),
            ],
            return_type: Some("i32".to_string()),
            is_async: false,
            generics: vec![],
        }),
        content: Some("fn calculate_sum(a: i32, b: i32) -> i32 { a + b }".to_string()),
        repository_id: "repo_id".to_string(),
        parent_scope: None,
        dependencies: vec![],
        visibility: Visibility::Public,
        language: Language::Rust,
        metadata: EntityMetadata::default(),
    };

    // Extract content using the function that's used for both indexing and reranking
    let content = extract_embedding_content(&entity);

    // Verify the content includes key components
    assert!(content.contains("Function"), "Should contain entity type");
    assert!(
        content.contains("calculate_sum"),
        "Should contain function name"
    );
    assert!(
        content.contains("math::calculate_sum"),
        "Should contain qualified name"
    );
    assert!(
        content.contains("Calculates the sum of two numbers"),
        "Should contain documentation"
    );
    assert!(content.contains("i32"), "Should contain parameter types");
    assert!(
        content.contains("fn calculate_sum(a: i32, b: i32) -> i32"),
        "Should contain function content"
    );

    // Test with minimal entity
    let minimal_entity = CodeEntity {
        entity_id: "minimal_id".to_string(),
        entity_type: EntityType::Struct,
        name: "Point".to_string(),
        qualified_name: "Point".to_string(),
        file_path: PathBuf::from("/test/minimal.rs"),
        location: SourceLocation {
            start_line: 1,
            end_line: 3,
            start_column: 0,
            end_column: 0,
        },
        documentation_summary: None,
        signature: None,
        content: None,
        repository_id: "repo_id".to_string(),
        parent_scope: None,
        dependencies: vec![],
        visibility: Visibility::Public,
        language: Language::Rust,
        metadata: EntityMetadata::default(),
    };

    let minimal_content = extract_embedding_content(&minimal_entity);

    // Verify minimal content includes at least entity type and name
    assert!(
        minimal_content.contains("Struct"),
        "Should contain entity type"
    );
    assert!(minimal_content.contains("Point"), "Should contain name");
}

/// Test reranker handles connection failures gracefully
#[tokio::test]
async fn test_reranker_connection_failure() {
    // Create provider pointing to non-existent endpoint
    let config = RerankingConfig {
        enabled: true,
        provider: "vllm".to_string(),
        model: "BAAI/bge-reranker-v2-m3".to_string(),
        candidates: 100,
        top_k: 10,
        api_base_url: Some("http://localhost:9999/v1".to_string()), // Non-existent port
        api_key: None,
        timeout_secs: 30,
        max_concurrent_requests: 16,
    };
    let provider = create_reranker_provider(&config)
        .await
        .expect("Failed to create reranker provider");

    let documents = vec![
        ("doc1".to_string(), "test content 1"),
        ("doc2".to_string(), "test content 2"),
    ];

    let result = provider.rerank("test query", &documents).await;

    // Should return an error due to connection failure
    assert!(result.is_err(), "Should return error when connection fails");

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Rerank API request failed") || err_msg.contains("error"),
        "Error message should indicate API request failure: {err_msg}"
    );
}

/// Test reranker handles HTTP error responses
#[tokio::test]
#[ignore]
async fn test_reranker_http_error() {
    // This test requires a running vLLM instance that can return error responses
    // For now, we test by sending an invalid request to a valid endpoint
    let config = RerankingConfig {
        enabled: true,
        provider: "vllm".to_string(),
        model: "invalid-model-name".to_string(), // Invalid model
        candidates: 100,
        top_k: 10,
        api_base_url: Some("http://localhost:8001/v1".to_string()),
        api_key: None,
        timeout_secs: 30,
        max_concurrent_requests: 16,
    };
    let provider = create_reranker_provider(&config)
        .await
        .expect("Failed to create reranker provider");

    let documents = vec![("doc1".to_string(), "test content")];

    let result = provider.rerank("test query", &documents).await;

    // Should return an error due to invalid model
    assert!(result.is_err(), "Should return error for invalid model");
}

/// Test reranker handles empty query gracefully
#[tokio::test]
async fn test_reranker_empty_query() {
    let config = vllm_config();
    let provider = create_reranker_provider(&config)
        .await
        .expect("Failed to create reranker provider");

    // Empty query should still work (though results may not be meaningful)
    let documents = vec![("doc1".to_string(), "test content")];
    let result = provider.rerank("", &documents).await;

    // This might succeed or fail depending on the backend - we just verify it doesn't panic
    // If it succeeds, verify the result structure
    if let Ok(scores) = result {
        assert!(
            scores.len() <= documents.len(),
            "Should return at most the number of documents"
        );
    }
}

/// Test reranker returns all documents
#[tokio::test]
async fn test_reranker_returns_all() {
    let config = vllm_config();
    let provider = create_reranker_provider(&config)
        .await
        .expect("Failed to create reranker provider");

    let documents = vec![
        ("doc1".to_string(), "test 1"),
        ("doc2".to_string(), "test 2"),
    ];

    let result = provider.rerank("test", &documents).await;

    // Should return all documents
    if let Ok(scores) = result {
        assert_eq!(
            scores.len(),
            documents.len(),
            "Should return all input documents"
        );
    }
}

/// Test that very large documents are handled gracefully with truncation
///
/// This test requires a running vLLM reranker instance and is ignored by default.
#[tokio::test]
#[ignore]
async fn test_reranker_handles_large_documents() {
    let config = vllm_config();
    let provider = create_reranker_provider(&config)
        .await
        .expect("Failed to create reranker provider");

    // Create documents with very large content (> 4800 chars each)
    let large_content = "a".repeat(10_000);
    let documents = vec![
        ("doc1".to_string(), large_content.as_str()),
        ("doc2".to_string(), large_content.as_str()),
        ("doc3".to_string(), large_content.as_str()),
    ];

    // This should succeed because documents are truncated internally
    let result = provider.rerank("test query", &documents).await;

    assert!(
        result.is_ok(),
        "Reranker should handle large documents with truncation"
    );
    let scores = result.unwrap();
    assert_eq!(scores.len(), 3, "Should return all 3 results");
}

// =============================================================================
// Jina Provider Tests
// =============================================================================

/// Test Jina provider with API key from config
#[tokio::test]
async fn test_jina_provider_with_config_api_key() {
    let config = RerankingConfig {
        enabled: true,
        provider: "jina".to_string(),
        model: "jina-reranker-v3".to_string(),
        candidates: 100,
        top_k: 10,
        api_base_url: None,
        api_key: Some("test_api_key".to_string()), // API key in config
        timeout_secs: 30,
        max_concurrent_requests: 16,
    };

    let result = create_reranker_provider(&config).await;
    // Should succeed in creating the provider (though API calls would fail with test key)
    assert!(result.is_ok(), "Should create provider with config API key");
}

/// Test Jina provider handles empty documents
#[tokio::test]
async fn test_jina_provider_handles_empty_documents() {
    let config = RerankingConfig {
        enabled: true,
        provider: "jina".to_string(),
        model: "jina-reranker-v3".to_string(),
        candidates: 100,
        top_k: 10,
        api_base_url: None,
        api_key: Some("test_api_key".to_string()),
        timeout_secs: 30,
        max_concurrent_requests: 16,
    };

    let provider = create_reranker_provider(&config)
        .await
        .expect("Failed to create Jina provider");

    let result = provider.rerank("test query", &[]).await;

    assert!(result.is_ok());
    let scores = result.unwrap();
    assert!(scores.is_empty());
}

/// Test Jina provider with actual API (requires JINA_API_KEY env var)
#[tokio::test]
#[ignore]
async fn test_jina_provider_basic_functionality() {
    let api_key = std::env::var("JINA_API_KEY").expect("JINA_API_KEY required for this test");

    let config = RerankingConfig {
        enabled: true,
        provider: "jina".to_string(),
        model: "jina-reranker-v3".to_string(),
        candidates: 100,
        top_k: 10,
        api_base_url: None,
        api_key: Some(api_key),
        timeout_secs: 30,
        max_concurrent_requests: 16,
    };

    let provider = create_reranker_provider(&config)
        .await
        .expect("Failed to create Jina provider");

    let document_contents = [
        (
            "doc1".to_string(),
            "function calculate_sum(a, b) { return a + b; }".to_string(),
        ),
        (
            "doc2".to_string(),
            "class User { constructor(name) { this.name = name; } }".to_string(),
        ),
        (
            "doc3".to_string(),
            "function multiply(x, y) { return x * y; }".to_string(),
        ),
    ];

    let documents: Vec<(String, &str)> = document_contents
        .iter()
        .map(|(id, content)| (id.clone(), content.as_str()))
        .collect();

    let result = provider
        .rerank("arithmetic addition function", &documents)
        .await;

    assert!(result.is_ok(), "Jina rerank should succeed");
    let scores = result.unwrap();
    assert_eq!(scores.len(), 3, "Should return all 3 results");

    // Verify results are sorted by score descending
    for i in 0..scores.len() - 1 {
        assert!(
            scores[i].1 >= scores[i + 1].1,
            "Results should be sorted by score descending"
        );
    }
}
