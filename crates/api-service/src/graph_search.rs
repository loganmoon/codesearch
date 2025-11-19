//! Graph search service for Neo4j-based code relationship queries

use crate::models::{
    GraphQueryRequest, GraphQueryResponse, GraphQueryType, GraphResponseMetadata, GraphResult,
};
use codesearch_core::error::Result;
use codesearch_indexer::entity_processor::extract_embedding_content;
use codesearch_storage::{Neo4jClientTrait, PostgresClientTrait};
use std::sync::Arc;
use std::time::Instant;
use tracing::warn;

/// Execute graph-based queries using Neo4j
pub async fn query_graph(
    mut request: GraphQueryRequest,
    neo4j_client: &Arc<dyn Neo4jClientTrait>,
    postgres_client: &Arc<dyn PostgresClientTrait>,
    reranker: &Option<Arc<dyn codesearch_embeddings::RerankerProvider>>,
) -> Result<GraphQueryResponse> {
    let start_time = Instant::now();

    // Validate and clamp input parameters to prevent resource exhaustion
    request.limit = request.limit.clamp(1, 1000);
    if let Some(ref mut md) = request.parameters.max_depth {
        *md = (*md).clamp(1, 10);
    }

    let is_ready = postgres_client
        .is_graph_ready(request.repository_id)
        .await?;

    let warning = if !is_ready {
        Some("Graph is incomplete (indexing in progress). Results may be partial.".to_string())
    } else {
        None
    };

    let qualified_names = execute_graph_query(&request, neo4j_client, postgres_client).await?;

    let (results, semantic_filter_applied) = if request.semantic_filter.is_some() {
        apply_semantic_filter(qualified_names, &request, postgres_client, reranker).await?
    } else {
        let results: Vec<GraphResult> = qualified_names
            .into_iter()
            .take(request.limit)
            .map(|qname| GraphResult {
                qualified_name: qname,
                relevance_score: None,
                entity: None,
            })
            .collect();
        (results, false)
    };

    let query_time_ms = start_time.elapsed().as_millis() as u64;

    Ok(GraphQueryResponse {
        results: results.clone(),
        metadata: GraphResponseMetadata {
            total_results: results.len(),
            semantic_filter_applied,
            query_time_ms,
            warning,
        },
    })
}

async fn execute_graph_query(
    request: &GraphQueryRequest,
    neo4j_client: &Arc<dyn Neo4jClientTrait>,
    postgres_client: &Arc<dyn PostgresClientTrait>,
) -> Result<Vec<String>> {
    let db_name = neo4j_client
        .ensure_repository_database(request.repository_id, postgres_client.as_ref())
        .await?;

    neo4j_client.use_database(&db_name).await?;

    match request.query_type {
        GraphQueryType::FindFunctionCallers => neo4j_client
            .find_function_callers(
                postgres_client,
                request.repository_id,
                &request.parameters.qualified_name,
                request.parameters.max_depth.unwrap_or(3),
            )
            .await
            .map(|results| results.into_iter().map(|(name, _depth)| name).collect())
            .map_err(|e| codesearch_core::error::Error::storage(e.to_string())),
        GraphQueryType::FindFunctionCallees => neo4j_client
            .find_function_callees(
                postgres_client,
                request.repository_id,
                &request.parameters.qualified_name,
                request.parameters.max_depth.unwrap_or(3),
            )
            .await
            .map(|results| results.into_iter().map(|(name, _depth)| name).collect())
            .map_err(|e| codesearch_core::error::Error::storage(e.to_string())),
        GraphQueryType::FindTraitImplementations => neo4j_client
            .find_trait_implementations(
                postgres_client,
                request.repository_id,
                &request.parameters.qualified_name,
            )
            .await
            .map_err(|e| codesearch_core::error::Error::storage(e.to_string())),
        GraphQueryType::FindClassHierarchy => neo4j_client
            .find_class_hierarchy(
                postgres_client,
                request.repository_id,
                &request.parameters.qualified_name,
            )
            .await
            .map(|hierarchy| hierarchy.into_iter().flatten().collect())
            .map_err(|e| codesearch_core::error::Error::storage(e.to_string())),
        GraphQueryType::FindModuleContents => neo4j_client
            .find_functions_in_module(
                postgres_client,
                request.repository_id,
                &request.parameters.qualified_name,
            )
            .await
            .map_err(|e| codesearch_core::error::Error::storage(e.to_string())),
        GraphQueryType::FindModuleDependencies => neo4j_client
            .find_module_dependencies(
                postgres_client,
                request.repository_id,
                &request.parameters.qualified_name,
            )
            .await
            .map_err(|e| codesearch_core::error::Error::storage(e.to_string())),
        GraphQueryType::FindUnusedFunctions => neo4j_client
            .find_unused_functions(postgres_client, request.repository_id)
            .await
            .map_err(|e| codesearch_core::error::Error::storage(e.to_string())),
        GraphQueryType::FindCircularDependencies => neo4j_client
            .find_circular_dependencies(postgres_client, request.repository_id)
            .await
            .map(|cycles| cycles.into_iter().flatten().collect())
            .map_err(|e| codesearch_core::error::Error::storage(e.to_string())),
    }
}

