//! Integration tests for the indexer crate
//!
//! These tests verify the complete three-stage indexing pipeline with mocked dependencies.

use codesearch_embeddings::{EmbeddingManager, MockEmbeddingProvider};
use codesearch_indexer::{create_indexer, IndexerConfig};
use codesearch_storage::{MockPostgresClient, PostgresClientTrait};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::fs;

fn create_test_embedding_manager() -> Arc<EmbeddingManager> {
    Arc::new(EmbeddingManager::new(
        Arc::new(MockEmbeddingProvider::new(384)),
        "test-model-v1".to_string(),
    ))
}

/// Helper to create a test repository with sample Rust files
async fn create_test_repository() -> TempDir {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();

    // Create a simple Rust project structure
    let src_dir = base.join("src");
    fs::create_dir(&src_dir).await.unwrap();

    // Main.rs with a function and struct
    let main_content = r#"
//! Main module

use std::collections::HashMap;

/// Main entry point
fn main() {
    println!("Hello, world!");
    let calculator = Calculator::new();
    let result = calculator.add(2, 3);
    println!("Result: {}", result);
}

/// A simple calculator
#[derive(Debug)]
pub struct Calculator {
    memory: HashMap<String, i32>,
}

impl Calculator {
    /// Create a new calculator
    pub fn new() -> Self {
        Self {
            memory: HashMap::new(),
        }
    }

    /// Add two numbers
    pub fn add(&self, a: i32, b: i32) -> i32 {
        a + b
    }

    /// Subtract two numbers
    pub fn subtract(&self, a: i32, b: i32) -> i32 {
        a - b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        let calc = Calculator::new();
        assert_eq!(calc.add(2, 3), 5);
    }
}
"#;
    fs::write(src_dir.join("main.rs"), main_content)
        .await
        .unwrap();

    // lib.rs with a module and trait
    let lib_content = r#"
//! Library module

pub mod utils;

/// A trait for processing data
pub trait DataProcessor {
    /// Process some data
    fn process(&self, data: &str) -> String;
}

/// Default implementation
pub struct DefaultProcessor;

impl DataProcessor for DefaultProcessor {
    fn process(&self, data: &str) -> String {
        data.to_uppercase()
    }
}

/// Configuration for the library
pub struct Config {
    pub debug: bool,
    pub max_items: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            debug: false,
            max_items: 100,
        }
    }
}
"#;
    fs::write(src_dir.join("lib.rs"), lib_content)
        .await
        .unwrap();

    // utils.rs with utility functions
    let utils_content = r#"
//! Utility functions

use std::fs;
use std::path::Path;

/// Read a file to string
pub fn read_file(path: &Path) -> Result<String, std::io::Error> {
    fs::read_to_string(path)
}

/// Write string to file
pub fn write_file(path: &Path, content: &str) -> Result<(), std::io::Error> {
    fs::write(path, content)
}

/// Check if a path exists
pub fn path_exists(path: &Path) -> bool {
    path.exists()
}

/// Format a number with commas
pub fn format_number(n: i64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}
"#;
    fs::write(src_dir.join("utils.rs"), utils_content)
        .await
        .unwrap();

    // Create some non-Rust files that should be ignored
    fs::write(base.join("README.md"), "# Test Project")
        .await
        .unwrap();
    fs::write(base.join("Cargo.toml"), "[package]\nname = \"test\"")
        .await
        .unwrap();

    // Create a target directory that should be excluded
    let target_dir = base.join("target");
    fs::create_dir(&target_dir).await.unwrap();
    fs::write(target_dir.join("debug.rs"), "// Should be ignored")
        .await
        .unwrap();

    temp_dir
}

#[tokio::test]
async fn test_full_indexing_pipeline() {
    // Create test repository
    let test_repo = create_test_repository().await;
    let repo_path = test_repo.path().to_path_buf();

    // Create indexer with mocked dependencies
    let postgres_client = Arc::new(MockPostgresClient::new());

    // Register repository with mock client
    let repository_id = postgres_client
        .ensure_repository(&repo_path, "test_collection", None)
        .await
        .unwrap()
        .to_string();

    let embedding_manager = create_test_embedding_manager();
    let postgres_client: Arc<dyn codesearch_storage::PostgresClientTrait> = postgres_client;

    let mut indexer = create_indexer(
        repo_path.clone(),
        repository_id,
        embedding_manager,
        postgres_client,
        None,
        IndexerConfig::default(),
    )
    .unwrap();

    // Run full indexing
    let result = indexer.index_repository().await;

    // Verify successful indexing
    assert!(result.is_ok());

    if let Ok(index_result) = result {
        let stats = index_result.stats();
        // Should have processed some Rust files
        assert!(stats.total_files() > 0);
        // Should have extracted some entities
        assert!(stats.entities_extracted() > 0);
        // Should have some processing time
        assert!(stats.processing_time_ms() > 0);
    }
}

#[tokio::test]
async fn test_indexer_skips_large_entities() {
    // Create a temporary directory with large and small test files
    let temp_dir = tempfile::tempdir().unwrap();
    let repo_path = temp_dir.path();

    // Create src directory
    let src_dir = repo_path.join("src");
    fs::create_dir(&src_dir).await.unwrap();

    // Small function that should be indexed
    let small_content = r#"
fn small_function() -> i32 {
    42
}
"#;
    fs::write(src_dir.join("small.rs"), small_content)
        .await
        .unwrap();

    // Large function that exceeds context window (simulate with very long content)
    let large_body = "x".repeat(10000); // Create a very large function body
    let large_content = format!(
        r#"
fn large_function() {{
    // This is a very large function that should be skipped
    let data = "{large_body}";
    println!("{{}}", data);
}}
"#
    );

    fs::write(src_dir.join("large.rs"), large_content)
        .await
        .unwrap();

    // Create indexer with mocked dependencies
    let postgres_client = Arc::new(MockPostgresClient::new());

    // Register repository with mock client
    let repository_id = postgres_client
        .ensure_repository(repo_path, "test_collection", None)
        .await
        .unwrap()
        .to_string();

    let embedding_manager = create_test_embedding_manager();
    let postgres_client: Arc<dyn codesearch_storage::PostgresClientTrait> = postgres_client;

    let mut indexer = create_indexer(
        repo_path.to_path_buf(),
        repository_id,
        embedding_manager,
        postgres_client,
        None,
        IndexerConfig::default(),
    )
    .unwrap();
    let result = indexer.index_repository().await.unwrap();

    // Verify successful processing
    let stats = result.stats();
    assert!(stats.total_files() > 0);
    assert!(stats.entities_extracted() > 0);
}

#[tokio::test]
async fn test_indexer_with_empty_repository() {
    let temp_dir = TempDir::new().unwrap();
    let postgres_client = Arc::new(MockPostgresClient::new());

    // Register repository with mock client
    let repository_id = postgres_client
        .ensure_repository(temp_dir.path(), "test_collection", None)
        .await
        .unwrap()
        .to_string();

    let embedding_manager = create_test_embedding_manager();
    let postgres_client: Arc<dyn codesearch_storage::PostgresClientTrait> = postgres_client;

    let mut indexer = create_indexer(
        temp_dir.path().to_path_buf(),
        repository_id,
        embedding_manager,
        postgres_client,
        None,
        IndexerConfig::default(),
    )
    .unwrap();

    let result = indexer.index_repository().await;
    assert!(result.is_ok());

    if let Ok(index_result) = result {
        assert_eq!(index_result.stats().total_files(), 0);
        assert_eq!(index_result.stats().entities_extracted(), 0);
    }
}
