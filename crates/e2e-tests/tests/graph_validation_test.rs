//! Graph validation integration test
//!
//! Validates codesearch's graph extraction against rust-analyzer's SCIP output.
//! This is an observation-only test that produces metrics without assertions.
//!
//! Run with: cargo test --manifest-path crates/e2e-tests/Cargo.toml graph_validation -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use git2::build::RepoBuilder;

use anyhow::{Context, Result};
use codesearch_e2e_tests::common::*;
use codesearch_e2e_tests::graph_validation::{
    aggregate_imports_to_module_level, compare, generate_scip_index, parse_scip_relationships,
    query_all_relationships, write_report,
};

/// Test repository: a small, stable Rust project
const TEST_REPO_URL: &str = "https://github.com/dtolnay/anyhow.git";
const TEST_REPO_REF: &str = "f2b963a759decf0828efb58a8fdd417fb12f71fb"; // 1.0.99
const TEST_PACKAGE_NAME: &str = "anyhow";

/// Clone a test repository to a temporary directory with unique suffix.
fn clone_test_repo(suffix: &str) -> Result<PathBuf> {
    let repo_dir = PathBuf::from(format!("/tmp/graph-validation-test-repo-{suffix}"));

    // Clean up existing directory
    if repo_dir.exists() {
        std::fs::remove_dir_all(&repo_dir).context("Failed to clean up existing test repo")?;
    }

    println!("Cloning test repository: {TEST_REPO_URL} @ {TEST_REPO_REF}");

    // Clone the repository
    let repo = RepoBuilder::new()
        .clone(TEST_REPO_URL, &repo_dir)
        .context("Failed to clone repository")?;

    // Checkout the specific commit
    let oid = git2::Oid::from_str(TEST_REPO_REF).context("Invalid commit hash")?;
    let commit = repo.find_commit(oid).context("Failed to find commit")?;
    repo.checkout_tree(commit.as_object(), None)
        .context("Failed to checkout commit")?;
    repo.set_head_detached(oid)
        .context("Failed to set HEAD to commit")?;

    // Verify Cargo.toml exists
    if !repo_dir.join("Cargo.toml").exists() {
        anyhow::bail!("Cloned repo should have Cargo.toml");
    }

    Ok(repo_dir)
}

/// Create a test config file with Neo4j support
fn create_test_config_with_neo4j(
    repo_path: &Path,
    qdrant: &TestQdrant,
    postgres: &TestPostgres,
    neo4j: &TestNeo4j,
    db_name: &str,
) -> Result<PathBuf> {
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
neo4j_bolt_port = {}
neo4j_http_port = {}
neo4j_user = "neo4j"
neo4j_password = ""

[embeddings]
provider = "mock"

[watcher]
debounce_ms = 500
ignore_patterns = ["*.log", "target", ".git"]

[languages]
enabled = ["rust"]
"#,
        qdrant.port(),
        qdrant.rest_port(),
        postgres.port(),
        db_name,
        neo4j.bolt_port(),
        neo4j.http_port()
    );

    let config_path = repo_path.join("codesearch.toml");
    std::fs::write(&config_path, config_content)?;
    Ok(config_path)
}

/// Get repository ID from postgres
async fn get_repository_id(
    postgres: &Arc<TestPostgres>,
    db_name: &str,
    repo_path: &Path,
) -> Result<String> {
    let connection_url = format!(
        "postgresql://codesearch:codesearch@localhost:{}/{}",
        postgres.port(),
        db_name
    );

    let pool = sqlx::PgPool::connect(&connection_url)
        .await
        .context("Failed to connect to postgres")?;

    // Query for repository by path (matching the end of the path)
    let repo_path_str = repo_path.to_string_lossy();
    let result: Option<(String,)> = sqlx::query_as(
        "SELECT repository_id::text FROM repositories WHERE repository_path LIKE $1 LIMIT 1",
    )
    .bind(format!("%{}", repo_path_str.rsplit('/').next().unwrap_or(&repo_path_str)))
    .fetch_optional(&pool)
    .await?;

    pool.close().await;

    result
        .map(|(id,)| id)
        .ok_or_else(|| anyhow::anyhow!("Repository not found in database"))
}

