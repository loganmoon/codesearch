//! Collect evaluation candidates for manual labeling
//!
//! This test runs hybrid search for each evaluation query and collects the top 50 results
//! with entity details for manual labeling.
//!
//! Run with: cargo test --package codesearch-e2e-tests --test test_collect_evaluation_candidates -- --ignored --nocapture

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use anyhow::{Context, Result};
use codesearch_core::{
    config::{global_config_path, Config},
    CodeEntity,
};
use codesearch_embeddings::{
    create_embedding_manager_from_app_config, Bm25SparseProvider, SparseEmbeddingProvider,
};
use codesearch_indexer::entity_processor::extract_embedding_content;
use codesearch_storage::{create_postgres_client, create_storage_client};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, Row};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct EvaluationQuery {
    query: String,
    query_type: String,
    category: String,
}

#[derive(Debug, Serialize)]
struct Candidate {
    entity_id: String,
    entity_name: String,
    entity_type: String,
    file_path: String,
    code_snippet: String,
    score: f32,
    rank: usize,
}

#[derive(Debug, Serialize)]
struct QueryWithCandidates {
    query: String,
    query_type: String,
    category: String,
    candidates: Vec<Candidate>,
}

async fn setup_clap_repository(config: &Config) -> Result<(std::path::PathBuf, Uuid, String)> {
    const CLAP_VERSION: &str = "v4.5.0";
    const CLAP_REPO_URL: &str = "https://github.com/clap-rs/clap.git";

    let repo_path = std::path::PathBuf::from(format!("/tmp/clap-eval-{CLAP_VERSION}"));

    println!("\n=== Setting up clap repository for evaluation ===");
    println!("Repository path: {}", repo_path.display());
    println!("Version: {CLAP_VERSION}");

    // Check if repository needs to be cloned or updated
    if !repo_path.exists() {
        println!("Cloning clap repository...");
        let output = std::process::Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                "--branch",
                CLAP_VERSION,
                CLAP_REPO_URL,
                repo_path.to_str().unwrap(),
            ])
            .output()
            .context("Failed to clone clap repository")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to clone clap: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        println!("✓ Cloned clap {CLAP_VERSION}");
    } else {
        println!(
            "✓ clap repository already exists at {}",
            repo_path.display()
        );
    }

    // Check if repository is already indexed
    let postgres_client = create_postgres_client(&config.storage).await?;

    match postgres_client.get_repository_by_path(&repo_path).await? {
        Some((repository_id, collection_name)) => {
            println!("✓ clap is already indexed");
            println!("  Repository ID: {repository_id}");
            println!("  Collection: {collection_name}");

            // Verify entity count
            let count_result = postgres_client
                .get_pool()
                .fetch_one(
                    sqlx::query(
                        "SELECT COUNT(*) as count FROM entity_metadata WHERE repository_id = $1",
                    )
                    .bind(repository_id),
                )
                .await?;
            let entity_count: i64 = count_result.try_get("count")?;

            println!("  Entities indexed: {entity_count}");

            if entity_count == 0 {
                anyhow::bail!(
                    "Repository is indexed but has 0 entities. Please re-index: \
                     cd {} && codesearch index",
                    repo_path.display()
                );
            }

            Ok((repo_path, repository_id, collection_name))
        }
        None => {
            anyhow::bail!(
                "clap repository is not indexed. Please index it first:\n  \
                 cd {} && codesearch index",
                repo_path.display()
            );
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_collect_evaluation_candidates() -> Result<()> {
    println!("\n=== Collecting Evaluation Candidates for Labeling ===\n");

    let config_path = global_config_path()?;
    let config = Config::from_file(&config_path)?;

    // Use clap-eval repository
    let (repo_path, repository_id, collection_name) = setup_clap_repository(&config).await?;

    println!("Repository: {}", repo_path.display());
    println!("Repository ID: {repository_id}");
    println!("Collection: {collection_name}\n");

    // Load evaluation queries
    let queries_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/evaluation_queries.json");
    let queries_json = std::fs::read_to_string(&queries_path)?;
    let evaluation_queries: Vec<EvaluationQuery> = serde_json::from_str(&queries_json)?;

    println!("Loaded {} evaluation queries\n", evaluation_queries.len());

    // Create clients
    let postgres_client = create_postgres_client(&config.storage).await?;
    let storage_client = create_storage_client(&config.storage, &collection_name).await?;
    let embedding_manager = create_embedding_manager_from_app_config(&config.embeddings).await?;

    // Get BM25 stats for sparse embeddings
    let bm25_stats = postgres_client
        .get_bm25_statistics(repository_id)
        .await
        .context("Failed to fetch BM25 statistics")?;
    let sparse_provider = Bm25SparseProvider::new(bm25_stats.avgdl);

    let bge_instruction = config.embeddings.default_bge_instruction.clone();
    let prefetch_multiplier = config.hybrid_search.prefetch_multiplier;

    let mut queries_with_candidates = Vec::new();

    // Run hybrid search for each query and collect top 50 candidates
    for (i, query) in evaluation_queries.iter().enumerate() {
        println!(
            "[{}/{}] Processing: \"{}\"",
            i + 1,
            evaluation_queries.len(),
            query.query
        );

        // Generate dense embedding
        let formatted_query = format!("<instruct>{}\n<query>{}", bge_instruction, query.query);
        let embeddings = embedding_manager
            .embed(vec![formatted_query])
            .await
            .context("Failed to generate embedding")?;

        let query_embedding = embeddings
            .into_iter()
            .next()
            .flatten()
            .context("Failed to generate embedding")?;

        // Generate sparse embedding
        let sparse_embeddings = sparse_provider
            .embed_sparse(vec![query.query.as_str()])
            .await
            .context("Failed to generate sparse embedding")?;

        let sparse_embedding = sparse_embeddings
            .into_iter()
            .next()
            .flatten()
            .context("Failed to generate sparse embedding")?;

        // Run hybrid search with limit=50
        let search_results = storage_client
            .search_similar_hybrid(
                query_embedding,
                sparse_embedding,
                50,
                None,
                prefetch_multiplier,
            )
            .await
            .context("Hybrid search failed")?;

        // Fetch entity details
        let entity_refs: Vec<_> = search_results
            .iter()
            .map(|(eid, _, _)| (repository_id, eid.to_string()))
            .collect();

        let entities = postgres_client.get_entities_by_ids(&entity_refs).await?;
        let entity_map: HashMap<String, &CodeEntity> =
            entities.iter().map(|e| (e.entity_id.clone(), e)).collect();

        // Build candidates with entity details
        let candidates: Vec<Candidate> = search_results
            .iter()
            .enumerate()
            .filter_map(|(rank, (entity_id, _, score))| {
                entity_map.get(entity_id).map(|entity| {
                    let content = extract_embedding_content(entity);
                    Candidate {
                        entity_id: entity_id.clone(),
                        entity_name: entity.name.clone(),
                        entity_type: format!("{:?}", entity.entity_type),
                        file_path: entity.file_path.display().to_string(),
                        code_snippet: content,
                        score: *score,
                        rank: rank + 1,
                    }
                })
            })
            .collect();

        println!("  Collected {} candidates\n", candidates.len());

        queries_with_candidates.push(QueryWithCandidates {
            query: query.query.clone(),
            query_type: query.query_type.clone(),
            category: query.category.clone(),
            candidates,
        });
    }

    // Save to labeling format
    let output_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/evaluation_candidates.json");
    let output_json = serde_json::to_string_pretty(&queries_with_candidates)?;
    std::fs::write(&output_path, output_json)?;

    println!("\n======================================================================");
    println!("Data collection complete!");
    println!(
        "Collected {} queries with candidates",
        queries_with_candidates.len()
    );
    println!("Data saved to: {}", output_path.display());
    println!("\nNext steps:");
    println!("  1. Review each query and candidate");
    println!("  2. Label each candidate as 0 (not helpful) or 1 (helpful)");
    println!("  3. Save labeled data as data/ground_truth_evaluation.json");
    println!("======================================================================\n");

    Ok(())
}
