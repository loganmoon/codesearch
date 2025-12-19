//! End-to-end resolution benchmark for the codesearch pipeline
//!
//! This benchmark evaluates the complete resolution pipeline against real open-source
//! codebases, producing a summary report of extraction and resolution quality.
//!
//! ## Running
//!
//! ```bash
//! cargo test --manifest-path crates/e2e-tests/Cargo.toml --test test_resolution_e2e -- --ignored --nocapture
//! ```

use anyhow::{Context, Result};
use codesearch_e2e_tests::common::*;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Results from benchmarking a single codebase
#[derive(Debug)]
struct CodebaseResult {
    name: String,
    language: String,
    indexing_time: Duration,
    entity_count: usize,
    relationships: RelationshipCounts,
    /// Count of External stub nodes (unresolved external references)
    external_count: usize,
    /// Resolution rate: resolved / (resolved + external) * 100
    internal_resolution_rate: f64,
    /// Relationship density: relationships per entity
    relationship_density: f64,
    error: Option<String>,
}

#[derive(Debug, Default)]
struct RelationshipCounts {
    contains: usize,
    implements: usize,
    calls: usize,
    imports: usize,
    uses: usize,
    inherits: usize,
    associates: usize,
    extends_interface: usize,
}

impl RelationshipCounts {
    fn total(&self) -> usize {
        self.contains
            + self.implements
            + self.calls
            + self.imports
            + self.uses
            + self.inherits
            + self.associates
            + self.extends_interface
    }
}

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
ignore_patterns = ["*.log", "target", ".git"]

[languages]
enabled = ["rust", "python", "typescript", "javascript"]
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