async fn apply_semantic_filter(
    qualified_names: Vec<String>,
    request: &GraphQueryRequest,
    postgres_client: &Arc<dyn PostgresClientTrait>,
    reranker: &Option<Arc<dyn codesearch_embeddings::RerankerProvider>>,
) -> Result<(Vec<GraphResult>, bool)> {
    if qualified_names.is_empty() {
        return Ok((vec![], false));
    }

    let entities = postgres_client
        .get_entities_by_qualified_names(request.repository_id, &qualified_names)
        .await
        .map_err(|e| {
            warn!("Failed to fetch entities for semantic filtering: {e}");
            e
        })?;

    if entities.is_empty() {
        warn!("No entities found for semantic filtering, returning unfiltered results");
        let results = qualified_names
            .into_iter()
            .take(request.limit)
            .map(|qname| GraphResult {
                qualified_name: qname,
                relevance_score: None,
                entity: None,
            })
            .collect();
        return Ok((results, false));
    }

    if let Some(ref reranker_provider) = reranker {
        if let Some(ref semantic_filter) = request.semantic_filter {
            let entity_contents: Vec<(String, String)> = qualified_names
                .iter()
                .filter_map(|qname| {
                    entities
                        .get(qname)
                        .map(|entity| (qname.clone(), extract_embedding_content(entity)))
                })
                .collect();

            let documents: Vec<(String, &str)> = entity_contents
                .iter()
                .map(|(qname, content)| (qname.clone(), content.as_str()))
                .collect();

            match reranker_provider
                .rerank(semantic_filter, &documents, request.limit)
                .await
            {
                Ok(reranked) => {
                    let results: Vec<GraphResult> = reranked
                        .into_iter()
                        .map(|(qname, score)| {
                            let entity = if request.return_entities {
                                entities
                                    .get(&qname)
                                    .map(|e| e.clone().try_into())
                                    .transpose()
                            } else {
                                Ok(None)
                            }?;

                            Ok(GraphResult {
                                qualified_name: qname,
                                relevance_score: Some(score),
                                entity,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;

                    return Ok((results, true));
                }
                Err(e) => {
                    warn!("Reranking failed: {e}, returning unscored results");
                }
            }
        }
    }

    let results: Vec<GraphResult> = qualified_names
        .into_iter()
        .take(request.limit)
        .map(|qname| {
            let entity = if request.return_entities {
                entities
                    .get(&qname)
                    .map(|e| e.clone().try_into())
                    .transpose()
            } else {
                Ok(None)
            }?;

            Ok(GraphResult {
                qualified_name: qname,
                relevance_score: None,
                entity,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok((results, false))
}
