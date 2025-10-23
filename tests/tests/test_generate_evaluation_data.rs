//! Generate evaluation dataset for hybrid search
//!
//! This test generates 100 queries and collects search results for manual labeling.
//!
//! Run with: cargo test --package codesearch-e2e-tests --test test_generate_evaluation_data -- --ignored --nocapture

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use anyhow::{Context, Result};
use codesearch_core::{
    config::{global_config_path, Config},
    CodeEntity,
};
use codesearch_storage::{create_postgres_client, PostgresClientTrait};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct EvaluationQuery {
    query: String,
    query_type: String, // "realistic" or "entity_focused"
    category: String,
}

/// Generate 50 entity-focused queries from actual codebase entities
async fn generate_entity_focused_queries(
    postgres_client: &dyn PostgresClientTrait,
    repository_id: Uuid,
) -> Result<Vec<EvaluationQuery>> {
    println!("Fetching entities from PostgreSQL...");

    let pool = postgres_client.get_pool();

    // Get a diverse sample of entities
    let entities: Vec<CodeEntity> = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT entity_data
         FROM entity_metadata
         WHERE repository_id = $1
           AND deleted_at IS NULL
         ORDER BY RANDOM()
         LIMIT 200",
    )
    .bind(repository_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .filter_map(|v| serde_json::from_value(v).ok())
    .collect();

    println!("Fetched {} random entities", entities.len());

    let mut queries = Vec::new();

    // Generate diverse query types
    for entity in entities.iter().take(50) {
        let entity_type_str = format!("{:?}", entity.entity_type);

        let query = match entity.entity_type {
            codesearch_core::EntityType::Struct => {
                format!("find the {} struct", entity.name)
            }
            codesearch_core::EntityType::Enum => {
                format!("find the {} enum", entity.name)
            }
            codesearch_core::EntityType::Trait => {
                format!("find the {} trait", entity.name)
            }
            codesearch_core::EntityType::Function => {
                format!("find the {} function", entity.name)
            }
            codesearch_core::EntityType::Method => {
                format!("find the {} method", entity.name)
            }
            codesearch_core::EntityType::Impl => {
                format!("find implementation of {}", entity.name)
            }
            _ => {
                format!("find {}", entity.name)
            }
        };

        queries.push(EvaluationQuery {
            query,
            query_type: "entity_focused".to_string(),
            category: entity_type_str.to_lowercase(),
        });
    }

    println!("Generated {} entity-focused queries", queries.len());
    Ok(queries)
}

/// Load existing realistic queries
fn load_realistic_queries() -> Result<Vec<EvaluationQuery>> {
    #[derive(Deserialize)]
    struct QueryEntry {
        query: String,
        category: String,
    }

    #[derive(Deserialize)]
    struct RealisticQueries {
        queries: Vec<QueryEntry>,
    }

    let queries_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/realistic_queries.json");
    let content = std::fs::read_to_string(&queries_path)?;
    let realistic_queries: RealisticQueries = serde_json::from_str(&content)?;

    Ok(realistic_queries
        .queries
        .into_iter()
        .map(|entry| EvaluationQuery {
            query: entry.query,
            query_type: "realistic".to_string(),
            category: entry.category,
        })
        .collect())
}

#[tokio::test]
#[ignore]
async fn test_generate_evaluation_data() -> Result<()> {
    println!("\n=== Generating Evaluation Dataset ===\n");

    let config_path = global_config_path()?;
    let config = Config::from_file(&config_path)?;

    // Use clap-eval repository
    let repo_path = Path::new("/tmp/clap-eval-v4.5.0");

    let postgres_client = create_postgres_client(&config.storage).await?;
    let (repository_id, _collection_name) = postgres_client
        .get_repository_by_path(repo_path)
        .await?
        .context("clap-eval repository not indexed")?;

    println!("Repository ID: {repository_id}\n");

    // Step 1: Generate queries
    println!("Step 1: Generating 100 queries...");
    let realistic = load_realistic_queries()?;
    let entity_focused =
        generate_entity_focused_queries(postgres_client.as_ref(), repository_id).await?;

    let mut all_queries = realistic;
    all_queries.extend(entity_focused);

    println!("Total queries: {}\n", all_queries.len());

    // Save queries
    let queries_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/evaluation_queries.json");
    let queries_json = serde_json::to_string_pretty(&all_queries)?;
    std::fs::write(&queries_path, queries_json)?;

    println!("Saved queries to: {}\n", queries_path.display());
    println!("Next: Run test_collect_evaluation_candidates to collect search results");

    Ok(())
}