/// Benchmark a single codebase
async fn benchmark_codebase<F, Fut>(
    name: &str,
    language: &str,
    clone_fn: F,
    qdrant: &Arc<TestQdrant>,
    postgres: &Arc<TestPostgres>,
    neo4j: &Arc<TestNeo4j>,
) -> CodebaseResult
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<tempfile::TempDir>>,
{
    let mut result = CodebaseResult {
        name: name.to_string(),
        language: language.to_string(),
        indexing_time: Duration::ZERO,
        entity_count: 0,
        relationships: RelationshipCounts::default(),
        external_count: 0,
        internal_resolution_rate: 0.0,
        relationship_density: 0.0,
        error: None,
    };

    // Clone the repository
    let repo = match clone_fn().await {
        Ok(r) => r,
        Err(e) => {
            result.error = Some(format!("Clone failed: {e}"));
            return result;
        }
    };

    // Create isolated database
    let db_name = match create_test_database(postgres).await {
        Ok(name) => name,
        Err(e) => {
            result.error = Some(format!("Database creation failed: {e}"));
            return result;
        }
    };

    // Create config
    if let Err(e) = create_test_config(repo.path(), qdrant, postgres, neo4j, &db_name) {
        result.error = Some(format!("Config creation failed: {e}"));
        let _ = drop_test_database(postgres, &db_name).await;
        return result;
    }

    // Run indexing
    let start = Instant::now();
    let output = match run_cli_with_full_infra(
        repo.path(),
        &["index"],
        qdrant,
        postgres,
        neo4j,
        &db_name,
    ) {
        Ok(o) => o,
        Err(e) => {
            result.error = Some(format!("CLI execution failed: {e}"));
            let _ = drop_test_database(postgres, &db_name).await;
            return result;
        }
    };
    result.indexing_time = start.elapsed();

    if !output.status.success() {
        result.error = Some(format!(
            "Indexing failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
        let _ = drop_test_database(postgres, &db_name).await;
        return result;
    }

    // Wait for graph resolution
    if let Err(e) = wait_for_graph_ready(postgres, &db_name, Duration::from_secs(60)).await {
        result.error = Some(format!("Graph resolution timeout: {e}"));
        let _ = drop_test_database(postgres, &db_name).await;
        return result;
    }

    // Additional delay to ensure Neo4j writes are fully committed
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Get repository ID
    let repo_id = match get_repository_id(postgres, &db_name).await {
        Ok(id) => id,
        Err(e) => {
            result.error = Some(format!("Failed to get repository ID: {e}"));
            let _ = drop_test_database(postgres, &db_name).await;
            return result;
        }
    };

    // Collect metrics from Neo4j
    if let Ok(stats) = get_neo4j_graph_stats(neo4j, &repo_id).await {
        result.entity_count = stats.node_count;
        result.external_count = stats.external_edge_count;
        result.internal_resolution_rate = stats.internal_resolution_rate();
        result.relationship_density = stats.relationship_density();
        result.relationships = RelationshipCounts {
            contains: stats.contains_count,
            implements: stats.implements_count,
            calls: stats.calls_count,
            imports: stats.imports_count,
            uses: stats.uses_count,
            inherits: stats.inherits_count,
            associates: stats.associates_count,
            extends_interface: stats.extends_interface_count,
        };

        // Debug: list entities for small codebases
        if stats.node_count < 50 && stats.node_count > 0 {
            if let Ok(entities) = list_neo4j_entities(neo4j, &repo_id).await {
                println!("\n  Entities in {}:", name);
                for (ent_name, ent_type) in entities.iter().take(20) {
                    println!("    - {} ({})", ent_name, ent_type);
                }
            }
        }
    }

    // Cleanup
    let _ = drop_test_database(postgres, &db_name).await;

    result
}

/// Print the benchmark report
fn print_report(results: &[CodebaseResult]) {
    println!("\n{}", "=".repeat(100));
    println!("                              RESOLUTION BENCHMARK REPORT");
    println!("{}\n", "=".repeat(100));

    // Summary table header
    println!(
        "{:<15} {:>8} {:>8} {:>10} {:>10} {:>10} {:>10}",
        "Codebase", "Language", "Entities", "Resolved", "External", "Rate", "Density"
    );
    println!("{:-<100}", "");

    let mut total_entities = 0;
    let mut total_resolved = 0;
    let mut total_external = 0;
    let mut failed = 0;

    for r in results {
        if let Some(ref err) = r.error {
            println!("{:<15} {:>8} FAILED: {}", r.name, r.language, err);
            failed += 1;
        } else {
            let resolved = r.relationships.total();
            println!(
                "{:<15} {:>8} {:>8} {:>10} {:>10} {:>9.1}% {:>9.2}",
                r.name,
                r.language,
                r.entity_count,
                resolved,
                r.external_count,
                r.internal_resolution_rate,
                r.relationship_density
            );
            total_entities += r.entity_count;
            total_resolved += resolved;
            total_external += r.external_count;
        }
    }

    println!("{:-<100}", "");

    let overall_rate = if total_resolved + total_external > 0 {
        (total_resolved as f64 / (total_resolved + total_external) as f64) * 100.0
    } else {
        100.0
    };
    let overall_density = if total_entities > 0 {
        total_resolved as f64 / total_entities as f64
    } else {
        0.0
    };

    println!(
        "{:<15} {:>8} {:>8} {:>10} {:>10} {:>9.1}% {:>9.2}",
        "TOTAL", "", total_entities, total_resolved, total_external, overall_rate, overall_density
    );
    println!(
        "\nNote: 'External' = unresolved external references (External stub nodes in Neo4j)"
    );
    println!("      'Rate' = resolved / (resolved + external) * 100");
    println!("      'Density' = relationships per entity");

    // Detailed breakdown - now includes all relationship types
    println!("\n\nRELATIONSHIP BREAKDOWN BY CODEBASE");
    println!("{:-<110}", "");
    println!(
        "{:<15} {:>8} {:>8} {:>8} {:>8} {:>8} {:>10} {:>8} {:>10}",
        "Codebase", "CONTAINS", "IMPLS", "CALLS", "IMPORTS", "USES", "INHERITS", "ASSOC", "EXT_IFACE"
    );
    println!("{:-<110}", "");

    for r in results {
        if r.error.is_none() {
            println!(
                "{:<15} {:>8} {:>8} {:>8} {:>8} {:>8} {:>10} {:>8} {:>10}",
                r.name,
                r.relationships.contains,
                r.relationships.implements,
                r.relationships.calls,
                r.relationships.imports,
                r.relationships.uses,
                r.relationships.inherits,
                r.relationships.associates,
                r.relationships.extends_interface
            );
        }
    }

    // Timing
    println!("\n\nINDEXING TIMES");
    println!("{:-<80}", "");
    for r in results {
        if r.error.is_none() {
            println!("{:<25} {:>10.2}s", r.name, r.indexing_time.as_secs_f64());
        }
    }

    // Summary
    println!("\n\n{}", "=".repeat(100));
    println!("SUMMARY");
    println!("{}", "=".repeat(100));
    println!("  Codebases tested: {}", results.len());
    println!("  Successful: {}", results.len() - failed);
    println!("  Failed: {}", failed);
    println!("  Total entities extracted: {}", total_entities);
    println!("  Total relationships resolved: {}", total_resolved);
    println!("  External references (unresolved): {}", total_external);
    println!("  Internal resolution rate: {:.1}%", overall_rate);
    println!("  Relationship density: {:.2} rels/entity", overall_density);
    println!("{}\n", "=".repeat(100));
}

#[tokio::test]
#[ignore] // Requires Docker and network access
async fn resolution_benchmark() -> Result<()> {
    init_test_logging();

    println!("\nStarting resolution benchmark...\n");

    // Start infrastructure
    println!("Starting test infrastructure...");
    let qdrant = get_shared_qdrant().await?;
    let postgres = get_shared_postgres().await?;
    let neo4j = get_shared_neo4j().await?;
    println!("Infrastructure ready.\n");

    let mut results = Vec::new();

    // Rust: anyhow
    println!("Benchmarking anyhow (Rust)...");
    results.push(
        benchmark_codebase(
            "anyhow",
            "Rust",
            real_rust_crate_anyhow,
            &qdrant,
            &postgres,
            &neo4j,
        )
        .await,
    );

    // Rust: thiserror
    println!("Benchmarking thiserror (Rust)...");
    results.push(
        benchmark_codebase(
            "thiserror",
            "Rust",
            real_rust_crate_thiserror,
            &qdrant,
            &postgres,
            &neo4j,
        )
        .await,
    );

    // Python: python-dotenv
    println!("Benchmarking python-dotenv (Python)...");
    results.push(
        benchmark_codebase(
            "python-dotenv",
            "Python",
            real_python_package,
            &qdrant,
            &postgres,
            &neo4j,
        )
        .await,
    );

    // TypeScript: jotai
    println!("Benchmarking jotai (TypeScript)...");
    results.push(
        benchmark_codebase(
            "jotai",
            "TypeScript",
            real_jotai_project,
            &qdrant,
            &postgres,
            &neo4j,
        )
        .await,
    );

    // JavaScript: Express
    println!("Benchmarking express (JavaScript)...");
    results.push(
        benchmark_codebase(
            "express",
            "JavaScript",
            real_express_project,
            &qdrant,
            &postgres,
            &neo4j,
        )
        .await,
    );

    // Print final report
    print_report(&results);

    Ok(())
}
