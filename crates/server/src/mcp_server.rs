use axum::Router;
use codesearch_core::error::{Error, ResultExt};
use codesearch_core::{config::Config, entities::EntityType, CodeEntity};
use codesearch_embeddings::EmbeddingManager;
use codesearch_indexer::entity_processor::extract_embedding_content;
use codesearch_storage::{
    create_storage_client, PostgresClientTrait, SearchFilters, StorageClient,
};
use codesearch_watcher::{FileWatcher, WatcherConfig};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, ErrorCode, ErrorData as McpError, InitializeRequestParam,
        InitializeResult, ListResourcesResult, PaginatedRequestParam, ProtocolVersion,
        ReadResourceRequestParam, ReadResourceResult, ResourceContents, ResourcesCapability,
        ServerCapabilities,
    },
    schemars::JsonSchema,
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        session::local::LocalSessionManager,
        tower::{StreamableHttpServerConfig, StreamableHttpService},
    },
    RoleServer, ServerHandler,
};
use serde::Deserialize;
use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

const DEFAULT_DEBOUNCE_MS: u64 = 500;
const MAX_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024; // 10MB

/// Information about a single indexed repository
struct RepositoryInfo {
    repository_id: Uuid,
    repository_root: PathBuf,
    collection_name: String,
    storage_client: Arc<dyn StorageClient>,
    last_indexed_commit: Option<String>,
}

/// MCP server for codesearch semantic code search
#[derive(Clone)]
struct CodeSearchMcpServer {
    repositories: Arc<RwLock<HashMap<Uuid, RepositoryInfo>>>,
    embedding_manager: Arc<EmbeddingManager>,
    postgres_client: Arc<dyn PostgresClientTrait>,
    neo4j_client: Option<Arc<dyn codesearch_storage::Neo4jClientTrait>>,
    #[allow(dead_code)]
    watchers: Arc<RwLock<HashMap<Uuid, FileWatcher>>>,
    tool_router: ToolRouter<Self>,
    default_bge_instruction: String,
    reranker: Option<Arc<dyn codesearch_embeddings::RerankerProvider>>,
    reranking_config: codesearch_core::config::RerankingConfig,
    hybrid_search_config: codesearch_core::config::HybridSearchConfig,
}

/// Request parameters for search_code tool
#[derive(Debug, Deserialize, JsonSchema)]
struct SearchCodeRequest {
    /// Semantic search query. Works best with code-like patterns (e.g., function signatures,
    /// type names, code snippets) rather than abstract descriptions.
    query: String,

    /// Custom instructions for the embedding model. If not provided, uses the default
    /// configured in embeddings.default_bge_instruction (or the hardcoded default if not set).
    /// This allows per-query customization of how the embedding model interprets the search.
    #[serde(default)]
    #[schemars(
        description = "Custom instructions for the embedding model. Defaults to code search instructions optimized for BGE models."
    )]
    embedding_instructions: Option<String>,

    /// Maximum number of results (1-100)
    #[serde(default = "default_limit")]
    limit: Option<usize>,

    /// Filter by entity type (e.g., function, method, class, struct)
    entity_type: Option<String>,

    /// Filter by programming language (currently only "rust" is supported)
    language: Option<String>,

    /// Filter by file path pattern
    file_path: Option<String>,

    /// Repository path to search (optional)
    /// If provided, searches only the repository at this path.
    /// If omitted, searches all indexed repositories.
    repository_path: Option<String>,

    // Structural filters for hybrid queries (require Neo4j)
    /// Filter to entities that implement this trait (e.g., "Display", "Clone")
    #[schemars(
        description = "Filter to code that implements this trait (Rust only, requires Neo4j)"
    )]
    implements_trait: Option<String>,

    /// Filter to entities called by this function
    #[schemars(description = "Filter to code called by this function (requires Neo4j)")]
    called_by: Option<String>,

    /// Filter to entities that call this function
    #[schemars(description = "Filter to code that calls this function (requires Neo4j)")]
    calls: Option<String>,

    /// Filter to entities within this module
    #[schemars(description = "Filter to code within this module (requires Neo4j)")]
    in_module: Option<String>,
}

fn default_limit() -> Option<usize> {
    Some(10)
}

/// Request parameters for navigate tool
#[derive(Debug, Deserialize, JsonSchema)]
struct NavigateRequest {
    /// Repository path to search
    repository_path: String,
    /// Qualified name of the entity to navigate from
    from_entity: String,
    /// Type of relationship to explore
    #[schemars(
        description = "Type of relationship: implementations, callers, calls, contains, dependencies, dependents, inherits, subclasses"
    )]
    relationship: String,
    /// Maximum number of results (default: 20)
    #[serde(default = "default_navigate_limit")]
    limit: Option<usize>,
    /// Optional semantic filter to rank results by similarity
    semantic_filter: Option<String>,
}

fn default_navigate_limit() -> Option<usize> {
    Some(20)
}

/// Request parameters for analyze tool
#[derive(Debug, Deserialize, JsonSchema)]
struct AnalyzeRequest {
    /// Repository path to analyze
    repository_path: String,
    /// Type of analysis to perform
    #[schemars(description = "Type of analysis: unused_code, circular_dependencies")]
    analysis_type: String,
}

