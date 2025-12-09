//! Full-text search service implementation
//!
//! Simple wrapper around PostgreSQL full-text search capabilities.

use super::models::{
    EntityResult, FulltextSearchRequest, FulltextSearchResponse, ResponseMetadata,
};
use codesearch_core::error::Result;
use codesearch_storage::PostgresClientTrait;
use std::sync::Arc;
use std::time::Instant;

/// Execute full-text search using PostgreSQL GIN indexes
pub async fn search_fulltext(
    request: FulltextSearchRequest,
    postgres_client: &Arc<dyn PostgresClientTrait>,
) -> Result<FulltextSearchResponse> {
    let start_time = Instant::now();

    let entities = postgres_client
        .search_entities_fulltext(
            request.repository_id,
            &request.query,
            request.limit as i64,
            false,
        )
        .await?;

    let results: Vec<EntityResult> = entities
        .into_iter()
        .map(|e| e.try_into())
        .collect::<Result<Vec<_>>>()?;

    let query_time_ms = start_time.elapsed().as_millis() as u64;
    let total_results = results.len();

    Ok(FulltextSearchResponse {
        results,
        metadata: ResponseMetadata {
            total_results,
            repositories_searched: 1,
            reranked: false,
            query_time_ms,
        },
    })
}
