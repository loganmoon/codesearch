//! LSP Validation test for codesearch relationship extraction
//!
//! This test validates that the relationships stored in Neo4j are correct
//! by comparing against Language Server Protocol (LSP) ground truth.
//!
//! ## Running
//!
//! Requires typescript-language-server to be installed:
//! ```bash
//! npm install -g typescript typescript-language-server
//! ```
//!
//! Run the test:
//! ```bash
//! cargo test --manifest-path crates/e2e-tests/Cargo.toml --test test_lsp_validation -- --ignored --nocapture
//! ```

use anyhow::{Context, Result};
use codesearch_core::entities::Language;
use codesearch_core::CodeEntity;
use codesearch_e2e_tests::common::*;
use codesearch_lsp_validation::{LspClient, LspServer, Neo4jEdge, ValidationEngine};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const NEO4J_DEFAULT_DATABASE: &str = "neo4j";

/// Create a test config file with full infrastructure
fn create_test_config(
    repo_path: &Path,
    qdrant: &TestQdrant,
    postgres: &TestPostgres,
    neo4j: &TestNeo4j,
    db_name: &str,
) -> Result<std::path::PathBuf> {
    let config_content = format!(
        r#"
[indexer]

[storage]
qdrant_host = "localhost"
qdrant_port = {}
qdrant_rest_port = {}
auto_start_deps = false
postgres_host = "localhost"
postgres_port = {}
postgres_database = "{}"
postgres_user = "codesearch"
postgres_password = "codesearch"
neo4j_host = "localhost"
neo4j_http_port = {}
neo4j_bolt_port = {}
neo4j_user = "neo4j"
neo4j_password = ""

[embeddings]
provider = "mock"

[watcher]
debounce_ms = 500
ignore_patterns = ["*.log", "target", ".git", "node_modules"]

[languages]
enabled = ["typescript", "javascript"]
"#,
        qdrant.port(),
        qdrant.rest_port(),
        postgres.port(),
        db_name,
        neo4j.http_port(),
        neo4j.bolt_port(),
    );

    let config_path = repo_path.join("codesearch.toml");
    std::fs::write(&config_path, config_content)?;
    Ok(config_path)
}

/// Fetch entities from Postgres (entity_metadata table has location info)
async fn fetch_entities_from_postgres(
    postgres: &Arc<TestPostgres>,
    db_name: &str,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/{db_name}",
        postgres.port()
    );
    let pool = sqlx::PgPool::connect(&connection_url).await?;

    // Query entity_metadata table - this has file_path and location info
    // entity_data JSONB contains the full SourceLocation
    let rows: Vec<EntityRow> = sqlx::query_as(
        r#"
        SELECT
            entity_id,
            name,
            qualified_name,
            file_path,
            entity_type,
            entity_data
        FROM entity_metadata
        WHERE repository_id = $1::uuid AND deleted_at IS NULL
        "#,
    )
    .bind(repository_id)
    .fetch_all(&pool)
    .await
    .context("Failed to query entity_metadata")?;

    let mut entities = Vec::new();
    for row in rows {
        // Parse location from entity_data JSONB
        let (start_line, end_line, start_column, end_column) = if let Some(data) = &row.entity_data {
            let loc = data.get("location");
            (
                loc.and_then(|l| l.get("start_line")).and_then(|v| v.as_u64()).unwrap_or(1) as usize,
                loc.and_then(|l| l.get("end_line")).and_then(|v| v.as_u64()).unwrap_or(1) as usize,
                loc.and_then(|l| l.get("start_column")).and_then(|v| v.as_u64()).unwrap_or(0) as usize,
                loc.and_then(|l| l.get("end_column")).and_then(|v| v.as_u64()).unwrap_or(0) as usize,
            )
        } else {
            (1, 1, 0, 0)
        };

        let entity_type = row.entity_type
            .parse()
            .unwrap_or(codesearch_core::entities::EntityType::Function);

        // entity_id is already in format "entity-{hash}" from generate_entity_id
        let neo4j_entity_id = row.entity_id.clone();

        entities.push(CodeEntity {
            entity_id: neo4j_entity_id,
            repository_id: repository_id.to_string(),
            name: row.name,
            qualified_name: row.qualified_name,
            path_entity_identifier: None,
            parent_scope: None,
            file_path: std::path::PathBuf::from(row.file_path),
            location: codesearch_core::entities::SourceLocation {
                start_line,
                end_line,
                start_column,
                end_column,
            },
            entity_type,
            language: Language::TypeScript,
            visibility: codesearch_core::entities::Visibility::Public,
            metadata: codesearch_core::entities::EntityMetadata::default(),
            content: None,
            dependencies: Vec::new(),
            documentation_summary: None,
            signature: None,
        });
    }

    pool.close().await;
    Ok(entities)
}