#[tool_router]
impl CodeSearchMcpServer {
    #[tool(
        description = "Search for code using semantic similarity and optional structural filters. Supports hybrid queries combining embeddings with graph relationships."
    )]
    async fn search(
        &self,
        Parameters(request): Parameters<SearchCodeRequest>,
    ) -> std::result::Result<CallToolResult, McpError> {
        // Validate limit
        let limit = request.limit.unwrap_or(10).clamp(1, 100);

        // Check if structural filters are requested
        let has_structural_filters = request.implements_trait.is_some()
            || request.called_by.is_some()
            || request.calls.is_some()
            || request.in_module.is_some();

        // Warn if structural filters are used but Neo4j is unavailable
        if has_structural_filters && self.neo4j_client.is_none() {
            return Err(McpError::new(
                ErrorCode::INVALID_PARAMS,
                "Structural filters (implements_trait, called_by, calls, in_module) require Neo4j, which is not available.".to_string(),
                None,
            ));
        }

        // Execute structural filters if present
        let structural_qualified_names: Option<std::collections::HashSet<String>> =
            if has_structural_filters {
                // Already validated above at line 184 that neo4j is Some
                let Some(ref neo4j) = self.neo4j_client else {
                    return Err(McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        "Neo4j client unexpectedly unavailable".to_string(),
                        None,
                    ));
                };

                // Structural filters require single repository (can't filter across repos)
                let (target_repo_id, _) = if let Some(ref repo_path) = request.repository_path {
                    let path_buf = PathBuf::from(repo_path);
                    self.postgres_client
                        .get_repository_by_path(&path_buf)
                        .await
                        .map_err(|e| {
                            McpError::new(
                                ErrorCode::INTERNAL_ERROR,
                                format!("Failed to look up repository: {e}"),
                                None,
                            )
                        })?
                        .ok_or_else(|| {
                            McpError::new(
                                ErrorCode::INVALID_PARAMS,
                                format!("No indexed repository found at path '{repo_path}'"),
                                None,
                            )
                        })?
                } else {
                    return Err(McpError::new(
                        ErrorCode::INVALID_PARAMS,
                        "Structural filters (implements_trait, called_by, calls, in_module) require specifying a single repository via repository_path parameter.".to_string(),
                        None,
                    ));
                };

                // Run structural queries and intersect results (AND logic)
                let mut qualified_names: Option<std::collections::HashSet<String>> = None;

                // Filter: implements_trait
                if let Some(ref trait_name) = request.implements_trait {
                    let trait_impls = crate::graph_queries::find_trait_implementations(
                        neo4j,
                        &self.postgres_client,
                        target_repo_id,
                        trait_name,
                    )
                    .await
                    .map_err(|e| {
                        tracing::warn!(
                            "Trait implementation query failed: {e}, falling back to semantic-only search"
                        );
                        e
                    })
                    .ok();

                    if let Some(impls) = trait_impls {
                        let impls_set: std::collections::HashSet<String> =
                            impls.into_iter().collect();
                        qualified_names = Some(match qualified_names {
                            None => impls_set,
                            Some(existing) => existing.intersection(&impls_set).cloned().collect(),
                        });
                    } else {
                        tracing::warn!(
                            "Structural filter failed, continuing with semantic-only search"
                        );
                    }
                }

                // Filter: in_module
                if let Some(ref module_name) = request.in_module {
                    let module_functions = crate::graph_queries::find_functions_in_module(
                        neo4j,
                        &self.postgres_client,
                        target_repo_id,
                        module_name,
                    )
                    .await
                    .map_err(|e| {
                        tracing::warn!(
                            "Module functions query failed: {e}, falling back to semantic-only search"
                        );
                        e
                    })
                    .ok();

                    if let Some(functions) = module_functions {
                        let functions_set: std::collections::HashSet<String> =
                            functions.into_iter().collect();
                        qualified_names = Some(match qualified_names {
                            None => functions_set,
                            Some(existing) => {
                                existing.intersection(&functions_set).cloned().collect()
                            }
                        });
                    } else {
                        tracing::warn!(
                            "Structural filter failed, continuing with semantic-only search"
                        );
                    }
                }

                // Filter: calls (entities that call this function)
                if let Some(ref function_name) = request.calls {
                    let callers = crate::graph_queries::find_function_callers(
                        neo4j,
                        &self.postgres_client,
                        target_repo_id,
                        function_name,
                        3, // Default max depth
                    )
                    .await
                    .map_err(|e| {
                        tracing::warn!(
                            "Function callers query failed: {e}, falling back to semantic-only search"
                        );
                        e
                    })
                    .ok();

                    if let Some(caller_list) = callers {
                        let callers_set: std::collections::HashSet<String> =
                            caller_list.into_iter().map(|(name, _depth)| name).collect();
                        qualified_names = Some(match qualified_names {
                            None => callers_set,
                            Some(existing) => {
                                existing.intersection(&callers_set).cloned().collect()
                            }
                        });
                    } else {
                        tracing::warn!(
                            "Structural filter failed, continuing with semantic-only search"
                        );
                    }
                }

                // Filter: called_by (find callees - what this function calls)
                if let Some(ref function_name) = request.called_by {
                    // Query for functions that are called BY the specified function
                    // Using forward CALLS edges: (function)-[:CALLS]->(callee)
                    let db_name = neo4j
                        .ensure_repository_database(target_repo_id, self.postgres_client.as_ref())
                        .await
                        .map_err(|e| {
                            McpError::new(
                                ErrorCode::INTERNAL_ERROR,
                                format!("Failed to ensure Neo4j database: {e}"),
                                None,
                            )
                        })?;
                    neo4j.use_database(&db_name).await.map_err(|e| {
                        McpError::new(
                            ErrorCode::INTERNAL_ERROR,
                            format!("Failed to use Neo4j database: {e}"),
                            None,
                        )
                    })?;

                    let callees_result = neo4j
                        .find_function_callees(
                            &self.postgres_client,
                            target_repo_id,
                            function_name,
                            3,
                        )
                        .await;

                    match callees_result {
                        Ok(callee_list) => {
                            let callees_set: std::collections::HashSet<String> =
                                callee_list.into_iter().map(|(name, _depth)| name).collect();
                            qualified_names = Some(match qualified_names {
                                None => callees_set,
                                Some(existing) => {
                                    existing.intersection(&callees_set).cloned().collect()
                                }
                            });
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Function callees query failed: {e}, falling back to semantic-only search"
                            );
                        }
                    }
                }

                // Check if intersection resulted in empty set
                if let Some(ref names) = qualified_names {
                    if names.is_empty() {
                        // No entities match all structural filters
                        return Ok(CallToolResult::success(vec![Content::text(
                            serde_json::to_string_pretty(&serde_json::json!({
                                "results": [],
                                "total": 0,
                                "query": request.query,
                                "repositories_searched": 1,
                                "message": "No entities match all structural filters",
                                "reranked": false,
                            }))
                            .map_err(|e| {
                                McpError::new(
                                    ErrorCode::INTERNAL_ERROR,
                                    format!("Failed to serialize response: {e}"),
                                    None,
                                )
                            })?,
                        )]));
                    }
                }

                qualified_names
            } else {
                None
            };

        // Determine which repositories to search
        let repos = self.repositories.read().await;

        let target_repos: Vec<(Uuid, &RepositoryInfo)> = if let Some(ref repo_path) =
            request.repository_path
        {
            // Search specific repository by path
            let path_buf = PathBuf::from(repo_path);

            // Look up repository by path
            let repo_lookup = self
                .postgres_client
                .get_repository_by_path(&path_buf)
                .await
                .map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to look up repository: {e}"),
                        None,
                    )
                })?;

            match repo_lookup {
                Some((repo_id, _)) => {
                    if let Some(repo_info) = repos.get(&repo_id) {
                        vec![(repo_id, repo_info)]
                    } else {
                        return Err(McpError::new(
                            ErrorCode::INVALID_PARAMS,
                            format!("Repository at '{repo_path}' is not currently being served"),
                            None,
                        ));
                    }
                }
                None => {
                    return Err(McpError::new(
                        ErrorCode::INVALID_PARAMS,
                        format!("No indexed repository found at path '{repo_path}'"),
                        None,
                    ));
                }
            }
        } else {
            // Search all repositories
            repos.iter().map(|(id, info)| (*id, info)).collect()
        };

        if target_repos.is_empty() {
            return Err(McpError::new(
                ErrorCode::INTERNAL_ERROR,
                "No repositories available to search".to_string(),
                None,
            ));
        }

        // Extract query
        let query_text = request.query;

        // Use client-provided instruction if present, otherwise use configured default
        let bge_instruction = request
            .embedding_instructions
            .unwrap_or_else(|| self.default_bge_instruction.clone());

        let formatted_query = format!("<instruct>{bge_instruction}\n<query>{query_text}");

        // Generate query embedding
        let embeddings = self
            .embedding_manager
            .embed(vec![formatted_query])
            .await
            .map_err(|e| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to generate embedding: {e}"),
                    None,
                )
            })?;

        let dense_query_embedding = embeddings
            .into_iter()
            .next()
            .ok_or_else(|| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    "No embedding returned from provider".to_string(),
                    None,
                )
            })?
            .ok_or_else(|| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    "Embedding provider returned None".to_string(),
                    None,
                )
            })?;

        // Generate sparse embeddings grouped by repository avgdl
        // This batches repositories with the same avgdl to minimize sparse embedding calls
        use ordered_float::OrderedFloat;
        let mut avgdl_to_repos: HashMap<OrderedFloat<f32>, Vec<(Uuid, &RepositoryInfo)>> =
            HashMap::new();

        // Batch fetch BM25 statistics for all repositories in a single query
        let repo_ids: Vec<Uuid> = target_repos.iter().map(|(id, _)| *id).collect();
        let stats_batch = self
            .postgres_client
            .get_bm25_statistics_batch(&repo_ids)
            .await
            .map_err(|e| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to get batch BM25 statistics: {e}"),
                    None,
                )
            })?;

        for (repo_id, repo_info) in &target_repos {
            let stats = stats_batch.get(repo_id).ok_or_else(|| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Missing BM25 statistics for repository {repo_id}"),
                    None,
                )
            })?;

            avgdl_to_repos
                .entry(OrderedFloat(stats.avgdl))
                .or_default()
                .push((*repo_id, *repo_info));
        }

        // Generate sparse query embedding once per unique avgdl value
        // Note: Use raw query text WITHOUT BGE instruction prefix for sparse embeddings
        let mut avgdl_to_sparse_embedding: HashMap<OrderedFloat<f32>, Vec<(u32, f32)>> =
            HashMap::new();

        for avgdl in avgdl_to_repos.keys() {
            let sparse_manager =
                codesearch_embeddings::create_sparse_manager(avgdl.0).map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to create sparse manager: {e}"),
                        None,
                    )
                })?;

            let sparse_embeddings = sparse_manager
                .embed_sparse(vec![query_text.as_str()])
                .await
                .map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to generate sparse embedding: {e}"),
                        None,
                    )
                })?;

            let sparse_embedding =
                sparse_embeddings
                    .into_iter()
                    .next()
                    .flatten()
                    .ok_or_else(|| {
                        McpError::new(
                            ErrorCode::INTERNAL_ERROR,
                            "Failed to generate sparse embedding".to_string(),
                            None,
                        )
                    })?;

            avgdl_to_sparse_embedding.insert(*avgdl, sparse_embedding);
        }

        // Parse entity type filter
        let entity_type = request
            .entity_type
            .as_ref()
            .and_then(|s| EntityType::try_from(s.as_str()).ok());

        // Build filters
        let filters = SearchFilters {
            entity_type,
            language: request.language.clone(),
            file_path: request.file_path.as_ref().map(PathBuf::from),
        };

        // Execute hybrid search using Reciprocal Rank Fusion (RRF)
        //
        // Hybrid search combines two retrieval methods for improved accuracy:
        // 1. BM25 sparse embeddings - Traditional keyword-based search using term frequency
        //    and inverse document frequency. Good for exact keyword matches.
        // 2. Dense vector embeddings - Semantic search using learned embeddings from the
        //    BGE model. Good for conceptual similarity.
        //
        // The Reciprocal Rank Fusion algorithm merges results from both methods:
        // - Each method retrieves (prefetch_multiplier × limit) candidates
        // - RRF assigns scores based on rank: score = 1 / (k + rank) where k=60
        // - Final ranking combines scores from both methods
        //
        // The prefetch_multiplier parameter controls the trade-off:
        // - Higher values (10-20): Better recall, captures more candidates, slower
        // - Lower values (3-5): Faster queries, may miss some relevant results
        // - Default (5): Balanced trade-off between quality and latency
        //
        // For more on RRF: https://plg.uwaterloo.ca/~gvcormac/cormacksigir09-rrf.pdf

        // Wrap shared data in Arc to avoid expensive clones
        let dense_query_emb_arc = std::sync::Arc::new(dense_query_embedding);
        let avgdl_to_sparse_arc = std::sync::Arc::new(avgdl_to_sparse_embedding);
        let stats_batch_arc = std::sync::Arc::new(stats_batch);

        let search_futures = target_repos.iter().map(|(repo_id, repo_info)| {
            let storage_client = repo_info.storage_client.clone();
            let dense_query_emb_arc = std::sync::Arc::clone(&dense_query_emb_arc);
            let filters_clone = filters.clone();
            let collection_name = repo_info.collection_name.clone();
            let repo_id = *repo_id;
            let avgdl_to_sparse_arc = std::sync::Arc::clone(&avgdl_to_sparse_arc);
            let prefetch_multiplier = self.hybrid_search_config.prefetch_multiplier;
            let stats_batch_arc = std::sync::Arc::clone(&stats_batch_arc);

            async move {
                // Get avgdl from batch results (already fetched above)
                let avgdl = stats_batch_arc
                    .get(&repo_id)
                    .map(|s| s.avgdl)
                    .ok_or_else(|| {
                        McpError::new(
                            ErrorCode::INTERNAL_ERROR,
                            format!("Missing BM25 statistics for repository {repo_id}"),
                            None,
                        )
                    })?;

                // Look up the correct sparse embedding using the repository's avgdl
                let sparse_query_emb = avgdl_to_sparse_arc
                    .get(&OrderedFloat(avgdl))
                    .ok_or_else(|| {
                        McpError::new(
                            ErrorCode::INTERNAL_ERROR,
                            "Sparse embedding not found for repository avgdl".to_string(),
                            None,
                        )
                    })?
                    .clone();

                storage_client
                    .search_similar_hybrid(
                        dense_query_emb_arc.as_ref().clone(),
                        sparse_query_emb,
                        limit,
                        Some(filters_clone),
                        prefetch_multiplier,
                    )
                    .await
                    .map(|results| {
                        results
                            .into_iter()
                            .map(|(entity_id, _repo_id_from_qdrant, score)| {
                                (repo_id, entity_id, score)
                            })
                            .collect::<Vec<_>>()
                    })
                    .map_err(|e| {
                        McpError::new(
                            ErrorCode::INTERNAL_ERROR,
                            format!("Hybrid search failed in repository {collection_name}: {e}"),
                            None,
                        )
                    })
            }
        });

        let search_results = futures::future::join_all(search_futures).await;

        // Collect all results, failing if any search failed
        let mut all_results = Vec::new();
        for result in search_results {
            match result {
                Ok(repo_results) => all_results.extend(repo_results),
                Err(e) => return Err(e),
            }
        }

        // Sort by score and determine candidate limit based on reranking config
        all_results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        let (candidates_limit, final_limit) =
            if self.reranking_config.enabled && self.reranker.is_some() {
                (
                    self.reranking_config.candidates,
                    self.reranking_config.top_k.min(limit),
                )
            } else {
                (limit, limit)
            };
        all_results.truncate(candidates_limit);

        // Batch fetch entities from Postgres for all candidates
        let entity_refs: Vec<_> = all_results
            .iter()
            .map(|(repo_id, eid, _)| (*repo_id, eid.to_string()))
            .collect();

        let entities_vec = self
            .postgres_client
            .get_entities_by_ids(&entity_refs)
            .await
            .map_err(|e| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to fetch entities: {e}"),
                    None,
                )
            })?;

        // Apply structural filtering if present
        let entities_vec: Vec<CodeEntity> =
            if let Some(ref allowed_names) = structural_qualified_names {
                entities_vec
                    .into_iter()
                    .filter(|entity| allowed_names.contains(&entity.qualified_name))
                    .collect()
            } else {
                entities_vec
            };

        // If filtering resulted in zero entities, return early
        if entities_vec.is_empty() && structural_qualified_names.is_some() {
            return Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&serde_json::json!({
                    "results": [],
                    "total": 0,
                    "query": query_text,
                    "repositories_searched": target_repos.len(),
                    "message": "Semantic search found results, but none matched structural filters",
                    "reranked": false,
                }))
                .map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to serialize response: {e}"),
                        None,
                    )
                })?,
            )]));
        }

        // Rerank if enabled, otherwise use vector scores
        let (final_results, reranked) = if let Some(ref reranker) = self.reranker {
            // Build documents for reranking
            let entity_contents: Vec<(String, String)> = entities_vec
                .iter()
                .map(|entity| (entity.entity_id.clone(), extract_embedding_content(entity)))
                .collect();

            let documents: Vec<(String, &str)> = entity_contents
                .iter()
                .map(|(id, content)| (id.clone(), content.as_str()))
                .collect();

            // Build HashMap for O(1) lookups instead of O(n) linear search
            let entity_to_repo: HashMap<&str, Uuid> = all_results
                .iter()
                .map(|(repo_id, entity_id, _)| (entity_id.as_str(), *repo_id))
                .collect();

            // Attempt reranking with fallback
            match reranker.rerank(&query_text, &documents, final_limit).await {
                Ok(reranked) => {
                    // Map reranked entity IDs back to (repo_id, entity_id, score) tuples
                    let results = reranked
                        .into_iter()
                        .filter_map(|(entity_id, score)| {
                            entity_to_repo
                                .get(entity_id.as_str())
                                .map(|repo_id| (*repo_id, entity_id, score))
                        })
                        .collect::<Vec<_>>();
                    (results, true)
                }
                Err(e) => {
                    // Log warning and fall back to vector search scores
                    tracing::warn!("Reranking failed: {e}, falling back to vector search scores");
                    (all_results.into_iter().take(final_limit).collect(), false)
                }
            }
        } else {
            // Reranking disabled, use vector search scores
            (all_results.into_iter().take(final_limit).collect(), false)
        };

        // Build entity lookup map
        let entities_map: HashMap<String, CodeEntity> = entities_vec
            .into_iter()
            .map(|entity| (entity.entity_id.clone(), entity))
            .collect();

        // Format results with repository information
        let formatted_results: Vec<_> = final_results
            .into_iter()
            .filter_map(|(repo_id, entity_id, score)| {
                match entities_map.get(&entity_id) {
                    Some(entity) => repos.get(&repo_id).map(|repo| {
                        serde_json::json!({
                            "repository_id": repo_id.to_string(),
                            "repository_path": repo.repository_root.display().to_string(),
                            "collection_name": repo.collection_name,
                            "entity_id": entity_id,
                            "similarity_percent": (score * 100.0).round() as i32,
                            "name": entity.name,
                            "qualified_name": entity.qualified_name,
                            "entity_type": format!("{:?}", entity.entity_type),
                            "language": format!("{:?}", entity.language),
                            "file_path": entity.file_path.display().to_string(),
                            "line_range": {
                                "start": entity.location.start_line,
                                "end": entity.location.end_line,
                            },
                            "content": entity.content,
                            "documentation_summary": entity.documentation_summary,
                            "signature": entity.signature.as_ref().map(|s| format!("{s:?}")),
                            "visibility": format!("{:?}", entity.visibility),
                        })
                    }),
                    None => {
                        tracing::warn!(
                            "Entity '{}' from Qdrant not found in Postgres (data consistency issue)",
                            entity_id
                        );
                        None
                    }
                }
            })
            .collect();

        let response = serde_json::json!({
            "results": formatted_results,
            "total": formatted_results.len(),
            "query": query_text,
            "repositories_searched": target_repos.len(),
            "reranked": reranked,
        });

        let response_str = serde_json::to_string_pretty(&response).map_err(|e| {
            McpError::new(
                ErrorCode::INTERNAL_ERROR,
                format!("Failed to serialize response: {e}"),
                None,
            )
        })?;

        Ok(CallToolResult::success(vec![Content::text(response_str)]))
    }

    #[tool(
        description = "Navigate code relationships from a known entity. Explore implementations, callers, dependencies, and more."
    )]
    async fn navigate(
        &self,
        Parameters(request): Parameters<NavigateRequest>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let neo4j = self.neo4j_client.as_ref().ok_or_else(|| {
            McpError::new(
                ErrorCode::INTERNAL_ERROR,
                "Neo4j graph database is not available. Graph navigation requires Neo4j."
                    .to_string(),
                None,
            )
        })?;

        // Get repository
        let path_buf = PathBuf::from(&request.repository_path);
        let (repo_id, _) = self
            .postgres_client
            .get_repository_by_path(&path_buf)
            .await
            .map_err(|e| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to look up repository: {e}"),
                    None,
                )
            })?
            .ok_or_else(|| {
                McpError::new(
                    ErrorCode::INVALID_PARAMS,
                    format!(
                        "No indexed repository found at path '{}'",
                        request.repository_path
                    ),
                    None,
                )
            })?;

        // Check graph_ready flag
        let is_ready = self
            .postgres_client
            .is_graph_ready(repo_id)
            .await
            .map_err(|e| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to check graph_ready status: {e}"),
                    None,
                )
            })?;

        let warning = if !is_ready {
            Some(
                "Warning: Graph is incomplete (indexing in progress). Results may be partial."
                    .to_string(),
            )
        } else {
            None
        };

        let limit = request.limit.unwrap_or(20).clamp(1, 100);

        // Route to appropriate graph query based on relationship type
        let results: serde_json::Value = match request.relationship.as_str() {
            "implementations" => {
                let impls = crate::graph_queries::find_trait_implementations(
                    neo4j,
                    &self.postgres_client,
                    repo_id,
                    &request.from_entity,
                )
                .await
                .map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Graph query failed: {e}"),
                        None,
                    )
                })?;
                serde_json::json!(impls)
            }
            "callers" => {
                let callers = crate::graph_queries::find_function_callers(
                    neo4j,
                    &self.postgres_client,
                    repo_id,
                    &request.from_entity,
                    3, // Default max depth
                )
                .await
                .map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Graph query failed: {e}"),
                        None,
                    )
                })?;
                serde_json::json!(callers)
            }
            "contains" => {
                let children = crate::graph_queries::find_functions_in_module(
                    neo4j,
                    &self.postgres_client,
                    repo_id,
                    &request.from_entity,
                )
                .await
                .map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Graph query failed: {e}"),
                        None,
                    )
                })?;
                serde_json::json!(children)
            }
            "dependencies" => {
                let deps = crate::graph_queries::find_module_dependencies(
                    neo4j,
                    &self.postgres_client,
                    repo_id,
                    &request.from_entity,
                )
                .await
                .map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Graph query failed: {e}"),
                        None,
                    )
                })?;
                serde_json::json!(deps)
            }
            "inherits" => {
                let hierarchy = crate::graph_queries::find_class_hierarchy(
                    neo4j,
                    &self.postgres_client,
                    repo_id,
                    &request.from_entity,
                )
                .await
                .map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Graph query failed: {e}"),
                        None,
                    )
                })?;
                serde_json::json!(hierarchy)
            }
            _ => {
                return Err(McpError::new(
                    ErrorCode::INVALID_PARAMS,
                    format!("Unknown relationship type '{}'. Valid types: implementations, callers, calls, contains, dependencies, dependents, inherits, subclasses", request.relationship),
                    None,
                ));
            }
        };

        // Apply semantic filtering if requested
        let results = if let Some(ref semantic_filter) = request.semantic_filter {
            // Extract qualified names from graph results
            let qualified_names: Vec<String> = match &results {
                serde_json::Value::Array(items) => items
                    .iter()
                    .filter_map(|v| match v {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Object(obj) => obj
                            .get("qualified_name")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        _ => None,
                    })
                    .collect(),
                _ => vec![],
            };

            if qualified_names.is_empty() {
                results
            } else {
                // Fetch full entities for reranking
                let entities = self
                    .postgres_client
                    .get_entities_by_qualified_names(repo_id, &qualified_names)
                    .await
                    .map_err(|e| {
                        tracing::warn!(
                            "Failed to fetch entities: {e}, returning unfiltered results"
                        );
                        e
                    })
                    .ok();

                match entities {
                    Some(entities_map) if !entities_map.is_empty() => {
                        // Use reranker if available, otherwise return unscored results
                        if let Some(ref reranker) = self.reranker {
                            // Build documents for reranking
                            let entity_contents: Vec<(String, String)> = qualified_names
                                .iter()
                                .filter_map(|qname| {
                                    entities_map.get(qname).map(|entity| {
                                        (qname.clone(), extract_embedding_content(entity))
                                    })
                                })
                                .collect();

                            let documents: Vec<(String, &str)> = entity_contents
                                .iter()
                                .map(|(qname, content)| (qname.clone(), content.as_str()))
                                .collect();

                            // Attempt reranking with fallback
                            match reranker.rerank(semantic_filter, &documents, limit).await {
                                Ok(reranked) => {
                                    let filtered_results: Vec<_> = reranked
                                        .into_iter()
                                        .map(|(qname, score)| {
                                            serde_json::json!({
                                                "qualified_name": qname,
                                                "relevance_score": (score * 100.0).round() as i32,
                                            })
                                        })
                                        .collect();

                                    serde_json::json!(filtered_results)
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Reranking failed: {e}, returning unscored results"
                                    );
                                    // Fall back to unscored results
                                    let unscored: Vec<_> = qualified_names
                                        .into_iter()
                                        .take(limit)
                                        .map(|qname| {
                                            serde_json::json!({
                                                "qualified_name": qname,
                                            })
                                        })
                                        .collect();
                                    serde_json::json!(unscored)
                                }
                            }
                        } else {
                            // No reranker configured, return unscored results
                            let unscored: Vec<_> = qualified_names
                                .into_iter()
                                .take(limit)
                                .map(|qname| {
                                    serde_json::json!({
                                        "qualified_name": qname,
                                    })
                                })
                                .collect();
                            serde_json::json!(unscored)
                        }
                    }
                    _ => {
                        // Failed to fetch entities - return unfiltered
                        tracing::warn!(
                            "Could not fetch entities for semantic filtering, returning unfiltered results"
                        );
                        match results {
                            serde_json::Value::Array(mut items) => {
                                items.truncate(limit);
                                serde_json::json!(items)
                            }
                            other => other,
                        }
                    }
                }
            }
        } else {
            // No semantic filter - apply limit only
            match results {
                serde_json::Value::Array(mut items) => {
                    items.truncate(limit);
                    serde_json::json!(items)
                }
                other => other,
            }
        };

        let response = serde_json::json!({
            "results": results,
            "relationship": request.relationship,
            "from_entity": request.from_entity,
            "warning": warning,
        });

        let response_str = serde_json::to_string_pretty(&response).map_err(|e| {
            McpError::new(
                ErrorCode::INTERNAL_ERROR,
                format!("Failed to serialize response: {e}"),
                None,
            )
        })?;

        Ok(CallToolResult::success(vec![Content::text(response_str)]))
    }

    #[tool(
        description = "Analyze code quality and health. Detect unused code, circular dependencies, and other code smells."
    )]
    async fn analyze(
        &self,
        Parameters(request): Parameters<AnalyzeRequest>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let neo4j = self.neo4j_client.as_ref().ok_or_else(|| {
            McpError::new(
                ErrorCode::INTERNAL_ERROR,
                "Code analysis requires Neo4j graph database, which is not available.".to_string(),
                None,
            )
        })?;

        // Get repository
        let path_buf = PathBuf::from(&request.repository_path);
        let (repo_id, _) = self
            .postgres_client
            .get_repository_by_path(&path_buf)
            .await
            .map_err(|e| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to look up repository: {e}"),
                    None,
                )
            })?
            .ok_or_else(|| {
                McpError::new(
                    ErrorCode::INVALID_PARAMS,
                    format!(
                        "No indexed repository found at path '{}'",
                        request.repository_path
                    ),
                    None,
                )
            })?;

        // Check graph_ready flag
        let is_ready = self
            .postgres_client
            .is_graph_ready(repo_id)
            .await
            .map_err(|e| {
                McpError::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to check graph_ready status: {e}"),
                    None,
                )
            })?;

        let warning = if !is_ready {
            Some(
                "Warning: Graph is incomplete (indexing in progress). Analysis may be partial."
                    .to_string(),
            )
        } else {
            None
        };

        // Route to appropriate analysis based on type
        let results: serde_json::Value = match request.analysis_type.as_str() {
            "unused_code" => {
                let unused = crate::graph_queries::find_unused_functions(
                    neo4j,
                    &self.postgres_client,
                    repo_id,
                )
                .await
                .map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Analysis failed: {e}"),
                        None,
                    )
                })?;
                serde_json::json!({
                    "unused_functions": unused,
                    "count": unused.len()
                })
            }
            "circular_dependencies" => {
                let cycles = crate::graph_queries::find_circular_dependencies(
                    neo4j,
                    &self.postgres_client,
                    repo_id,
                )
                .await
                .map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Analysis failed: {e}"),
                        None,
                    )
                })?;
                serde_json::json!({
                    "cycles": cycles,
                    "count": cycles.len()
                })
            }
            _ => {
                return Err(McpError::new(
                    ErrorCode::INVALID_PARAMS,
                    format!("Unknown analysis type '{}'. Valid types: unused_code, circular_dependencies", request.analysis_type),
                    None,
                ));
            }
        };

        let response = serde_json::json!({
            "analysis_type": request.analysis_type,
            "results": results,
            "warning": warning,
        });

        let response_str = serde_json::to_string_pretty(&response).map_err(|e| {
            McpError::new(
                ErrorCode::INTERNAL_ERROR,
                format!("Failed to serialize response: {e}"),
                None,
            )
        })?;

        Ok(CallToolResult::success(vec![Content::text(response_str)]))
    }

    #[tool(description = "List all indexed repositories available for search")]
    async fn list_repositories(&self) -> std::result::Result<CallToolResult, McpError> {
        let repos = self.repositories.read().await;

        let repo_list: Vec<_> = repos
            .values()
            .map(|repo| {
                serde_json::json!({
                    "repository_id": repo.repository_id.to_string(),
                    "repository_name": repo.repository_root
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown"),
                    "repository_path": repo.repository_root.display().to_string(),
                    "collection_name": repo.collection_name,
                    "last_indexed_commit": repo.last_indexed_commit,
                })
            })
            .collect();

        let response = serde_json::json!({
            "repositories": repo_list,
            "total": repo_list.len(),
        });

        let response_str = serde_json::to_string_pretty(&response).map_err(|e| {
            McpError::new(
                ErrorCode::INTERNAL_ERROR,
                format!("Failed to serialize response: {e}"),
                None,
            )
        })?;

        Ok(CallToolResult::success(vec![Content::text(response_str)]))
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        repositories: Arc<RwLock<HashMap<Uuid, RepositoryInfo>>>,
        embedding_manager: Arc<EmbeddingManager>,
        postgres_client: Arc<dyn PostgresClientTrait>,
        neo4j_client: Option<Arc<dyn codesearch_storage::Neo4jClientTrait>>,
        watchers: Arc<RwLock<HashMap<Uuid, FileWatcher>>>,
        default_bge_instruction: String,
        reranker: Option<Arc<dyn codesearch_embeddings::RerankerProvider>>,
        reranking_config: codesearch_core::config::RerankingConfig,
        hybrid_search_config: codesearch_core::config::HybridSearchConfig,
    ) -> Self {
        Self {
            repositories,
            embedding_manager,
            postgres_client,
            neo4j_client,
            watchers,
            tool_router: Self::tool_router(),
            default_bge_instruction,
            reranker,
            reranking_config,
            hybrid_search_config,
        }
    }
}

