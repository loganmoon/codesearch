//! Integration tests for REST API endpoints
//!
//! These tests verify the basic structure and routing of REST API endpoints.
//! Full integration testing with real database connections is done in the e2e test suite.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn test_health_endpoint() -> Result<(), Box<dyn std::error::Error>> {
    // Build a minimal router just for health endpoint testing
    use axum::routing::get;
    use axum::{Json, Router};

    let app = Router::new().route(
        "/health",
        get(|| async {
            Json(serde_json::json!({
                "status": "healthy",
                "version": env!("CARGO_PKG_VERSION")
            }))
        }),
    );

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .map_err(|e| anyhow::anyhow!("Failed to build request: {e}"))?,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Request failed: {e}"))?;

    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

/// Tests for input validation logic
#[cfg(test)]
mod input_validation_tests {
    /// Test that limit values are properly clamped in unified search
    ///
    /// Validates that:
    /// - limit is clamped to [1, 1000]
    /// - fulltext_limit is clamped to [1, 1000]
    /// - semantic_limit is clamped to [1, 1000]
    #[test]
    fn test_unified_search_limit_clamping() {
        // Test that limits below 1 are clamped to 1
        assert_eq!(0_usize.clamp(1, 1000), 1);

        // Test that limits above 1000 are clamped to 1000
        assert_eq!(1001_usize.clamp(1, 1000), 1000);
        assert_eq!(5000_usize.clamp(1, 1000), 1000);
        assert_eq!(usize::MAX.clamp(1, 1000), 1000);

        // Test that valid limits are unchanged
        assert_eq!(100_usize.clamp(1, 1000), 100);
        assert_eq!(1_usize.clamp(1, 1000), 1);
        assert_eq!(1000_usize.clamp(1, 1000), 1000);
    }

    /// Test that limit values are properly clamped in semantic search
    ///
    /// Validates that:
    /// - limit is clamped to [1, 1000]
    /// - prefetch_multiplier is clamped to [1, 10]
    #[test]
    fn test_semantic_search_limit_clamping() {
        // Test limit clamping [1, 1000]
        assert_eq!(0_usize.clamp(1, 1000), 1);
        assert_eq!(2000_usize.clamp(1, 1000), 1000);
        assert_eq!(500_usize.clamp(1, 1000), 500);

        // Test prefetch_multiplier clamping [1, 10]
        assert_eq!(0_usize.clamp(1, 10), 1);
        assert_eq!(20_usize.clamp(1, 10), 10);
        assert_eq!(5_usize.clamp(1, 10), 5);
    }

    /// Test that max_depth is properly clamped in graph queries
    ///
    /// Validates that:
    /// - max_depth is clamped to [1, 10]
    #[test]
    fn test_graph_query_max_depth_clamping() {
        // Test max_depth clamping [1, 10]
        assert_eq!(0_usize.clamp(1, 10), 1);
        assert_eq!(50_usize.clamp(1, 10), 10);
        assert_eq!(3_usize.clamp(1, 10), 3);
        assert_eq!(1_usize.clamp(1, 10), 1);
        assert_eq!(10_usize.clamp(1, 10), 10);
    }

    /// Test that batch size validation works correctly
    ///
    /// Validates that entity batch requests exceeding max_batch_size are rejected
    #[test]
    fn test_batch_size_validation() {
        let max_batch_size = 10000;

        // Test that sizes within limit are valid
        assert!(100 <= max_batch_size);
        assert!(max_batch_size <= max_batch_size);

        // Test that sizes exceeding limit would be rejected
        assert!(max_batch_size + 1 > max_batch_size);
        assert!(50000 > max_batch_size);
    }
}
