//! Semantic search service implementation
//!
//! Provides semantic code search using vector embeddings, hybrid search,
//! and optional reranking for improved relevance.

use super::models::{
    BackendClients, EntityResult, ResponseMetadata, SearchConfig, SemanticSearchRequest,
    SemanticSearchResponse,
};
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use codesearch_indexer::entity_processor::extract_embedding_content;
use ordered_float::OrderedFloat;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tracing::warn;
use uuid::Uuid;

/// Main entry point for semantic search
pub async fn search_semantic(
    mut request: SemanticSearchRequest,
    clients: &BackendClients,
    config: &SearchConfig,
) -> Result<SemanticSearchResponse> {
    let start_time = Instant::now();

    // Validate and clamp input parameters to prevent resource exhaustion
    let limit = request.limit.clamp(1, 1000);
    if let Some(ref mut pm) = request.prefetch_multiplier {
        *pm = (*pm).clamp(1, 10);
    }

    // Step 1: Structural filtering (if needed)
    let structural_filter = if has_structural_filters(&request.filters) {
        Some(apply_structural_filters(&request, clients).await?)
    } else {
        None
    };

    // Step 2: Generate embeddings
    let (dense_embedding, avgdl_to_sparse) =
        generate_query_embeddings(&request, clients, config).await?;

    // Step 3: Determine target repositories
    let target_repos =
        determine_target_repositories(request.repository_ids.take(), clients).await?;

    if target_repos.is_empty() {
        return Err(Error::config(
            "No repositories available to search".to_string(),
        ));
    }

    // Step 4: Execute parallel repository searches
    let candidates = search_repositories(
        &dense_embedding,
        &avgdl_to_sparse,
        &target_repos,
        limit,
        &request,
        clients,
        config,
    )
    .await?;

    // Step 5: Fetch entities from Postgres
    let entities = fetch_entities(&candidates, clients).await?;

    // Step 6: Apply structural filtering
    let filtered_entities = if let Some(ref allowed_names) = structural_filter {
        entities
            .into_iter()
            .filter(|e| allowed_names.contains(&e.qualified_name))
            .collect()
    } else {
        entities
    };

    if filtered_entities.is_empty() && structural_filter.is_some() {
        return Ok(SemanticSearchResponse {
            results: vec![],
            metadata: ResponseMetadata {
                total_results: 0,
                repositories_searched: target_repos.len(),
                reranked: false,
                query_time_ms: start_time.elapsed().as_millis() as u64,
            },
        });
    }

    // Step 7: Reranking
    let (final_results, reranked) = rerank_results(
        filtered_entities,
        &request,
        &candidates,
        clients,
        config,
        limit,
    )
    .await?;

    // Step 8: Build response
    let query_time_ms = start_time.elapsed().as_millis() as u64;
    let total_results = final_results.len();
    Ok(SemanticSearchResponse {
        results: final_results,
        metadata: ResponseMetadata {
            total_results,
            repositories_searched: target_repos.len(),
            reranked,
            query_time_ms,
        },
    })
}

fn has_structural_filters(filters: &Option<super::models::SearchFilters>) -> bool {
    if let Some(f) = filters {
        f.implements_trait.is_some()
            || f.called_by.is_some()
            || f.calls.is_some()
            || f.in_module.is_some()
    } else {
        false
    }
}