/// Row type for entity query
#[derive(sqlx::FromRow)]
struct EntityRow {
    entity_id: String,
    name: String,
    qualified_name: String,
    file_path: String,
    entity_type: String,
    entity_data: Option<serde_json::Value>,
}

/// Count internal-to-internal edges (Entity -> Entity, not External)
async fn count_internal_edges(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
) -> Result<usize> {
    let url = format!(
        "{}/db/{}/tx/commit",
        neo4j.http_url(),
        NEO4J_DEFAULT_DATABASE
    );

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "statements": [{
            "statement": r#"
                MATCH (a:Entity {repository_id: $repo_id})-[r]->(b:Entity)
                WHERE NOT type(r) IN ['CONTAINS', 'IMPORTED_BY', 'USED_BY', 'CALLED_BY']
                RETURN count(r) as cnt
            "#,
            "parameters": {
                "repo_id": repository_id
            }
        }]
    });

    let response = client.post(&url).json(&body).send().await?;
    let text = response.text().await?;

    #[derive(serde::Deserialize)]
    struct Neo4jResult {
        results: Vec<Neo4jStatementResult>,
    }

    #[derive(serde::Deserialize)]
    struct Neo4jStatementResult {
        data: Vec<Neo4jDataRow>,
    }

    #[derive(serde::Deserialize)]
    struct Neo4jDataRow {
        row: Vec<serde_json::Value>,
    }

    let result: Neo4jResult = serde_json::from_str(&text)?;

    let count = result
        .results
        .first()
        .and_then(|r| r.data.first())
        .and_then(|d| d.row.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    Ok(count)
}

