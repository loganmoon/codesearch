//! Embedding generation service

use crate::models::{EmbeddingRequest, EmbeddingResponse};
use codesearch_core::error::Result;
use codesearch_embeddings::EmbeddingManager;
use std::sync::Arc;

/// Generate embeddings for the provided texts
pub async fn generate_embeddings(
    request: EmbeddingRequest,
    embedding_manager: &Arc<EmbeddingManager>,
    default_instruction: &str,
) -> Result<EmbeddingResponse> {
    let instruction = request
        .instruction
        .as_deref()
        .unwrap_or(default_instruction);

    let formatted_texts: Vec<String> = request
        .texts
        .iter()
        .map(|text| format!("<instruct>{instruction}\n<query>{text}"))
        .collect();

    let embeddings = embedding_manager.embed(formatted_texts).await?;

    let dense_embeddings: Vec<Vec<f32>> = embeddings.into_iter().flatten().collect();

    let dimension = dense_embeddings.first().map(|e| e.len()).unwrap_or(0);

    Ok(EmbeddingResponse {
        embeddings: dense_embeddings,
        dimension,
    })
}
