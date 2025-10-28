//! End-to-end test infrastructure
//!
//! This module provides utilities for E2E testing of the complete codesearch pipeline.

#![allow(dead_code)]
#![allow(unused_imports)]

pub mod assertions;
pub mod cleanup;
pub mod containers;
pub mod fixtures;
pub mod logging;

use anyhow::{Context, Result};
use std::future::Future;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

// Re-export key types and utilities
pub use assertions::*;
pub use containers::*;
pub use fixtures::*;
pub use logging::*;

/// Wrap a test future with a timeout
///
/// Prevents tests from hanging indefinitely by adding a timeout.
/// Returns an error if the future doesn't complete within the specified duration.
pub async fn with_timeout<F, T>(duration: Duration, future: F) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    tokio::time::timeout(duration, future)
        .await
        .context(format!("Test timed out after {duration:?}"))?
}

/// Get the path to the codesearch binary
///
/// Automatically builds the binary on first call if it doesn't exist.
/// Uses OnceLock to ensure the build happens only once per test run.
pub fn codesearch_binary() -> std::path::PathBuf {
    use std::process::Command;
    use std::sync::OnceLock;

    static BINARY_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();

    BINARY_PATH
        .get_or_init(|| {
            let manifest_dir = env!("CARGO_MANIFEST_DIR");
            let workspace_root = std::path::Path::new(manifest_dir)
                .parent()
                .expect("tests directory should have a parent");
            let binary_path = workspace_root.join("target/debug/codesearch");

            if !binary_path.exists() {
                eprintln!("Building codesearch binary (one-time)...");
                let status = Command::new("cargo")
                    .args(["build", "--package", "codesearch"])
                    .current_dir(workspace_root)
                    .status()
                    .expect("Failed to spawn cargo build");

                if !status.success() {
                    panic!("Failed to build codesearch binary");
                }
            }

            binary_path
        })
        .clone()
}

/// Get the workspace manifest path for cargo run commands
pub fn workspace_manifest() -> std::path::PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    std::path::Path::new(manifest_dir)
        .parent()
        .expect("tests directory should have a parent")
        .join("Cargo.toml")
}

/// Run the codesearch CLI with environment variables pointing to test infrastructure
///
/// This ensures tests use testcontainer ports instead of the user's shell environment
/// or global config, providing complete isolation.
///
/// # Arguments
/// * `repo_path` - Repository directory to use as current directory
/// * `args` - CLI arguments (e.g., `&["index"]`)
/// * `qdrant` - Testcontainer Qdrant instance
/// * `postgres` - Testcontainer Postgres instance
/// * `db_name` - Isolated test database name
pub fn run_cli_with_test_infra(
    repo_path: &Path,
    args: &[&str],
    qdrant: &Arc<TestQdrant>,
    postgres: &Arc<TestPostgres>,
    db_name: &str,
) -> Result<std::process::Output> {
    Command::new(codesearch_binary())
        .current_dir(repo_path)
        .args(args)
        .env("RUST_LOG", "info")
        // Override environment variables to use testcontainer ports
        // This ensures tests use isolated infrastructure regardless of user's
        // shell environment or global config
        .env("QDRANT_HOST", "localhost")
        .env("QDRANT_PORT", qdrant.port().to_string())
        .env("QDRANT_REST_PORT", qdrant.rest_port().to_string())
        .env("POSTGRES_HOST", "localhost")
        .env("POSTGRES_PORT", postgres.port().to_string())
        .env("POSTGRES_DATABASE", db_name)
        .env("POSTGRES_USER", "codesearch")
        .env("POSTGRES_PASSWORD", "codesearch")
        .output()
        .context("Failed to run codesearch CLI")
}
