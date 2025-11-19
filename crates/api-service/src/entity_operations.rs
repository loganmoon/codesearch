//! Entity operation services for batch retrieval and repository listing

use crate::models::{
    BatchEntityRequest, BatchEntityResponse, EntityResult, ListRepositoriesResponse,
    RepositoryInfo, ResponseMetadata,
};
use codesearch_core::error::Result;
use codesearch_storage::PostgresClientTrait;
use std::sync::Arc;
use std::time::Instant;

/// Get entities in batch by their IDs
pub async fn get_entities_batch(
    request: BatchEntityRequest,
    postgres_client: &Arc<dyn PostgresClientTrait>,
) -> Result<BatchEntityResponse> {
    let start_time = Instant::now();

    let entities = postgres_client
        .get_entities_by_ids(&request.entity_refs)
        .await?;

    let results: Vec<EntityResult> = entities
        .into_iter()
        .map(|e| e.try_into())
        .collect::<Result<Vec<_>>>()?;

    let query_time_ms = start_time.elapsed().as_millis() as u64;
    let total_results = results.len();

    Ok(BatchEntityResponse {
        entities: results,
        metadata: ResponseMetadata {
            total_results,
            repositories_searched: 0,
            reranked: false,
            query_time_ms,
        },
    })
}

/// List all indexed repositories
pub async fn list_repositories(
    postgres_client: &Arc<dyn PostgresClientTrait>,
) -> Result<ListRepositoriesResponse> {
    let repos = postgres_client.list_all_repositories().await?;

    let repo_list: Vec<RepositoryInfo> = repos
        .into_iter()
        .map(
            |(repository_id, collection_name, repository_path)| RepositoryInfo {
                repository_id,
                repository_name: repository_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                repository_path: repository_path.display().to_string(),
                collection_name,
                last_indexed_commit: None,
            },
        )
        .collect();

    let total = repo_list.len();

    Ok(ListRepositoriesResponse {
        repositories: repo_list,
        total,
    })
}
