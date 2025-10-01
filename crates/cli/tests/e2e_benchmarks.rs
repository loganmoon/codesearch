//! Performance benchmark tests for codesearch
//!
//! These tests are marked #[ignore] and run separately to measure performance.
//!
//! Run with: cargo test --test e2e_benchmarks -- --ignored --nocapture

mod e2e;

use anyhow::{Context, Result};
use e2e::*;
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use uuid::Uuid;

/// Get the workspace manifest path for cargo run commands
fn workspace_manifest() -> std::path::PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    Path::new(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Cargo.toml")
}

/// Create a test config file for the given repository and Qdrant instance
fn create_test_config(
    repo_path: &Path,
    qdrant: &TestQdrant,
    collection_name: Option<&str>,
) -> Result<std::path::PathBuf> {
    let config_content = format!(
        r#"
[indexer]

[storage]
qdrant_host = "localhost"
qdrant_port = {}
qdrant_rest_port = {}
collection_name = "{}"
auto_start_deps = false

[embeddings]
provider = "mock"

[watcher]
debounce_ms = 500
ignore_patterns = ["*.log", "target", ".git"]
branch_strategy = "index_current"

[languages]
enabled = ["rust"]
"#,
        qdrant.port(),
        qdrant.rest_port(),
        collection_name.unwrap_or("")
    );

    let config_path = repo_path.join("codesearch.toml");
    std::fs::write(&config_path, config_content)?;
    Ok(config_path)
}

/// Run the codesearch CLI with the given arguments
fn run_cli(repo_path: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("cargo")
        .current_dir(repo_path)
        .args([
            "run",
            "--manifest-path",
            workspace_manifest().to_str().unwrap(),
            "--package",
            "codesearch",
            "--",
        ])
        .args(args)
        .env("RUST_LOG", "warn") // Reduce noise in benchmarks
        .output()
        .context("Failed to run codesearch CLI")
}