#[tool_handler]
impl ServerHandler for CodeSearchMcpServer {
    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<InitializeResult, rmcp::model::ErrorData> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities {
                tools: Some(rmcp::model::ToolsCapability { list_changed: None }),
                resources: Some(ResourcesCapability {
                    subscribe: None,
                    list_changed: None,
                }),
                ..Default::default()
            },
            server_info: rmcp::model::Implementation {
                name: "codesearch".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                website_url: None,
                icons: None,
            },
            ..Default::default()
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![rmcp::model::Annotated::new(
                rmcp::model::RawResource {
                    uri: "codesearch://repositories/info".to_string(),
                    name: "Repositories Information".to_string(),
                    title: None,
                    description: Some("All indexed repositories and their metadata".to_string()),
                    mime_type: Some("application/json".to_string()),
                    size: None,
                    icons: None,
                },
                None,
            )],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResult, McpError> {
        let contents = match request.uri.as_str() {
            "codesearch://repositories/info" => {
                let repos = self.repositories.read().await;

                let repo_list: Vec<_> = repos
                    .values()
                    .map(|repo| {
                        serde_json::json!({
                            "repository_id": repo.repository_id.to_string(),
                            "repository_root": repo.repository_root.display().to_string(),
                            "collection_name": repo.collection_name,
                            "last_indexed_commit": repo.last_indexed_commit,
                        })
                    })
                    .collect();

                let info = serde_json::json!({
                    "repositories": repo_list,
                    "total": repo_list.len(),
                    "languages_supported": ["rust"],
                });

                let text = serde_json::to_string_pretty(&info).map_err(|e| {
                    McpError::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to serialize resource: {e}"),
                        None,
                    )
                })?;

                vec![ResourceContents::TextResourceContents {
                    uri: request.uri.clone(),
                    mime_type: Some("application/json".to_string()),
                    text,
                    meta: None,
                }]
            }

            _ => {
                return Err(McpError::new(
                    ErrorCode::INVALID_PARAMS,
                    format!("Unknown resource URI: {}", request.uri),
                    None,
                ))
            }
        };

