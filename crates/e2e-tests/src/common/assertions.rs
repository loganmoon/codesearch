//! Custom assertions for E2E tests

use super::containers::TestQdrant;
use super::fixtures::ExpectedEntity;
use anyhow::{Context, Result};
use serde::Deserialize;

/// Assert that a collection exists in Qdrant
pub async fn assert_collection_exists(qdrant: &TestQdrant, collection_name: &str) -> Result<()> {
    let url = format!("{}/collections/{}", qdrant.rest_url(), collection_name);
    let response = reqwest::get(&url)
        .await
        .context("Failed to query Qdrant collections endpoint")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Collection '{collection_name}' does not exist. Status: {}",
            response.status()
        ));
    }

    Ok(())
}

/// Assert that a collection has the expected number of points
pub async fn assert_point_count(
    qdrant: &TestQdrant,
    collection_name: &str,
    expected: usize,
) -> Result<()> {
    let url = format!("{}/collections/{}", qdrant.rest_url(), collection_name);
    let response = reqwest::get(&url)
        .await
        .context("Failed to query Qdrant collection info")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to get collection info. Status: {}",
            response.status()
        ));
    }

    let info: CollectionInfo = response
        .json()
        .await
        .context("Failed to parse collection info")?;

    let actual = info.result.points_count;
    if actual != expected {
        return Err(anyhow::anyhow!(
            "Expected {expected} points but found {actual} in collection '{collection_name}'"
        ));
    }

    Ok(())
}

/// Get the current point count for a collection
pub async fn get_point_count(qdrant: &TestQdrant, collection_name: &str) -> Result<usize> {
    let url = format!("{}/collections/{}", qdrant.rest_url(), collection_name);
    let response = reqwest::get(&url)
        .await
        .context("Failed to query Qdrant collection info")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to get collection info. Status: {}",
            response.status()
        ));
    }

    let info: CollectionInfo = response
        .json()
        .await
        .context("Failed to parse collection info")?;

    Ok(info.result.points_count)
}

/// Assert that a collection has at least the minimum number of points
pub async fn assert_min_point_count(
    qdrant: &TestQdrant,
    collection_name: &str,
    minimum: usize,
) -> Result<()> {
    let actual = get_point_count(qdrant, collection_name).await?;
    if actual < minimum {
        return Err(anyhow::anyhow!(
            "Expected at least {minimum} points but found {actual} in collection '{collection_name}'"
        ));
    }

    Ok(())
}

/// Assert that an expected entity exists in Qdrant
pub async fn assert_entity_in_qdrant(
    qdrant: &TestQdrant,
    collection_name: &str,
    expected: &ExpectedEntity,
) -> Result<()> {
    // Scroll through all points to find matching entity
    let url = format!(
        "{}/collections/{}/points/scroll",
        qdrant.rest_url(),
        collection_name
    );

    let client = reqwest::Client::new();
    let mut offset: Option<serde_json::Value> = None;
    let mut found = false;

    // Scroll through points in batches
    loop {
        let mut body = serde_json::json!({
            "limit": 100,
            "with_payload": true,
            "with_vector": false,
        });

        if let Some(ref offset_val) = offset {
            body["offset"] = offset_val.clone();
        }

        let response = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to scroll points")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to scroll points. Status: {}",
                response.status()
            ));
        }

        let scroll_result: ScrollResult = response
            .json()
            .await
            .context("Failed to parse scroll result")?;

        // Check each point's payload
        for point in &scroll_result.result.points {
            if let Some(payload) = &point.payload {
                if let (Some(name), Some(entity_type), Some(file_path)) = (
                    payload.get("name").and_then(|v| v.as_str()),
                    payload.get("entity_type").and_then(|v| v.as_str()),
                    payload.get("file_path").and_then(|v| v.as_str()),
                ) {
                    // EntityType serializes as snake_case (e.g., "struct" not "Struct")
                    let expected_type = format!("{:?}", expected.entity_type).to_lowercase();
                    if name == expected.name
                        && entity_type.eq_ignore_ascii_case(&expected_type)
                        && file_path.contains(&expected.file_path_contains)
                    {
                        found = true;
                        break;
                    }
                }
            }
        }

        if found {
            break;
        }

        // Check if there are more points
        if let Some(next_offset) = scroll_result.result.next_page_offset {
            offset = Some(next_offset);
        } else {
            break;
        }
    }

    if !found {
        return Err(anyhow::anyhow!(
            "Expected entity not found: {} ({:?}) in file containing '{}'",
            expected.name,
            expected.entity_type,
            expected.file_path_contains
        ));
    }

    Ok(())
}