#[tokio::test]
#[ignore]
async fn benchmark_indexing_speed() -> Result<()> {
    println!("\n=== Indexing Speed Benchmark ===\n");

    let qdrant = TestQdrant::start().await?;

    // Create a repository with multiple files
    let mut builder = TestRepositoryBuilder::new("benchmark");

    // Add 20 Rust files
    for i in 0..20 {
        let content = format!(
            r#"
//! Module {i}

pub struct Thing{i} {{
    value: i32,
}}

impl Thing{i} {{
    pub fn new(value: i32) -> Self {{
        Self {{ value }}
    }}

    pub fn get_value(&self) -> i32 {{
        self.value
    }}

    pub fn set_value(&mut self, value: i32) {{
        self.value = value;
    }}

    pub fn double(&self) -> i32 {{
        self.value * 2
    }}

    pub fn triple(&self) -> i32 {{
        self.value * 3
    }}
}}

pub fn process_{i}(x: i32) -> i32 {{
    x + {i}
}}

pub fn compute_{i}(a: i32, b: i32) -> i32 {{
    a * b + {i}
}}
"#
        );
        builder = builder.with_rust_file(&format!("module{i}.rs"), &content);
    }

    let repo = builder.build().await?;

    let collection_name = format!("benchmark_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, Some(&collection_name))?;

    // Measure init time
    let init_start = Instant::now();
    let init_output = run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    let init_duration = init_start.elapsed();

    assert!(init_output.status.success(), "Init failed");
    println!("Init time: {init_duration:?}");

    // Measure index time
    let index_start = Instant::now();
    let index_output = run_cli(repo.path(), &["index"])?;
    let index_duration = index_start.elapsed();

    assert!(index_output.status.success(), "Index failed");

    let stdout = String::from_utf8_lossy(&index_output.stdout);
    let stderr = String::from_utf8_lossy(&index_output.stderr);
    println!("Index time: {index_duration:?}");
    println!("stdout: {stdout}");
    println!("stderr: {stderr}");

    // Verify entities were indexed
    let point_count = get_point_count(&qdrant, &collection_name).await?;
    println!("Total entities indexed: {point_count}");

    if point_count > 0 {
        let entities_per_sec = point_count as f64 / index_duration.as_secs_f64();
        println!("Indexing rate: {entities_per_sec:.2} entities/sec");
    }

    println!("\n=== Benchmark Complete ===\n");

    Ok(())
}

#[tokio::test]
#[ignore]
async fn benchmark_search_latency() -> Result<()> {
    println!("\n=== Search Latency Benchmark ===\n");

    let qdrant = TestQdrant::start().await?;
    let repo = complex_rust_repo().await?;

    let collection_name = format!("benchmark_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, Some(&collection_name))?;

    // Init and index
    run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    run_cli(repo.path(), &["index"])?;

    println!("Repository indexed. Starting search benchmark...");

    // Perform 100 searches and measure latency
    let mut latencies = Vec::new();

    for i in 0..100 {
        let start = Instant::now();

        // Use Qdrant REST API for search
        let client = reqwest::Client::new();
        let search_url = format!(
            "{}/collections/{collection_name}/points/search",
            qdrant.rest_url()
        );

        // Create a dummy query vector (mock embedding)
        let query_vector = vec![0.1; 384];

        let search_body = serde_json::json!({
            "vector": query_vector,
            "limit": 5,
            "with_payload": true,
        });

        let response = client.post(&search_url).json(&search_body).send().await?;

        if !response.status().is_success() {
            eprintln!("Search {i} failed: {}", response.status());
            continue;
        }

        let duration = start.elapsed();
        latencies.push(duration);

        if i % 20 == 0 {
            println!("Completed {i} searches...");
        }
    }

    // Calculate statistics
    latencies.sort();
    let count = latencies.len();

    let p50 = latencies[count / 2];
    let p95 = latencies[(count * 95) / 100];
    let p99 = latencies[(count * 99) / 100];

    let total: std::time::Duration = latencies.iter().sum();
    let avg = total / count as u32;

    println!("\n=== Search Latency Results ===");
    println!("Total searches: {count}");
    println!("Average: {avg:?}");
    println!("p50: {p50:?}");
    println!("p95: {p95:?}");
    println!("p99: {p99:?}");
    println!("=== Benchmark Complete ===\n");

    Ok(())
}

#[tokio::test]
#[ignore]
async fn benchmark_large_repository() -> Result<()> {
    println!("\n=== Large Repository Benchmark ===\n");

    let qdrant = TestQdrant::start().await?;

    // Create a repository with 100 files
    let mut builder = TestRepositoryBuilder::new("large_benchmark");

    println!("Creating test repository with 100 files...");
    for i in 0..100 {
        let content = format!(
            r#"
//! File {i}

pub fn function_{i}_1(x: i32) -> i32 {{ x + {i} }}
pub fn function_{i}_2(x: i32) -> i32 {{ x * {i} }}

pub struct Struct{i} {{
    field: i32,
}}

impl Struct{i} {{
    pub fn method_{i}(&self) -> i32 {{ self.field }}
}}
"#
        );
        builder = builder.with_rust_file(&format!("file{i}.rs"), &content);
    }

    let repo = builder.build().await?;
    println!("Test repository created");

    let collection_name = format!("large_benchmark_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, Some(&collection_name))?;

    // Measure total pipeline time
    let total_start = Instant::now();

    let init_output = run_cli(
        repo.path(),
        &["init", "--config", config_path.to_str().unwrap()],
    )?;
    assert!(init_output.status.success(), "Init failed");

    let index_output = run_cli(repo.path(), &["index"])?;
    assert!(index_output.status.success(), "Index failed");

    let total_duration = total_start.elapsed();

    let point_count = get_point_count(&qdrant, &collection_name).await?;

    println!("\n=== Large Repository Results ===");
    println!("Files: 100");
    println!("Total entities: {point_count}");
    println!("Total time: {total_duration:?}");
    println!("Files/sec: {:.2}", 100.0 / total_duration.as_secs_f64());
    if point_count > 0 {
        println!(
            "Entities/sec: {:.2}",
            point_count as f64 / total_duration.as_secs_f64()
        );
    }
    println!("=== Benchmark Complete ===\n");

    Ok(())
}

/// Helper to get the point count from a collection
async fn get_point_count(qdrant: &TestQdrant, collection_name: &str) -> Result<usize> {
    let url = format!("{}/collections/{collection_name}", qdrant.rest_url());
    let response = reqwest::get(&url).await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("Failed to get collection info"));
    }

    let info: serde_json::Value = response.json().await?;
    let count = info["result"]["points_count"]
        .as_u64()
        .context("Failed to get points_count")? as usize;

    Ok(count)
}