async fn apply_structural_filters(
    request: &SemanticSearchRequest,
    clients: &BackendClients,
) -> Result<HashSet<String>> {
    let neo4j = clients
        .neo4j
        .as_ref()
        .ok_or_else(|| Error::config("Structural filters require Neo4j".to_string()))?;

    let target_repo_id = request
        .repository_ids
        .as_ref()
        .and_then(|ids| ids.first())
        .ok_or_else(|| {
            Error::config("Structural filters require specifying a single repository".to_string())
        })?;

    let mut qualified_names: Option<HashSet<String>> = None;

    if let Some(ref filters) = request.filters {
        if let Some(ref trait_name) = filters.implements_trait {
            let impls = neo4j
                .find_trait_implementations(&clients.postgres, *target_repo_id, trait_name)
                .await
                .map_err(|e| {
                    warn!("Trait implementation query failed: {e}");
                    e
                })
                .ok();

            if let Some(impls) = impls {
                let impls_set: HashSet<String> = impls.into_iter().collect();
                qualified_names = Some(match qualified_names {
                    None => impls_set,
                    Some(existing) => existing.intersection(&impls_set).cloned().collect(),
                });
            }
        }

        if let Some(ref module_name) = filters.in_module {
            let functions = neo4j
                .find_functions_in_module(&clients.postgres, *target_repo_id, module_name)
                .await
                .map_err(|e| {
                    warn!("Module functions query failed: {e}");
                    e
                })
                .ok();

            if let Some(functions) = functions {
                let functions_set: HashSet<String> = functions.into_iter().collect();
                qualified_names = Some(match qualified_names {
                    None => functions_set,
                    Some(existing) => existing.intersection(&functions_set).cloned().collect(),
                });
            }
        }

        if let Some(ref function_name) = filters.calls {
            let callers = neo4j
                .find_function_callers(&clients.postgres, *target_repo_id, function_name, 3)
                .await
                .map_err(|e| {
                    warn!("Function callers query failed: {e}");
                    e
                })
                .ok();

            if let Some(callers) = callers {
                let callers_set: HashSet<String> =
                    callers.into_iter().map(|(name, _)| name).collect();
                qualified_names = Some(match qualified_names {
                    None => callers_set,
                    Some(existing) => existing.intersection(&callers_set).cloned().collect(),
                });
            }
        }

        if let Some(ref function_name) = filters.called_by {
            let callees = neo4j
                .find_function_callees(&clients.postgres, *target_repo_id, function_name, 3)
                .await
                .map_err(|e| {
                    warn!("Function callees query failed: {e}");
                    e
                })
                .ok();

            if let Some(callees) = callees {
                let callees_set: HashSet<String> =
                    callees.into_iter().map(|(name, _)| name).collect();
                qualified_names = Some(match qualified_names {
                    None => callees_set,
                    Some(existing) => existing.intersection(&callees_set).cloned().collect(),
                });
            }
        }
    }

    qualified_names.ok_or_else(|| Error::config("No structural filter results".to_string()))
}

async fn generate_query_embeddings(
    request: &SemanticSearchRequest,
    clients: &BackendClients,
    config: &SearchConfig,
) -> Result<(Vec<f32>, HashMap<OrderedFloat<f32>, Vec<(u32, f32)>>)> {
    let query_text = &request.query.text;

    let bge_instruction = request
        .query
        .instruction
        .as_ref()
        .unwrap_or(&config.default_bge_instruction)
        .clone();

    let formatted_query = format!("<instruct>{bge_instruction}\n<query>{query_text}");

    let embeddings = clients
        .embedding_manager
        .embed(vec![formatted_query])
        .await?;

    let dense_embedding = embeddings
        .into_iter()
        .next()
        .ok_or_else(|| Error::config("No embedding returned".to_string()))?
        .ok_or_else(|| Error::config("Embedding provider returned None".to_string()))?;

    let avgdl_to_sparse = HashMap::new();

    Ok((dense_embedding, avgdl_to_sparse))
}

async fn determine_target_repositories(
    repository_ids: Option<Vec<Uuid>>,
    clients: &BackendClients,
) -> Result<Vec<Uuid>> {
    if let Some(ids) = repository_ids {
        Ok(ids)
    } else {
        let all_repos = clients.postgres.list_all_repositories().await?;
        Ok(all_repos.into_iter().map(|(id, _, _)| id).collect())
    }
}

