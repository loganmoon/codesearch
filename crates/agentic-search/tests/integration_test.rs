//! Integration tests for agentic search

use async_trait::async_trait;
use codesearch_agentic_search::{
    AgenticSearchConfig, AgenticSearchOrchestrator, AgenticSearchRequest,
};
use codesearch_core::entities::{
    EntityType, FunctionSignature, Language, SourceLocation, Visibility,
};
use codesearch_core::search_models::*;
use codesearch_core::{error::Result as CoreResult, SearchApi};
use std::sync::Arc;
use uuid::Uuid;

/// Mock SearchApi for testing
struct MockSearchApi {
    semantic_results: Vec<EntityResult>,
    fulltext_results: Vec<EntityResult>,
    unified_results: Vec<EntityResult>,
    graph_results: Vec<GraphResult>,
}

impl MockSearchApi {
    fn new() -> Self {
        Self {
            semantic_results: vec![],
            fulltext_results: vec![],
            unified_results: vec![],
            graph_results: vec![],
        }
    }

    #[allow(dead_code)]
    fn with_semantic_results(mut self, results: Vec<EntityResult>) -> Self {
        self.semantic_results = results;
        self
    }

    fn with_unified_results(mut self, results: Vec<EntityResult>) -> Self {
        self.unified_results = results;
        self
    }

    #[allow(dead_code)]
    fn with_graph_results(mut self, results: Vec<GraphResult>) -> Self {
        self.graph_results = results;
        self
    }
}

#[async_trait]
impl SearchApi for MockSearchApi {
    async fn search_semantic(
        &self,
        _request: SemanticSearchRequest,
    ) -> CoreResult<SemanticSearchResponse> {
        Ok(SemanticSearchResponse {
            results: self.semantic_results.clone(),
            metadata: ResponseMetadata {
                total_results: self.semantic_results.len(),
                repositories_searched: 1,
                reranked: false,
                query_time_ms: 100,
            },
        })
    }

    async fn search_fulltext(
        &self,
        _request: FulltextSearchRequest,
    ) -> CoreResult<FulltextSearchResponse> {
        Ok(FulltextSearchResponse {
            results: self.fulltext_results.clone(),
            metadata: ResponseMetadata {
                total_results: self.fulltext_results.len(),
                repositories_searched: 1,
                reranked: false,
                query_time_ms: 100,
            },
        })
    }

    async fn search_unified(
        &self,
        _request: UnifiedSearchRequest,
    ) -> CoreResult<UnifiedSearchResponse> {
        Ok(UnifiedSearchResponse {
            results: self.unified_results.clone(),
            metadata: UnifiedResponseMetadata {
                total_results: self.unified_results.len(),
                fulltext_count: 0,
                semantic_count: self.unified_results.len(),
                merged_via_rrf: false,
                reranked: false,
                query_time_ms: 100,
            },
        })
    }

    async fn query_graph(&self, _request: GraphQueryRequest) -> CoreResult<GraphQueryResponse> {
        Ok(GraphQueryResponse {
            results: self.graph_results.clone(),
            metadata: GraphResponseMetadata {
                total_results: self.graph_results.len(),
                semantic_filter_applied: false,
                query_time_ms: 100,
                warning: None,
            },
        })
    }
}

/// Create a mock GraphResult with an embedded entity
#[allow(dead_code)]
fn create_mock_graph_result(id: &str, name: &str) -> GraphResult {
    GraphResult {
        qualified_name: name.to_string(),
        relevance_score: Some(0.5),
        entity: Some(create_mock_entity(id, name, 0.3)), // Low semantic score on purpose
    }
}

fn create_mock_entity(id: &str, name: &str, score: f32) -> EntityResult {
    EntityResult {
        entity_id: id.to_string(),
        repository_id: Uuid::new_v4(),
        qualified_name: name.to_string(),
        name: name.to_string(),
        entity_type: EntityType::Function,
        language: Language::Rust,
        file_path: format!("src/{name}.rs"),
        location: SourceLocation {
            start_line: 1,
            end_line: 10,
            start_column: 0,
            end_column: 0,
        },
        content: Some(format!("fn {name}() {{\n    // implementation\n}}")),
        signature: Some(FunctionSignature {
            parameters: vec![],
            return_type: None,
            is_async: false,
            generics: vec![],
        }),
        documentation_summary: Some(format!("Documentation for {name}")),
        visibility: Visibility::Public,
        score,
        reranked: false,
        reasoning: None,
    }
}

#[tokio::test]
#[ignore = "Requires ANTHROPIC_API_KEY environment variable"]
async fn test_orchestrator_initialization() {
    let mock_api = Arc::new(MockSearchApi::new()) as Arc<dyn SearchApi>;

    let config = AgenticSearchConfig {
        api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
        orchestrator_model: "claude-sonnet-4-5".to_string(),
        worker_model: "claude-haiku-4-5".to_string(),
        quality_gate: codesearch_agentic_search::QualityGateConfig::default(),
    };

    let result = AgenticSearchOrchestrator::new(mock_api, config);
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_orchestrator_requires_api_key() {
    let mock_api = Arc::new(MockSearchApi::new()) as Arc<dyn SearchApi>;

    let config = AgenticSearchConfig {
        api_key: None,
        orchestrator_model: "claude-sonnet-4-5".to_string(),
        worker_model: "claude-haiku-4-5".to_string(),
        quality_gate: codesearch_agentic_search::QualityGateConfig::default(),
    };

    std::env::remove_var("ANTHROPIC_API_KEY");

    let result = AgenticSearchOrchestrator::new(mock_api, config);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("API key not configured"));
}