/// Query all relationship types that exist in Neo4j for debugging
async fn fetch_all_relationship_types(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
) -> Result<Vec<(String, usize)>> {
    let url = format!(
        "{}/db/{}/tx/commit",
        neo4j.http_url(),
        NEO4J_DEFAULT_DATABASE
    );

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "statements": [{
            "statement": r#"
                MATCH (a:Entity {repository_id: $repo_id})-[r]->()
                RETURN type(r) as rel_type, count(r) as cnt
                ORDER BY cnt DESC
            "#,
            "parameters": {
                "repo_id": repository_id
            }
        }]
    });

    let response = client.post(&url).json(&body).send().await?;
    let text = response.text().await?;

    #[derive(serde::Deserialize)]
    struct Neo4jResult {
        results: Vec<Neo4jStatementResult>,
    }

    #[derive(serde::Deserialize)]
    struct Neo4jStatementResult {
        data: Vec<Neo4jDataRow>,
    }

    #[derive(serde::Deserialize)]
    struct Neo4jDataRow {
        row: Vec<serde_json::Value>,
    }

    let result: Neo4jResult = serde_json::from_str(&text)?;

    let types: Vec<(String, usize)> = result
        .results
        .first()
        .map(|r| {
            r.data
                .iter()
                .filter_map(|d| {
                    let rel_type = d.row.first()?.as_str()?.to_string();
                    let count = d.row.get(1)?.as_u64()? as usize;
                    Some((rel_type, count))
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(types)
}

/// Fetch relationship edges from Neo4j
async fn fetch_neo4j_edges(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
) -> Result<Vec<Neo4jEdge>> {
    let url = format!(
        "{}/db/{}/tx/commit",
        neo4j.http_url(),
        NEO4J_DEFAULT_DATABASE
    );

    let client = reqwest::Client::new();
    // Query only Entity->Entity relationships (internal references)
    // We can only validate these with LSP since external refs point outside the codebase
    // Skip structural relationships like CONTAINS
    let body = serde_json::json!({
        "statements": [{
            "statement": r#"
                MATCH (a:Entity {repository_id: $repo_id})-[r]->(b:Entity)
                WHERE NOT type(r) IN ['CONTAINS', 'IMPORTED_BY', 'USED_BY', 'CALLED_BY', 'IMPLEMENTED_BY', 'EXTENDED_BY', 'HAS_SUBCLASS']
                RETURN a.id, b.id, type(r)
            "#,
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
        .context("Failed to query Neo4j for edges")?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Neo4j query failed: {text}"));
    }

    #[derive(serde::Deserialize)]
    struct Neo4jResult {
        results: Vec<Neo4jStatementResult>,
        #[serde(default)]
        errors: Vec<serde_json::Value>,
    }

    #[derive(serde::Deserialize)]
    struct Neo4jStatementResult {
        data: Vec<Neo4jDataRow>,
    }

    #[derive(serde::Deserialize)]
    struct Neo4jDataRow {
        row: Vec<serde_json::Value>,
    }

    let text = response.text().await.context("Failed to read Neo4j response")?;

    let result: Neo4jResult = serde_json::from_str(&text)
        .context("Failed to parse Neo4j response")?;

    if !result.errors.is_empty() {
        return Err(anyhow::anyhow!("Neo4j query errors: {:?}", result.errors));
    }

    // Debug: print first few raw rows
    if let Some(stmt) = result.results.first() {
        let row_count = stmt.data.len();
        println!("Neo4j returned {} raw rows", row_count);
        for (i, row) in stmt.data.iter().take(3).enumerate() {
            println!("  Row {}: {:?}", i, row.row);
        }
    }

    let edges: Vec<Neo4jEdge> = result
        .results
        .first()
        .map(|r| {
            r.data
                .iter()
                .filter_map(|d| {
                    let from_id = d.row.first()?.as_str()?.to_string();
                    let to_id = d.row.get(1)?.as_str()?.to_string();
                    let rel_type = d.row.get(2)?.as_str()?.to_string();
                    Some(Neo4jEdge {
                        from_entity_id: from_id,
                        to_entity_id: to_id,
                        rel_type,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(edges)
}

/// Get the repository ID from the database
async fn get_repository_id(postgres: &Arc<TestPostgres>, db_name: &str) -> Result<String> {
    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/{db_name}",
        postgres.port()
    );
    let pool = sqlx::PgPool::connect(&connection_url).await?;
    let id: String = sqlx::query_scalar("SELECT repository_id::text FROM repositories LIMIT 1")
        .fetch_one(&pool)
        .await
        .context("No repository found")?;
    pool.close().await;
    Ok(id)
}

/// Check if typescript-language-server is available (globally or via npx)
fn check_lsp_available() -> bool {
    // Try global installation first
    if std::process::Command::new("typescript-language-server")
        .arg("--version")
        .output()
        .is_ok()
    {
        return true;
    }

    // Fallback to npx
    std::process::Command::new("npx")
        .args(["typescript-language-server", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test]
#[ignore] // Requires Docker, LSP servers, and network access
async fn lsp_validation_typescript() -> Result<()> {
    init_test_logging();

    // Check if LSP is available
    if !check_lsp_available() {
        println!("SKIP: typescript-language-server not found. Install with: npm install -g typescript typescript-language-server");
        return Ok(());
    }

    println!("\nStarting LSP validation test for TypeScript...\n");

    // Start infrastructure
    println!("Starting test infrastructure...");
    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;
    let neo4j = get_shared_neo4j().await?;
    println!("Infrastructure ready.\n");

    // Clone jotai (TypeScript fixture)
    println!("Cloning jotai repository...");
    let repo = real_jotai_project().await?;
    let repo_path = repo.path().to_path_buf();
    println!("Repository cloned to: {}", repo_path.display());

    // Create isolated database
    let db_name = create_test_database(&postgres).await?;
    println!("Created test database: {db_name}");

    // Create config
    create_test_config(&repo_path, &qdrant, &postgres, &neo4j, &db_name)?;

    // Run indexing
    println!("Running codesearch index...");
    let output = run_cli_with_full_infra(&repo_path, &["index"], &qdrant, &postgres, &neo4j, &db_name)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("Indexing stderr: {stderr}");
        return Err(anyhow::anyhow!("Indexing failed"));
    }
    println!("Indexing completed.");

    // Wait for graph resolution
    println!("Waiting for graph resolution...");
    wait_for_graph_ready(&postgres, &db_name, Duration::from_secs(120)).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("Graph resolution complete.");

    // Get repository ID
    let repo_id = get_repository_id(&postgres, &db_name).await?;
    println!("Repository ID: {repo_id}");

    // Fetch entities from Postgres (has file_path and location info)
    println!("Fetching entities from Postgres...");
    let entities = fetch_entities_from_postgres(&postgres, &db_name, &repo_id).await?;
    println!("Found {} entities", entities.len());

    // Debug: show first few entity IDs
    println!("Sample entity IDs from Postgres:");
    for entity in entities.iter().take(3) {
        println!("  {}", entity.entity_id);
    }

    // Fetch relationships from Neo4j
    println!("Fetching relationships from Neo4j...");

    // Debug: query all relationship types first
    let all_rel_types = fetch_all_relationship_types(&neo4j, &repo_id).await?;
    println!("All relationship types in Neo4j: {:?}", all_rel_types);

    // Debug: count internal-to-internal edges specifically
    let internal_edge_count = count_internal_edges(&neo4j, &repo_id).await?;
    println!("Internal-to-internal edges (Entity->Entity): {}", internal_edge_count);

    let edges = fetch_neo4j_edges(&neo4j, &repo_id).await?;
    println!("Found {} relationship edges", edges.len());

    // Debug: show edge type breakdown
    if !edges.is_empty() {
        let mut type_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for edge in &edges {
            *type_counts.entry(&edge.rel_type).or_default() += 1;
        }
        println!("Edge types: {:?}", type_counts);

        // Count how many targets are in entity map
        let entity_ids: std::collections::HashSet<_> = entities.iter().map(|e| &e.entity_id).collect();
        let internal_targets = edges.iter().filter(|e| entity_ids.contains(&e.to_entity_id)).count();
        let external_targets = edges.len() - internal_targets;
        println!("Internal targets: {}, External targets: {}", internal_targets, external_targets);
    }

    if edges.is_empty() {
        println!("No edges found in Neo4j - skipping LSP validation.");
        let _ = drop_test_database(&postgres, &db_name).await;
        return Ok(());
    }

    // Start LSP server
    println!("Starting typescript-language-server...");
    let lsp = LspClient::spawn(LspServer::TypeScript, &repo_path)?;
    println!("LSP server started.");

    // Give LSP time to index the project
    println!("Waiting for LSP to index the project...");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Create validation engine
    let mut engine = ValidationEngine::new(lsp, Language::TypeScript, repo_path.clone(), entities);

    // Run validation
    println!("Running LSP validation...");
    let report = engine.validate_relationships(&edges, "jotai")?;

    // Print report
    report.print_summary();

    // Shutdown LSP
    engine.shutdown()?;

    // Cleanup
    let _ = drop_test_database(&postgres, &db_name).await;

    // Assert quality thresholds
    let overall = report.overall_metrics();
    let f1 = overall.f1();

    println!("\nValidation Results:");
    println!("  F1 Score: {:.1}%", f1 * 100.0);
    println!("  Precision: {:.1}%", overall.precision() * 100.0);
    println!("  Recall: {:.1}%", overall.recall() * 100.0);
    println!("  LSP Errors: {}", overall.lsp_errors);
    println!("  External Refs: {}", overall.external_refs);
    println!("  Module Refs (not validated): {}", overall.module_refs);

    // For initial baseline, we're not asserting strict thresholds
    // This test primarily generates metrics we can use to improve
    if f1 < 0.10 && overall.true_positives > 0 {
        println!("\nWARNING: F1 score is very low ({:.1}%), but found {} true positives", f1 * 100.0, overall.true_positives);
        println!("This may indicate issues with reference location matching.");
    }

    Ok(())
}