        Ok(ReadResourceResult { contents })
    }
}

/// Run multi-repository MCP server
pub(crate) async fn run_multi_repo_server(
    config: Config,
    all_repos: Vec<(Uuid, String, PathBuf)>,
    postgres_client: Arc<dyn PostgresClientTrait>,
) -> std::result::Result<(), codesearch_core::Error> {
    info!("Initializing multi-repository MCP server...");

    let embedding_manager = crate::storage_init::create_embedding_manager(&config).await?;

    // Initialize reranker if enabled
    let reranker: Option<Arc<dyn codesearch_embeddings::RerankerProvider>> =
        if config.reranking.enabled {
            let api_base_url = config
                .reranking
                .api_base_url
                .clone()
                .or_else(|| config.embeddings.api_base_url.clone())
                .unwrap_or_else(|| "http://localhost:8000/v1".to_string());

            match codesearch_embeddings::create_reranker_provider(
                config.reranking.model.clone(),
                api_base_url,
                config.reranking.timeout_secs,
            )
            .await
            {
                Ok(provider) => {
                    info!("Reranker initialized successfully");
                    Some(provider)
                }
                Err(e) => {
                    tracing::warn!("Failed to initialize reranker: {e}");
                    tracing::warn!("Reranking will be disabled for this session");
                    None
                }
            }
        } else {
            None
        };

    // Parallelize repository loading (including collection existence checks)
    let repo_load_futures =
        all_repos
            .into_iter()
            .map(|(repository_id, collection_name, repo_path)| {
                let storage_config = config.storage.clone();
                let postgres_client = postgres_client.clone();
                async move {
                    let storage_client = create_storage_client(&storage_config, &collection_name)
                        .await
                        .context("Failed to create storage client")?;

                    let last_indexed_commit = postgres_client
                        .get_last_indexed_commit(repository_id)
                        .await
                        .context("Failed to get last indexed commit")?;

                    info!(
                        "Loaded repository: {} ({}) at {}",
                        collection_name,
                        repository_id,
                        repo_path.display()
                    );

                    Ok::<_, codesearch_core::Error>((
                        repository_id,
                        RepositoryInfo {
                            repository_id,
                            repository_root: repo_path,
                            collection_name,
                            storage_client,
                            last_indexed_commit,
                        },
                    ))
                }
            });

    let loaded_repos = futures::future::join_all(repo_load_futures).await;

    // Collect successfully loaded repositories
    let mut repositories = HashMap::new();
    for result in loaded_repos {
        match result {
            Ok((repo_id, repo_info)) => {
                repositories.insert(repo_id, repo_info);
            }
            Err(e) => {
                tracing::error!("Failed to load repository: {e}");
            }
        }
    }

    if repositories.is_empty() {
        return Err(Error::config(
            "No valid repositories found to serve.\n\
            Run 'codesearch index' from a git repository to create an index."
                .to_string(),
        ));
    }

    let repositories = Arc::new(RwLock::new(repositories));

    info!("Starting file watchers for all repositories...");
    let watchers = start_all_watchers(
        &repositories,
        embedding_manager.clone(),
        postgres_client.clone(),
    )
    .await?;

    info!("All watchers started successfully");

    // Initialize Neo4j client if enabled
    let neo4j_client = match codesearch_storage::create_neo4j_client(&config.storage).await {
        Ok(client) => {
            info!("Neo4j client initialized successfully");
            Some(client)
        }
        Err(e) => {
            tracing::warn!("Failed to initialize Neo4j client: {e}");
            tracing::warn!("Graph queries will be disabled for this session");
            None
        }
    };

    let mcp_server = CodeSearchMcpServer::new(
        repositories,
        embedding_manager,
        postgres_client,
        neo4j_client,
        watchers.clone(),
        config.embeddings.default_bge_instruction.clone(),
        reranker,
        config.reranking.clone(),
        config.hybrid_search.clone(),
    );

    run_mcp_server_with_shutdown_multi(mcp_server, watchers, config.server.port).await
}