/// Assert that the collection has the correct vector dimensions
pub async fn assert_vector_dimensions(
    qdrant: &TestQdrant,
    collection_name: &str,
    expected_dims: usize,
) -> Result<()> {
    let url = format!("{}/collections/{}", qdrant.rest_url(), collection_name);
    let response = reqwest::get(&url)
        .await
        .context("Failed to query Qdrant collection info")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to get collection info. Status: {}",
            response.status()
        ));
    }

    let info: CollectionInfo = response
        .json()
        .await
        .context("Failed to parse collection info")?;

    let actual_dims = info.result.config.params.vectors.size;
    if actual_dims != expected_dims {
        return Err(anyhow::anyhow!(
            "Expected vector dimensions {expected_dims} but found {actual_dims}"
        ));
    }

    Ok(())
}

// =============================================================================
// Neo4j Graph Assertions
// =============================================================================

use super::containers::TestNeo4j;
use std::sync::Arc;

// Neo4j Community Edition only supports a single database.
// All queries use the default 'neo4j' database with repository_id filtering for isolation.
const NEO4J_DEFAULT_DATABASE: &str = "neo4j";

/// Assert that Neo4j has at least the minimum number of Entity nodes for a repository
pub async fn assert_min_neo4j_nodes(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
    minimum: usize,
) -> Result<()> {
    let node_count = query_neo4j_node_count(neo4j, repository_id).await?;
    if node_count < minimum {
        return Err(anyhow::anyhow!(
            "Expected at least {minimum} nodes but found {node_count} for repository '{repository_id}'"
        ));
    }
    Ok(())
}

/// Query the total number of Entity nodes in Neo4j for a specific repository
///
/// Uses the default 'neo4j' database with repository_id filtering (Community Edition pattern).
pub async fn query_neo4j_node_count(neo4j: &Arc<TestNeo4j>, repository_id: &str) -> Result<usize> {
    let url = format!(
        "{}/db/{}/tx/commit",
        neo4j.http_url(),
        NEO4J_DEFAULT_DATABASE
    );

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "statements": [{
            "statement": "MATCH (n:Entity {repository_id: $repo_id}) RETURN count(n) AS count",
            "parameters": {
                "repo_id": repository_id
            }
        }]
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to query Neo4j")?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Neo4j query failed: {text}"));
    }

    let result: Neo4jTransactionResult = response
        .json()
        .await
        .context("Failed to parse Neo4j response")?;

    // Check for errors in the response
    if !result.errors.is_empty() {
        return Err(anyhow::anyhow!("Neo4j query failed: {:?}", result.errors));
    }

    // Extract count from result
    let count = result
        .results
        .first()
        .and_then(|r| r.data.first())
        .and_then(|d| d.row.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    Ok(count)
}

/// Assert that Neo4j has relationships of the specified type for a repository
pub async fn assert_has_relationship_type(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
    rel_type: &str,
) -> Result<()> {
    let count = query_neo4j_relationship_count(neo4j, repository_id, rel_type).await?;
    if count == 0 {
        return Err(anyhow::anyhow!(
            "Expected at least one {rel_type} relationship but found none"
        ));
    }
    Ok(())
}

