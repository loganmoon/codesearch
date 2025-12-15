//! Embedding generation service

use super::models::{EmbeddingRequest, EmbeddingResponse};
use codesearch_core::error::Result;
use codesearch_embeddings::{EmbeddingManager, EmbeddingTask};
use std::sync::Arc;

/// Generate embeddings for the provided texts
///
/// Uses the Query task type since this endpoint is typically used for generating
/// search query embeddings. The provider handles task-specific formatting internally.
pub async fn generate_embeddings(
    request: EmbeddingRequest,
    embedding_manager: &Arc<EmbeddingManager>,
) -> Result<EmbeddingResponse> {
    let embeddings = embedding_manager
        .embed_for_task(request.texts, None, EmbeddingTask::Query)
        .await?;

    let dense_embeddings: Vec<Vec<f32>> = embeddings.into_iter().flatten().collect();

    let dimension = dense_embeddings.first().map(|e| e.len()).unwrap_or(0);

    Ok(EmbeddingResponse {
        embeddings: dense_embeddings,
        dimension,
    })
}