/// Start file watchers for all repositories
async fn start_all_watchers(
    repositories: &Arc<RwLock<HashMap<Uuid, RepositoryInfo>>>,
    embedding_manager: Arc<EmbeddingManager>,
    postgres_client: Arc<dyn PostgresClientTrait>,
) -> std::result::Result<Arc<RwLock<HashMap<Uuid, FileWatcher>>>, codesearch_core::Error> {
    let repos = repositories.read().await;

    // Parallelize watcher initialization
    let watcher_futures = repos.iter().map(|(repo_id, repo_info)| {
        let repo_id = *repo_id;
        let repo_root = repo_info.repository_root.clone();
        let embedding_manager = embedding_manager.clone();
        let postgres_client = postgres_client.clone();

        async move {
            info!("Setting up watcher for {}", repo_root.display());

            // Run catch-up indexing if git repository exists
            if let Ok(git_repo) = codesearch_watcher::GitRepository::open(&repo_root) {
                info!("Running catch-up indexing for {}", repo_root.display());
                codesearch_indexer::catch_up_from_git(
                    &repo_root,
                    repo_id,
                    &postgres_client,
                    &embedding_manager,
                    &git_repo,
                )
                .await
                .context(format!(
                    "Catch-up indexing failed for {}",
                    repo_root.display()
                ))?;
            }

            let watcher_config = WatcherConfig::builder()
                .debounce_ms(DEFAULT_DEBOUNCE_MS)
                .max_file_size(MAX_FILE_SIZE_BYTES)
                .events_per_batch(100)
                .build();

            let mut watcher =
                FileWatcher::new(watcher_config).context("Failed to create file watcher")?;

            let event_rx = watcher.watch(&repo_root).await.context(format!(
                "Failed to watch repository at {}",
                repo_root.display()
            ))?;

            // Spawn watcher task
            let repo_id_clone = repo_id;
            let repo_root_clone = repo_root.clone();
            let embedding_manager_clone = embedding_manager.clone();
            let postgres_client_clone = postgres_client.clone();

            tokio::spawn(async move {
                let result = codesearch_indexer::start_watching(
                    event_rx,
                    repo_id_clone,
                    repo_root_clone.clone(),
                    embedding_manager_clone,
                    postgres_client_clone,
                )
                .await;

                if let Err(e) = result {
                    tracing::error!(
                        "Watcher task failed for repository at {}: {e}",
                        repo_root_clone.display()
                    );
                }
            });

            info!("Watcher started for {}", repo_root.display());

            Ok::<_, codesearch_core::Error>((repo_id, watcher))
        }
    });

    let watcher_results = futures::future::join_all(watcher_futures).await;

    // Collect successfully initialized watchers
    let mut watchers = HashMap::new();
    for result in watcher_results {
        match result {
            Ok((repo_id, watcher)) => {
                watchers.insert(repo_id, watcher);
            }
            Err(e) => {
                tracing::error!("Failed to initialize watcher: {e}");
                // Continue with other watchers instead of failing completely
            }
        }
    }

    if watchers.is_empty() {
        return Err(Error::config(
            "Failed to initialize any watchers".to_string(),
        ));
    }

    Ok(Arc::new(RwLock::new(watchers)))
}