/// Query the count of relationships of a specific type for a repository
///
/// Uses the default 'neo4j' database with repository_id filtering (Community Edition pattern).
pub async fn query_neo4j_relationship_count(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
    rel_type: &str,
) -> Result<usize> {
    let url = format!(
        "{}/db/{}/tx/commit",
        neo4j.http_url(),
        NEO4J_DEFAULT_DATABASE
    );

    let client = reqwest::Client::new();
    // Filter relationships where at least one endpoint belongs to the repository
    let statement = format!(
        "MATCH (a:Entity {{repository_id: $repo_id}})-[r:{rel_type}]->() RETURN count(r) AS count"
    );
    let body = serde_json::json!({
        "statements": [{
            "statement": statement,
            "parameters": {
                "repo_id": repository_id
            }
        }]
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to query Neo4j")?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Neo4j query failed: {text}"));
    }

    let result: Neo4jTransactionResult = response
        .json()
        .await
        .context("Failed to parse Neo4j response")?;

    let count = result
        .results
        .first()
        .and_then(|r| r.data.first())
        .and_then(|d| d.row.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    Ok(count)
}

/// Assert that a specific resolution chain exists (from_name -> to_name via rel_type)
///
/// Uses the default 'neo4j' database with repository_id filtering (Community Edition pattern).
pub async fn assert_resolution_chain(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
    from_name: &str,
    to_name: &str,
    rel_type: &str,
) -> Result<()> {
    let url = format!(
        "{}/db/{}/tx/commit",
        neo4j.http_url(),
        NEO4J_DEFAULT_DATABASE
    );

    let client = reqwest::Client::new();
    let statement = format!(
        "MATCH (a:Entity {{repository_id: $repo_id, name: $from_name}})-[:{rel_type}]->(b {{name: $to_name}}) RETURN count(*) AS count"
    );
    let body = serde_json::json!({
        "statements": [{
            "statement": statement,
            "parameters": {
                "repo_id": repository_id,
                "from_name": from_name,
                "to_name": to_name
            }
        }]
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to query Neo4j")?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Neo4j query failed: {text}"));
    }

    let result: Neo4jTransactionResult = response
        .json()
        .await
        .context("Failed to parse Neo4j response")?;

    let count = result
        .results
        .first()
        .and_then(|r| r.data.first())
        .and_then(|d| d.row.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    if count == 0 {
        return Err(anyhow::anyhow!(
            "Expected resolution chain {from_name} -[{rel_type}]-> {to_name} but none found"
        ));
    }

    Ok(())
}

/// Get summary statistics for the Neo4j graph for a specific repository
///
/// Uses the default 'neo4j' database with repository_id filtering (Community Edition pattern).
pub async fn get_neo4j_graph_stats(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
) -> Result<Neo4jGraphStats> {
    let node_count = query_neo4j_node_count(neo4j, repository_id).await?;
    // Count edges TO External nodes (not External nodes themselves, as they don't have repository_id)
    let external_edge_count = query_neo4j_external_edge_count(neo4j, repository_id).await.unwrap_or(0);

    let contains = query_neo4j_relationship_count(neo4j, repository_id, "CONTAINS").await.unwrap_or(0);
    let implements = query_neo4j_relationship_count(neo4j, repository_id, "IMPLEMENTS").await.unwrap_or(0);
    let calls = query_neo4j_relationship_count(neo4j, repository_id, "CALLS").await.unwrap_or(0);
    let imports = query_neo4j_relationship_count(neo4j, repository_id, "IMPORTS").await.unwrap_or(0);
    let uses = query_neo4j_relationship_count(neo4j, repository_id, "USES").await.unwrap_or(0);
    let inherits = query_neo4j_relationship_count(neo4j, repository_id, "INHERITS_FROM").await.unwrap_or(0);
    let associates = query_neo4j_relationship_count(neo4j, repository_id, "ASSOCIATES").await.unwrap_or(0);
    let extends_interface = query_neo4j_relationship_count(neo4j, repository_id, "EXTENDS_INTERFACE").await.unwrap_or(0);

    Ok(Neo4jGraphStats {
        node_count,
        external_edge_count,
        contains_count: contains,
        implements_count: implements,
        calls_count: calls,
        imports_count: imports,
        uses_count: uses,
        inherits_count: inherits,
        associates_count: associates,
        extends_interface_count: extends_interface,
    })
}

