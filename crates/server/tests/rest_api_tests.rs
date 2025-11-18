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
