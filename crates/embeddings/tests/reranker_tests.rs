//! Unit tests for reranker functionality

use codesearch_embeddings::{create_reranker_provider, RerankerProvider};

/// Test reranker handles empty documents
#[tokio::test]
async fn test_reranker_handles_empty_documents() {
    let provider = create_reranker_provider(
        "BAAI/bge-reranker-v2-m3".to_string(),
        "http://localhost:8001/v1".to_string(),
    )
    .await
    .expect("Failed to create reranker provider");

    let result = provider.rerank("test query", &[], 10).await;

    assert!(result.is_ok());
    let scores = result.unwrap();
    assert!(scores.is_empty());
}

/// Test reranker respects top_k parameter
///
/// This test requires a running vLLM reranker instance and is ignored by default.
/// Run with: cargo test --package codesearch-embeddings -- --ignored test_reranker_respects_top_k
#[tokio::test]
#[ignore]
async fn test_reranker_respects_top_k() {
    let provider = create_reranker_provider(
        "BAAI/bge-reranker-v2-m3".to_string(),
        "http://localhost:8001/v1".to_string(),
    )
    .await
    .expect("Failed to create reranker provider");

    let documents = vec![
        ("doc1".to_string(), "function foo() {}".to_string()),
        ("doc2".to_string(), "function bar() {}".to_string()),
        ("doc3".to_string(), "function baz() {}".to_string()),
        ("doc4".to_string(), "function qux() {}".to_string()),
        ("doc5".to_string(), "function quux() {}".to_string()),
    ];

    let result = provider
        .rerank("function implementation", &documents, 2)
        .await;

    assert!(result.is_ok());
    let scores = result.unwrap();
    assert_eq!(scores.len(), 2, "Should return exactly 2 results");
}

/// Test reranker basic functionality
///
/// This test requires a running vLLM reranker instance and is ignored by default.
/// Run with: cargo test --package codesearch-embeddings -- --ignored test_reranker_basic_functionality
#[tokio::test]
#[ignore]
async fn test_reranker_basic_functionality() {
    let provider = create_reranker_provider(
        "BAAI/bge-reranker-v2-m3".to_string(),
        "http://localhost:8001/v1".to_string(),
    )
    .await
    .expect("Failed to create reranker provider");

    let documents = vec![
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

    let result = provider
        .rerank("arithmetic addition function", &documents, 3)
        .await;

    assert!(result.is_ok());
    let scores = result.unwrap();
    assert_eq!(scores.len(), 3, "Should return 3 results");

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