async fn search_repositories(
    dense_embedding: &[f32],
    _avgdl_to_sparse: &HashMap<OrderedFloat<f32>, Vec<(u32, f32)>>,
    target_repos: &[Uuid],
    limit: usize,
    request: &SemanticSearchRequest,
    clients: &BackendClients,
    config: &SearchConfig,
) -> Result<Vec<(Uuid, String, f32)>> {
    let prefetch_multiplier = request
        .prefetch_multiplier
        .unwrap_or(config.hybrid_search.prefetch_multiplier);

    let repo_ids = target_repos.to_vec();
    let stats_batch = clients
        .postgres
        .get_bm25_statistics_batch(&repo_ids)
        .await?;

    let mut avgdl_to_repos: HashMap<OrderedFloat<f32>, Vec<Uuid>> = HashMap::new();
    for repo_id in target_repos {
        let stats = stats_batch
            .get(repo_id)
            .ok_or_else(|| Error::config(format!("Missing BM25 stats for {repo_id}")))?;
        avgdl_to_repos
            .entry(OrderedFloat(stats.avgdl))
            .or_default()
            .push(*repo_id);
    }

    let mut avgdl_to_sparse: HashMap<OrderedFloat<f32>, Vec<(u32, f32)>> = HashMap::new();
    for avgdl in avgdl_to_repos.keys() {
        let sparse_manager = codesearch_embeddings::create_sparse_manager(avgdl.0)?;
        let sparse_embeddings = sparse_manager
            .embed_sparse(vec![request.query.text.as_str()])
            .await?;
        let sparse_embedding = sparse_embeddings
            .into_iter()
            .next()
            .flatten()
            .ok_or_else(|| Error::config("Failed to generate sparse embedding".to_string()))?;
        avgdl_to_sparse.insert(*avgdl, sparse_embedding);
    }

    let filters = super::models::build_storage_filters(&request.filters);

    let dense_query_arc = std::sync::Arc::new(dense_embedding.to_vec());
    let avgdl_to_sparse_arc = std::sync::Arc::new(avgdl_to_sparse);
    let stats_batch_arc = std::sync::Arc::new(stats_batch);

    let search_futures = target_repos.iter().map(|repo_id| {
        let dense_query = std::sync::Arc::clone(&dense_query_arc);
        let avgdl_sparse = std::sync::Arc::clone(&avgdl_to_sparse_arc);
        let stats = std::sync::Arc::clone(&stats_batch_arc);
        let filters_clone = filters.clone();
        let repo_id = *repo_id;

        async move {
            let avgdl = stats
                .get(&repo_id)
                .map(|s| s.avgdl)
                .ok_or_else(|| Error::config(format!("Missing BM25 stats for {repo_id}")))?;

            let sparse_query = avgdl_sparse
                .get(&OrderedFloat(avgdl))
                .ok_or_else(|| Error::config("Sparse embedding not found".to_string()))?
                .clone();

            clients
                .qdrant
                .search_similar_hybrid(
                    dense_query.as_ref().clone(),
                    sparse_query,
                    limit,
                    filters_clone,
                    prefetch_multiplier,
                )
                .await
                .map(|results| {
                    results
                        .into_iter()
                        .map(|(entity_id, _repo_id_from_qdrant, score)| (repo_id, entity_id, score))
                        .collect::<Vec<_>>()
                })
        }
    });

    let search_results = futures::future::join_all(search_futures).await;

    let mut all_results = Vec::new();
    for result in search_results {
        all_results.extend(result?);
    }

    all_results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    Ok(all_results)
}

async fn fetch_entities(
    candidates: &[(Uuid, String, f32)],
    clients: &BackendClients,
) -> Result<Vec<CodeEntity>> {
    let entity_refs: Vec<_> = candidates
        .iter()
        .map(|(repo_id, eid, _)| (*repo_id, eid.to_string()))
        .collect();

    clients.postgres.get_entities_by_ids(&entity_refs).await
}

async fn rerank_results(
    entities: Vec<CodeEntity>,
    request: &SemanticSearchRequest,
    candidates: &[(Uuid, String, f32)],
    clients: &BackendClients,
    config: &SearchConfig,
    limit: usize,
) -> Result<(Vec<EntityResult>, bool)> {
    let rerank_config = request
        .rerank
        .as_ref()
        .map(|r| r.merge_with(&config.reranking))
        .unwrap_or_else(|| config.reranking.clone());

    let (candidates_limit, final_limit) = if rerank_config.enabled && clients.reranker.is_some() {
        (rerank_config.candidates, rerank_config.top_k.min(limit))
    } else {
        (limit, limit)
    };

    let truncated_candidates: Vec<_> = candidates.iter().take(candidates_limit).collect();

    let entities_map: HashMap<String, CodeEntity> = entities
        .into_iter()
        .map(|e| (e.entity_id.clone(), e))
        .collect();

    if let Some(ref reranker) = clients.reranker {
        if rerank_config.enabled {
            let entity_contents: Vec<(String, String)> = truncated_candidates
                .iter()
                .filter_map(|(_, entity_id, _)| {
                    entities_map
                        .get(entity_id)
                        .map(|e| (entity_id.to_string(), extract_embedding_content(e)))
                })
                .collect();

            let documents: Vec<(String, &str)> = entity_contents
                .iter()
                .map(|(id, content)| (id.clone(), content.as_str()))
                .collect();

            match reranker
                .rerank(&request.query.text, &documents, final_limit)
                .await
            {
                Ok(reranked) => {
                    let results: Vec<EntityResult> = reranked
                        .into_iter()
                        .filter_map(|(entity_id, score)| {
                            entities_map.get(&entity_id).map(|entity| {
                                let result: Result<EntityResult> = entity.clone().try_into();
                                result.map(|mut r| {
                                    r.score = score;
                                    r.reranked = true;
                                    r
                                })
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    return Ok((results, true));
                }
                Err(e) => {
                    warn!("Reranking failed: {e}, falling back to vector scores");
                }
            }
        }
    }

    let results: Vec<EntityResult> = truncated_candidates
        .iter()
        .take(final_limit)
        .filter_map(|(repo_id, entity_id, score)| {
            entities_map.get(entity_id).map(|entity| {
                let result: Result<EntityResult> = entity.clone().try_into();
                result.map(|mut r| {
                    r.repository_id = *repo_id;
                    r.score = *score;
                    r.reranked = false;
                    r
                })
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok((results, false))
}
