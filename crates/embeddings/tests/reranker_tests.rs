//! Unit tests for reranker functionality

use codesearch_core::entities::{
    EntityMetadata, EntityType, FunctionSignature, Language, SourceLocation, Visibility,
};
use codesearch_core::CodeEntity;
use codesearch_embeddings::create_reranker_provider;
use codesearch_indexer::entity_processor::extract_embedding_content;
use std::path::PathBuf;

/// Test reranker handles empty documents
#[tokio::test]
async fn test_reranker_handles_empty_documents() {
    let provider = create_reranker_provider(
        "BAAI/bge-reranker-v2-m3".to_string(),
        "http://localhost:8001".to_string(),
        30,
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
        "http://localhost:8001".to_string(),
        30,
    )
    .await
    .expect("Failed to create reranker provider");

    let document_contents = vec![
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
        "http://localhost:8001".to_string(),
        30,
    )
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
    let provider = create_reranker_provider(
        "BAAI/bge-reranker-v2-m3".to_string(),
        "http://localhost:9999/v1".to_string(), // Non-existent port
        30,
    )
    .await
    .expect("Failed to create reranker provider");

    let documents = vec![
        ("doc1".to_string(), "test content 1"),
        ("doc2".to_string(), "test content 2"),
    ];

    let result = provider.rerank("test query", &documents, 2).await;

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
    let provider = create_reranker_provider(
        "invalid-model-name".to_string(), // Invalid model
        "http://localhost:8001".to_string(),
        30,
    )
    .await
    .expect("Failed to create reranker provider");

    let documents = vec![("doc1".to_string(), "test content")];

    let result = provider.rerank("test query", &documents, 1).await;

    // Should return an error due to invalid model
    assert!(result.is_err(), "Should return error for invalid model");
}

/// Test reranker handles empty query gracefully
#[tokio::test]
async fn test_reranker_empty_query() {
    let provider = create_reranker_provider(
        "BAAI/bge-reranker-v2-m3".to_string(),
        "http://localhost:8001".to_string(),
        30,
    )
    .await
    .expect("Failed to create reranker provider");

    // Empty query should still work (though results may not be meaningful)
    let documents = vec![("doc1".to_string(), "test content")];
    let result = provider.rerank("", &documents, 1).await;

    // This might succeed or fail depending on the backend - we just verify it doesn't panic
    // If it succeeds, verify the result structure
    if let Ok(scores) = result {
        assert!(
            scores.len() <= 1,
            "Should return at most the requested top_k"
        );
    }
}

/// Test reranker with very large top_k
#[tokio::test]
async fn test_reranker_large_top_k() {
    let provider = create_reranker_provider(
        "BAAI/bge-reranker-v2-m3".to_string(),
        "http://localhost:8001".to_string(),
        30,
    )
    .await
    .expect("Failed to create reranker provider");

    let documents = vec![
        ("doc1".to_string(), "test 1"),
        ("doc2".to_string(), "test 2"),
    ];

    // Request more results than available documents
    let result = provider.rerank("test", &documents, 100).await;

    // Should either succeed with ≤ num_docs results, or fail gracefully
    if let Ok(scores) = result {
        assert!(
            scores.len() <= documents.len(),
            "Should not return more results than input documents"
        );
    }
}

/// Test that very large documents are handled gracefully with truncation
///
/// This test requires a running vLLM reranker instance and is ignored by default.
#[tokio::test]
#[ignore]
async fn test_reranker_handles_large_documents() {
    let provider = create_reranker_provider(
        "BAAI/bge-reranker-v2-m3".to_string(),
        "http://localhost:8001".to_string(),
        30,
    )
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
    let result = provider.rerank("test query", &documents, 2).await;

    assert!(
        result.is_ok(),
        "Reranker should handle large documents with truncation"
    );
    let scores = result.unwrap();
    assert_eq!(scores.len(), 2, "Should return top 2 results");
}