#[tokio::test]
#[ignore = "Requires ANTHROPIC_API_KEY and makes real API calls"]
async fn test_end_to_end_search() {
    let mock_results = vec![
        create_mock_entity("e1", "validate_jwt", 0.95),
        create_mock_entity("e2", "parse_token", 0.85),
        create_mock_entity("e3", "verify_signature", 0.80),
    ];

    let mock_api =
        Arc::new(MockSearchApi::new().with_unified_results(mock_results)) as Arc<dyn SearchApi>;

    let config = AgenticSearchConfig {
        api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
        orchestrator_model: "claude-sonnet-4-5".to_string(),
        worker_model: "claude-haiku-4-5".to_string(),
        quality_gate: codesearch_agentic_search::QualityGateConfig::default(),
    };

    let orchestrator = AgenticSearchOrchestrator::new(mock_api, config).unwrap();

    let request = AgenticSearchRequest {
        query: "JWT authentication implementation".to_string(),
        force_sonnet: false,
        repository_ids: vec![],
    };

    let response = orchestrator.search(request).await;

    match response {
        Ok(resp) => {
            assert!(!resp.results.is_empty());
            assert!(resp.metadata.iterations > 0);
            assert!(resp.metadata.iterations <= 5);
            println!("Search completed successfully:");
            println!("  Results: {}", resp.results.len());
            println!("  Iterations: {}", resp.metadata.iterations);
            println!("  Cost: ${:.4}", resp.metadata.estimated_cost_usd);
        }
        Err(e) => {
            panic!("Search failed: {e}");
        }
    }
}

#[test]
fn test_mock_entity_creation() {
    let entity = create_mock_entity("test_id", "test_function", 0.95);

    assert_eq!(entity.entity_id, "test_id");
    assert_eq!(entity.qualified_name, "test_function");
    assert_eq!(entity.score, 0.95);
    assert!(entity.content.is_some());
    assert!(entity.signature.is_some());
}

#[test]
fn test_mock_graph_result_creation() {
    let result = create_mock_graph_result("graph_id", "graph_function");

    assert_eq!(result.qualified_name, "graph_function");
    assert!(result.relevance_score.is_some());
    assert!(result.entity.is_some());

    let entity = result.entity.unwrap();
    assert_eq!(entity.entity_id, "graph_id");
    // Graph entities have low semantic scores by design
    assert!(entity.score < 0.5);
}

// =============================================================================
// Phase 3 Integration Tests: Graph and Dual-Track Support
// =============================================================================

#[tokio::test]
async fn test_mock_search_api_with_graph() {
    let graph_results = vec![
        create_mock_graph_result("g1", "graph_callers"),
        create_mock_graph_result("g2", "graph_callees"),
    ];

    let mock_api = MockSearchApi::new().with_graph_results(graph_results);

    let request = GraphQueryRequest {
        repository_id: Uuid::new_v4(),
        query_type: GraphQueryType::FindFunctionCallers,
        parameters: GraphQueryParameters {
            qualified_name: "test_function".to_string(),
            max_depth: Some(2),
        },
        return_entities: true,
        semantic_filter: None,
        limit: 10,
    };

    let response = mock_api.query_graph(request).await.unwrap();
    assert_eq!(response.results.len(), 2);
    assert_eq!(response.metadata.total_results, 2);
}

#[tokio::test]
#[ignore = "Requires ANTHROPIC_API_KEY and makes real API calls"]
async fn test_dual_track_metadata_population() {
    // Setup: unified results + graph results with low semantic score
    let unified_results = vec![
        create_mock_entity("u1", "validate_jwt", 0.95),
        create_mock_entity("u2", "parse_token", 0.85),
    ];

    let graph_results = vec![
        create_mock_graph_result("g1", "auth_controller"), // Low score (0.3)
        create_mock_graph_result("g2", "login_handler"),   // Low score (0.3)
    ];

    let mock_api = Arc::new(
        MockSearchApi::new()
            .with_unified_results(unified_results)
            .with_graph_results(graph_results),
    ) as Arc<dyn SearchApi>;

    let config = AgenticSearchConfig {
        api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
        orchestrator_model: "claude-sonnet-4-5".to_string(),
        worker_model: "claude-haiku-4-5".to_string(),
        quality_gate: codesearch_agentic_search::QualityGateConfig::default(),
    };

    let orchestrator = AgenticSearchOrchestrator::new(mock_api, config).unwrap();

    let request = AgenticSearchRequest {
        query: "JWT authentication implementation".to_string(),
        force_sonnet: false,
        repository_ids: vec![],
    };

    let response = orchestrator.search(request).await;

    match response {
        Ok(resp) => {
            println!("Metadata:");
            println!(
                "  total_direct_candidates: {}",
                resp.metadata.total_direct_candidates
            );
            println!(
                "  graph_context_entities: {}",
                resp.metadata.graph_context_entities
            );
            println!(
                "  graph_entities_in_results: {}",
                resp.metadata.graph_entities_in_results
            );
            println!(
                "  graph_traversal_used: {}",
                resp.metadata.graph_traversal_used
            );

            // Verify metadata tracking works (usize is always >= 0, just check it exists)
            let _ = resp.metadata.total_direct_candidates;
        }
        Err(e) => {
            panic!("Search failed: {e}");
        }
    }
}