/// Query the count of relationships TO External nodes from entities in this repository
///
/// External nodes don't have repository_id (they're shared across repos), so we count
/// edges from this repository's entities to External nodes instead.
pub async fn query_neo4j_external_edge_count(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
) -> Result<usize> {
    let url = format!(
        "{}/db/{}/tx/commit",
        neo4j.http_url(),
        NEO4J_DEFAULT_DATABASE
    );

    let client = reqwest::Client::new();
    // Count relationships from entities in this repo to External nodes
    let body = serde_json::json!({
        "statements": [{
            "statement": "MATCH (e:Entity {repository_id: $repo_id})-[r]->(ext:External) RETURN count(r) AS count",
            "parameters": {
                "repo_id": repository_id
            }
        }]
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to query Neo4j for External edges")?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Neo4j query failed: {text}"));
    }

    let result: Neo4jTransactionResult = response
        .json()
        .await
        .context("Failed to parse Neo4j response")?;

    let count = result
        .results
        .first()
        .and_then(|r| r.data.first())
        .and_then(|d| d.row.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    Ok(count)
}

/// Summary statistics for a Neo4j graph
#[derive(Debug, Clone)]
pub struct Neo4jGraphStats {
    pub node_count: usize,
    /// Count of edges TO External nodes (unresolved external references)
    pub external_edge_count: usize,
    pub contains_count: usize,
    pub implements_count: usize,
    pub calls_count: usize,
    pub imports_count: usize,
    pub uses_count: usize,
    pub inherits_count: usize,
    pub associates_count: usize,
    pub extends_interface_count: usize,
}

impl Neo4jGraphStats {
    /// Get total internal relationship count (edges between Entity nodes)
    pub fn total_internal_relationships(&self) -> usize {
        self.contains_count
            + self.implements_count
            + self.calls_count
            + self.imports_count
            + self.uses_count
            + self.inherits_count
            + self.associates_count
            + self.extends_interface_count
    }

    /// Get total relationship count including external edges
    pub fn total_relationships(&self) -> usize {
        self.total_internal_relationships() + self.external_edge_count
    }

    /// Calculate internal resolution rate
    ///
    /// Returns the percentage of edges that point to internal entities
    /// vs External stub nodes.
    /// Formula: internal_edges / (internal_edges + external_edges) * 100
    pub fn internal_resolution_rate(&self) -> f64 {
        let internal = self.total_internal_relationships();
        let total = internal + self.external_edge_count;
        if total == 0 {
            100.0
        } else {
            (internal as f64 / total as f64) * 100.0
        }
    }

    /// Calculate relationship density (relationships per entity)
    pub fn relationship_density(&self) -> f64 {
        if self.node_count == 0 {
            0.0
        } else {
            self.total_internal_relationships() as f64 / self.node_count as f64
        }
    }

    /// Print a human-readable summary
    pub fn print_summary(&self) {
        println!("\n=== Neo4j Graph Statistics ===");
        println!("Entity nodes: {}", self.node_count);
        println!("External edges: {}", self.external_edge_count);
        println!("CONTAINS relationships: {}", self.contains_count);
        println!("IMPLEMENTS relationships: {}", self.implements_count);
        println!("CALLS relationships: {}", self.calls_count);
        println!("IMPORTS relationships: {}", self.imports_count);
        println!("USES relationships: {}", self.uses_count);
        println!("INHERITS_FROM relationships: {}", self.inherits_count);
        println!("ASSOCIATES relationships: {}", self.associates_count);
        println!("EXTENDS_INTERFACE relationships: {}", self.extends_interface_count);
        println!("Total internal relationships: {}", self.total_internal_relationships());
        println!("Internal resolution rate: {:.1}%", self.internal_resolution_rate());
        println!("Relationship density: {:.2}", self.relationship_density());
    }
}

// =============================================================================
// Resolution Metrics (Deprecated - use Neo4jGraphStats methods instead)
// =============================================================================

use super::containers::TestPostgres;

/// Resolution metrics showing resolved relationships
///
/// DEPRECATED: Use `Neo4jGraphStats::internal_resolution_rate()` and
/// `Neo4jGraphStats::relationship_density()` instead, which provide more
/// accurate metrics based on External stub node counts.
#[derive(Debug, Clone)]
#[deprecated(
    since = "0.1.0",
    note = "Use Neo4jGraphStats methods instead: internal_resolution_rate(), relationship_density()"
)]
pub struct ResolutionMetrics {
    /// Number of relationships successfully resolved (in Neo4j)
    pub resolved_count: usize,
}