/// Main graph validation test
///
/// This test:
/// 1. Clones a test repository
/// 2. Generates SCIP index using rust-analyzer
/// 3. Runs codesearch indexing pipeline
/// 4. Queries Neo4j for extracted relationships
/// 5. Compares against SCIP ground truth
/// 6. Writes a detailed report to logs/
#[tokio::test]
#[ignore] // Requires Docker + rust-analyzer
async fn test_graph_extraction_accuracy() -> Result<()> {
    init_test_logging();

    println!("\n=== Graph Validation Test ===\n");

    // 1. Start infrastructure
    println!("Starting infrastructure...");
    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;
    let neo4j = get_shared_neo4j().await?;
    let db_name = create_test_database(&postgres).await?;
    println!("  Database: {db_name}");

    // 2. Clone test repository
    println!("Cloning test repository...");
    let repo_path = clone_test_repo("full")?;
    println!("  Path: {}", repo_path.display());

    // Generate collection name
    let collection_name = codesearch_core::config::StorageConfig::generate_collection_name(&repo_path)?;
    println!("  Collection: {collection_name}");

    // 3. Generate SCIP ground truth
    println!("Generating SCIP index (this may take a while)...");
    let scip_path = match generate_scip_index(&repo_path) {
        Ok(path) => {
            println!("  SCIP index: {}", path.display());
            path
        }
        Err(e) => {
            println!("  SCIP generation failed: {e}");
            println!("  Is rust-analyzer installed? Try: rustup component add rust-analyzer");
            // Clean up and return error
            drop_test_database(&postgres, &db_name).await?;
            return Err(e);
        }
    };

    println!("Parsing SCIP relationships...");
    let ground_truth = parse_scip_relationships(&scip_path, TEST_PACKAGE_NAME)?;
    println!("  Ground truth relationships: {}", ground_truth.len());

    // 4. Create config and run indexing
    println!("Running codesearch indexing...");
    create_test_config_with_neo4j(&repo_path, &qdrant, &postgres, &neo4j, &db_name)?;

    let output = run_cli_with_full_infra(
        &repo_path,
        &["index"],
        &qdrant,
        &postgres,
        &neo4j,
        &db_name,
    )?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("  Indexing failed: {stderr}");
        drop_test_database(&postgres, &db_name).await?;
        anyhow::bail!("Indexing failed: {stderr}");
    }
    println!("  Indexing complete");

    // 5. Wait for graph_ready flag (index command already drained the outbox)
    println!("Waiting for graph resolution...");
    wait_for_graph_ready(&postgres, &db_name, Duration::from_secs(120)).await?;
    println!("  Graph ready");

    // 6. Get repository ID and query Neo4j
    println!("Querying Neo4j for extracted relationships...");
    let repo_id = get_repository_id(&postgres, &db_name, &repo_path).await?;
    println!("  Repository ID: {repo_id}");

    let extracted = query_all_relationships(&neo4j, &repo_id).await?;
    println!("  Extracted relationships: {}", extracted.len());

    // Also get graph stats for additional context
    let stats = get_neo4j_graph_stats(&neo4j, &repo_id).await?;
    stats.print_summary();

    // 7. Compare and report
    println!("\nComparing graphs...");

    // Aggregate IMPORTS to module level for comparison with SCIP
    // SCIP tracks module→module imports while we track entity→entity
    println!("Aggregating IMPORTS to module level for SCIP comparison...");
    let ground_truth_aggregated = aggregate_imports_to_module_level(&ground_truth);
    let extracted_aggregated = aggregate_imports_to_module_level(&extracted);
    println!(
        "  Ground truth: {} -> {} relationships",
        ground_truth.len(),
        ground_truth_aggregated.len()
    );
    println!(
        "  Extracted: {} -> {} relationships",
        extracted.len(),
        extracted_aggregated.len()
    );

    let result = compare(&ground_truth_aggregated, &extracted_aggregated);

    // Print summary to stdout
    result.print_summary();

    // Write detailed report to logs/
    let log_path = write_report(&result, "anyhow")?;
    println!("\nReport written to: {}", log_path.display());

    // 8. Cleanup
    println!("\nCleaning up...");
    drop_test_collection(&qdrant, &collection_name).await?;
    drop_test_database(&postgres, &db_name).await?;

    // Clean up cloned repo
    let _ = std::fs::remove_dir_all(&repo_path);
    // Clean up SCIP index
    let _ = std::fs::remove_file(&scip_path);

    println!("Done!");

    Ok(())
}

/// Test SCIP parsing in isolation (faster, no infrastructure needed)
#[tokio::test]
#[ignore] // Requires rust-analyzer
async fn test_scip_parsing_only() -> Result<()> {
    println!("\n=== SCIP Parsing Test ===\n");

    // Clone test repository
    let repo_path = clone_test_repo("scip-only")?;
    println!("Repository: {}", repo_path.display());

    // Generate SCIP index
    println!("Generating SCIP index...");
    let scip_path = generate_scip_index(&repo_path)?;
    println!("SCIP index: {}", scip_path.display());

    // Parse relationships
    println!("Parsing relationships...");
    let relationships = parse_scip_relationships(&scip_path, TEST_PACKAGE_NAME)?;
    println!("Total relationships: {}", relationships.len());

    // Count by type
    use codesearch_e2e_tests::graph_validation::RelationshipType;
    use std::collections::HashMap;

    let mut counts: HashMap<RelationshipType, usize> = HashMap::new();
    for rel in &relationships {
        *counts.entry(rel.relationship_type).or_insert(0) += 1;
    }

    println!("\nRelationships by type:");
    for rel_type in RelationshipType::all() {
        if let Some(count) = counts.get(rel_type) {
            println!("  {}: {}", rel_type, count);
        }
    }

    // Print sample relationships
    println!("\nSample relationships:");
    for rel in relationships.iter().take(10) {
        println!(
            "  {} --[{}]--> {}",
            rel.source.qualified_name(), rel.relationship_type, rel.target.qualified_name()
        );
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&repo_path);
    let _ = std::fs::remove_file(&scip_path);

    Ok(())
}