/// Run MCP server with shutdown handling for multi-repo mode
async fn run_mcp_server_with_shutdown_multi(
    server: CodeSearchMcpServer,
    watchers: Arc<RwLock<HashMap<Uuid, FileWatcher>>>,
    port: u16,
) -> std::result::Result<(), codesearch_core::Error> {
    let config = StreamableHttpServerConfig {
        sse_keep_alive: Some(std::time::Duration::from_secs(30)),
        stateful_mode: false,
    };

    let session_manager = Arc::new(LocalSessionManager::default());

    let server_clone = server.clone();
    let http_service =
        StreamableHttpService::new(move || Ok(server_clone.clone()), session_manager, config);

    let app = Router::new().nest_service("/mcp", http_service);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| Error::config(format!("Failed to bind to {addr}: {e}")))?;

    println!("🚀 Starting multi-repository MCP server on http://{addr}/mcp");
    info!("MCP server listening on http://{addr}/mcp");

    let server_task = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .map_err(|e| Error::config(format!("Server error: {e}")))
    });

    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            info!("Received Ctrl+C, initiating graceful shutdown");
        }
        Err(e) => {
            tracing::error!("Error setting up signal handler: {e}");
        }
    }

    server_task.abort();
    let _ = server_task.await;

    info!("Stopping all file watchers...");
    let mut watchers_guard = watchers.write().await;
    for (repo_id, watcher) in watchers_guard.iter_mut() {
        info!("Stopping watcher for repository {}", repo_id);
        if let Err(e) = watcher.stop().await {
            tracing::error!("Error stopping watcher for {}: {e}", repo_id);
        }
    }

    info!("Codesearch MCP server shut down successfully");

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_bge_query_formatting() {
        let instruction =
            "Represent this code search query for retrieving semantically similar code snippets";
        let query = "find async functions";
        let formatted = format!("<instruct>{instruction}\n<query>{query}");

        // Verify format structure
        assert!(formatted.starts_with("<instruct>"));
        assert!(formatted.contains("\n<query>"));
        assert_eq!(formatted.matches('\n').count(), 1);

        // Verify components are present
        assert!(formatted.contains(instruction));
        assert!(formatted.contains(query));

        // Verify exact format matches BGE specification
        let expected = format!("<instruct>{instruction}\n<query>{query}");
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_bge_query_formatting_with_special_characters() {
        let instruction = "Test instruction";
        let query = "find \"quoted\" code with\nnewlines and\ttabs";
        let formatted = format!("<instruct>{instruction}\n<query>{query}");

        // Special characters should be preserved in the query
        assert!(formatted.contains("\"quoted\""));
        assert!(formatted.contains("\nnewlines"));
        assert!(formatted.contains("\ttabs"));

        // But only ONE newline should be between <instruct> and <query> tags
        let parts: Vec<&str> = formatted.split("<query>").collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].ends_with("\n"));
    }

    #[test]
    fn test_bge_query_formatting_empty_query() {
        let instruction = "Test instruction";
        let query = "";
        let formatted = format!("<instruct>{instruction}\n<query>{query}");

        // Should still have proper structure even with empty query
        assert!(formatted.starts_with("<instruct>"));
        assert!(formatted.contains("\n<query>"));
        assert_eq!(formatted, format!("<instruct>{instruction}\n<query>"));
    }
}