#[allow(deprecated)]
impl ResolutionMetrics {
    /// Print a human-readable summary
    pub fn print_summary(&self) {
        println!("\n=== Resolution Metrics ===");
        println!("Resolved relationships: {}", self.resolved_count);
    }
}

/// Get resolution metrics for a repository
///
/// DEPRECATED: Use `get_neo4j_graph_stats()` instead, which returns a
/// `Neo4jGraphStats` with comprehensive metrics including external node
/// counts and resolution rates.
#[allow(unused_variables)]
#[deprecated(
    since = "0.1.0",
    note = "Use get_neo4j_graph_stats() instead for comprehensive metrics"
)]
#[allow(deprecated)]
pub async fn get_resolution_metrics(
    postgres: &Arc<TestPostgres>,
    neo4j: &Arc<TestNeo4j>,
    db_name: &str,
    repository_id: &str,
) -> Result<ResolutionMetrics> {
    let stats = get_neo4j_graph_stats(neo4j, repository_id).await?;
    let resolved_count = stats.total_relationships();

    Ok(ResolutionMetrics { resolved_count })
}

/// Assert that a specific relationship exists between two entities
///
/// Verifies that source_name -[rel_type]-> target_name exists in the graph.
pub async fn assert_relationship_exists(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
    source_name: &str,
    rel_type: &str,
    target_name: &str,
) -> Result<()> {
    let url = format!(
        "{}/db/{}/tx/commit",
        neo4j.http_url(),
        NEO4J_DEFAULT_DATABASE
    );

    let client = reqwest::Client::new();
    let statement = format!(
        "MATCH (a:Entity {{repository_id: $repo_id}})-[r:{rel_type}]->(b:Entity)
         WHERE a.name = $source_name AND b.name = $target_name
         RETURN count(r) AS count"
    );
    let body = serde_json::json!({
        "statements": [{
            "statement": statement,
            "parameters": {
                "repo_id": repository_id,
                "source_name": source_name,
                "target_name": target_name
            }
        }]
    });

    let response = client.post(&url).json(&body).send().await?;
    let result: Neo4jTransactionResult = response.json().await?;

    let count = result
        .results
        .first()
        .and_then(|r| r.data.first())
        .and_then(|d| d.row.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    if count == 0 {
        return Err(anyhow::anyhow!(
            "Expected relationship {source_name} -[{rel_type}]-> {target_name} not found"
        ));
    }

    Ok(())
}

/// List all entities in the graph (for debugging)
///
/// Returns entities prioritizing named ones over anonymous, using Neo4j labels
/// for entity type since entity_type is stored as a label not a property.
pub async fn list_neo4j_entities(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
) -> Result<Vec<(String, String)>> {
    let url = format!(
        "{}/db/{}/tx/commit",
        neo4j.http_url(),
        NEO4J_DEFAULT_DATABASE
    );

    let client = reqwest::Client::new();
    // Use labels() to get entity type since it's stored as a label, not a property
    // Sort to show named entities first (those not starting with '<')
    let body = serde_json::json!({
        "statements": [{
            "statement": r#"
                MATCH (n:Entity {repository_id: $repo_id})
                RETURN n.name, labels(n), n.qualified_name
                ORDER BY
                    CASE WHEN n.name STARTS WITH '<' THEN 1 ELSE 0 END,
                    n.name
                LIMIT 40
            "#,
            "parameters": { "repo_id": repository_id }
        }]
    });

    let response = client.post(&url).json(&body).send().await?;
    let text = response.text().await?;

    // Parse and check for errors
    let result: Neo4jTransactionResult = serde_json::from_str(&text)
        .with_context(|| format!("Failed to parse Neo4j response: {}", text))?;

    if !result.errors.is_empty() {
        println!("    Neo4j errors: {:?}", result.errors);
    }

    let row_count = result.results.first().map(|r| r.data.len()).unwrap_or(0);
    if row_count == 0 {
        println!("    (no entities returned from Neo4j query)");
    }

    let entities: Vec<(String, String)> = result
        .results
        .first()
        .map(|r| {
            r.data
                .iter()
                .map(|d| {
                    // Handle nulls gracefully
                    let name = d.row.first()
                        .and_then(|v| v.as_str())
                        .unwrap_or("<null>");
                    // labels() returns an array like ["Entity", "Function"]
                    let labels = d.row.get(1)
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .filter(|s| *s != "Entity") // Skip the common Entity label
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .unwrap_or_else(|| "<unknown>".to_string());
                    let qualified_name = d.row.get(2)
                        .and_then(|v| v.as_str())
                        .unwrap_or("<null>");

                    // Show qualified_name if name is null
                    let display_name = if name == "<null>" {
                        qualified_name.to_string()
                    } else {
                        name.to_string()
                    };

                    (display_name, labels)
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(entities)
}

/// List all relationships in the graph (for debugging)
pub async fn list_neo4j_relationships(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
) -> Result<Vec<(String, String, String)>> {
    let url = format!(
        "{}/db/{}/tx/commit",
        neo4j.http_url(),
        NEO4J_DEFAULT_DATABASE
    );

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "statements": [{
            "statement": "MATCH (a:Entity {repository_id: $repo_id})-[r]->(b:Entity) RETURN a.name, type(r), b.name ORDER BY a.name, type(r)",
            "parameters": { "repo_id": repository_id }
        }]
    });

    let response = client.post(&url).json(&body).send().await?;
    let result: Neo4jTransactionResult = response.json().await?;

    let relationships: Vec<(String, String, String)> = result
        .results
        .first()
        .map(|r| {
            r.data
                .iter()
                .filter_map(|d| {
                    let source = d.row.first()?.as_str()?.to_string();
                    let rel_type = d.row.get(1)?.as_str()?.to_string();
                    let target = d.row.get(2)?.as_str()?.to_string();
                    Some((source, rel_type, target))
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(relationships)
}

// Neo4j HTTP API response structures
#[derive(Debug, Deserialize)]
struct Neo4jTransactionResult {
    results: Vec<Neo4jStatementResult>,
    #[serde(default)]
    errors: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Neo4jStatementResult {
    data: Vec<Neo4jDataRow>,
}

#[derive(Debug, Deserialize)]
struct Neo4jDataRow {
    row: Vec<serde_json::Value>,
}

// =============================================================================
// Response structures for Qdrant REST API
// =============================================================================

#[derive(Debug, Deserialize)]
struct CollectionInfo {
    result: CollectionResult,
}

#[derive(Debug, Deserialize)]
struct CollectionResult {
    points_count: usize,
    config: CollectionConfig,
}

#[derive(Debug, Deserialize)]
struct CollectionConfig {
    params: CollectionParams,
}

#[derive(Debug, Deserialize)]
struct CollectionParams {
    vectors: VectorParams,
}

#[derive(Debug, Deserialize)]
struct VectorParams {
    size: usize,
}

#[derive(Debug, Deserialize)]
struct ScrollResult {
    result: ScrollResultData,
}

#[derive(Debug, Deserialize)]
struct ScrollResultData {
    points: Vec<Point>,
    next_page_offset: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Point {
    payload: Option<serde_json::Map<String, serde_json::Value>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires Docker
    async fn test_assertions_with_real_qdrant() -> Result<()> {
        // This test requires a running Qdrant instance
        let qdrant = TestQdrant::start().await?;

        // Create a test collection
        let collection_name = format!("test_collection_{}", uuid::Uuid::new_v4());
        let url = format!("{}/collections/{}", qdrant.rest_url(), collection_name);

        let client = reqwest::Client::new();
        let create_body = serde_json::json!({
            "vectors": {
                "size": 384,
                "distance": "Cosine"
            }
        });

        client.put(&url).json(&create_body).send().await?;

        // Test assert_collection_exists
        assert_collection_exists(&qdrant, &collection_name).await?;

        // Test assert_point_count (should be 0 for new collection)
        assert_point_count(&qdrant, &collection_name, 0).await?;

        // Test assert_vector_dimensions
        assert_vector_dimensions(&qdrant, &collection_name, 384).await?;

        Ok(())
    }
}
