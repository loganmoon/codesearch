//! Code Context CLI - Semantic Code Indexing System
//!
//! This binary provides the command-line interface for the codesearch system.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use codesearch_core::config::Config;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "codesearch")]
#[command(about = "Semantic code indexing and RAG system")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Configuration file path
    #[arg(short, long, value_name = "FILE", global = true)]
    config: Option<PathBuf>,

    /// Verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the MCP (Model Context Protocol) server for client integration
    Serve {
        /// Port to bind to (for HTTP mode)
        #[arg(short, long, default_value = "8699")]
        port: u16,

        /// Host to bind to (for HTTP mode)
        #[arg(long, default_value = "localhost")]
        host: String,

        /// Use HTTP transport instead of stdio
        #[arg(long)]
        http: bool,

        /// Don't auto-start Docker dependencies
        #[arg(long)]
        no_auto_start: bool,

        /// Stop Docker dependencies when server exits
        #[arg(long)]
        stop_on_exit: bool,

        /// Path to docker-compose.yml file
        #[arg(long)]
        compose_file: Option<PathBuf>,
    },
    /// Index the repository (initializes configuration if needed)
    Index {
        /// Force re-indexing of all files
        #[arg(long)]
        force: bool,

        /// Show indexing progress
        #[arg(long)]
        progress: bool,
    },
    /// Search the indexed code
    Search {
        /// Search query
        query: String,

        /// Number of results to return
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose)?;

    // Execute commands
    match cli.command {
        Some(Commands::Serve {
            port,
            host,
            http,
            no_auto_start,
            stop_on_exit,
            compose_file,
        }) => {
            // Find repository root
            let repo_root = find_repository_root()?;
            // Load configuration
            let config = load_config(&repo_root, cli.config.as_deref()).await?;
            serve(
                config,
                host,
                port,
                http,
                no_auto_start,
                stop_on_exit,
                compose_file,
            )
            .await
        }
        Some(Commands::Index { force, progress }) => {
            // Find repository root
            let repo_root = find_repository_root()?;
            // Initialize configuration if needed (won't replace existing)
            ensure_config_exists(&repo_root, cli.config.as_deref()).await?;
            // Load configuration
            let config = load_config(&repo_root, cli.config.as_deref()).await?;
            index_repository(config, force, progress).await
        }
        Some(Commands::Search { query, limit }) => {
            // Find repository root
            let repo_root = find_repository_root()?;
            // Load configuration
            let config = load_config(&repo_root, cli.config.as_deref()).await?;
            search_code(config, query, limit).await
        }
        None => {
            // Default behavior - show help
            println!("Use 'codesearch index' to index a repository, or --help for more options");
            Ok(())
        }
    }
}

/// Initialize logging system
fn init_logging(verbose: bool) -> Result<()> {
    let level = if verbose { "debug" } else { "info" };

    tracing_subscriber::fmt()
        .with_env_filter(format!(
            "codesearch={level},{}={level}",
            env!("CARGO_PKG_NAME")
        ))
        .init();

    Ok(())
}

/// Ensure configuration exists (create if needed, but don't replace existing)
async fn ensure_config_exists(repo_root: &Path, config_path: Option<&Path>) -> Result<()> {
    // Determine the config file path
    let config_file = if let Some(path) = config_path {
        path.to_path_buf()
    } else {
        repo_root.join(".codesearch").join("config.toml")
    };

    // Only create if it doesn't exist
    if !config_file.exists() {
        info!("Initializing codesearch configuration at {:?}", config_file);

        // Create parent directory if needed
        if let Some(parent) = config_file.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {parent:?}"))?;
        }

        // Create default configuration
        let config = Config::default();
        config
            .save(&config_file)
            .with_context(|| format!("Failed to save config to {config_file:?}"))?;
        info!("Created default configuration at {:?}", config_file);
    } else {
        info!("Using existing configuration at {:?}", config_file);
    }

    Ok(())
}

/// Find the repository root directory
fn find_repository_root() -> Result<PathBuf> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;

    // Try to discover the git repository
    match git2::Repository::discover(&current_dir) {
        Ok(repo) => {
            // Get the workdir (repository root)
            let workdir = repo.workdir().ok_or_else(|| {
                anyhow::anyhow!("Repository has no working directory (bare repository?)")
            })?;
            Ok(workdir.to_path_buf())
        }
        Err(e) => {
            // If git discovery fails, fall back to current directory
            warn!("Could not find git repository: {e}. Using current directory as root.");
            Ok(current_dir)
        }
    }
}

/// Load configuration from file or defaults
async fn load_config(repo_root: &Path, config_path: Option<&Path>) -> Result<Config> {
    let config_file = if let Some(path) = config_path {
        path.to_path_buf()
    } else {
        repo_root.join(".codesearch").join("config.toml")
    };

    if config_file.exists() {
        Config::from_file(&config_file)
            .with_context(|| format!("Failed to load configuration from {config_file:?}"))
    } else {
        warn!("No configuration file found, using defaults");
        Ok(Config::default())
    }
}

/// Start the MCP server
async fn serve(
    config: Config,
    host: String,
    port: u16,
    http: bool,
    no_auto_start: bool,
    _stop_on_exit: bool, // TODO: Implement cleanup on exit
    compose_file: Option<PathBuf>,
) -> Result<()> {
    // Check and start dependencies if needed
    if !no_auto_start {
        ensure_dependencies_running(&config, compose_file.as_deref()).await?;
    } else {
        // Just check if dependencies are available
        if !check_storage_health(&config).await {
            bail!("Storage service (Qdrant) is not running. Start it with docker-compose or remove --no-auto-start flag");
        }
    }

    // Start the MCP server
    if http {
        println!("🚀 Starting MCP server on HTTP at {host}:{port}...");
        todo!("HTTP mode not yet implemented")
    } else {
        println!("🚀 Starting MCP server on stdio...");
        todo!("stdio mode not yet implemented")
    }

    // Note: stop_on_exit would be handled in a defer/drop guard
}

/// Index the repository
async fn index_repository(config: Config, _force: bool, _progress: bool) -> Result<()> {
    // Create storage client
    let storage_client = codesearch_storage::create_storage_client(config.storage.clone())
        .await
        .context("Failed to create storage client")?;

    // Initialize storage (create collections if needed)
    storage_client
        .initialize()
        .await
        .context("Failed to initialize storage")?;

    // Create indexer with storage client
    let mut indexer = indexer::create_indexer(
        storage_client,
        std::env::current_dir().context("Failed to get current directory")?,
    );

    // Run indexing
    let result = indexer
        .index_repository()
        .await
        .context("Failed to index repository")?;

    println!("✅ Indexing complete!");
    println!("  Files processed: {}", result.stats.total_files);
    println!("  Entities extracted: {}", result.stats.entities_extracted);
    println!("  Relationships: {}", result.stats.relationships_extracted);

    // TODO: Wire up actual storage operations in issue #3
    Ok(())
}

/// Search the indexed code
async fn search_code(_config: Config, _query: String, _limit: usize) -> Result<()> {
    // TODO: Implement code search
    println!("🔍 Searching code...");
    todo!("⚠️  Code search implementation is not yet complete")
}

/// Check if storage service is healthy
async fn check_storage_health(config: &Config) -> bool {
    // Try to create a storage client and check its health
    match codesearch_storage::create_storage_client(config.storage.clone()).await {
        Ok(_) => {
            info!("Storage service is healthy");
            true
        }
        Err(e) => {
            warn!("Storage service check failed: {}", e);
            false
        }
    }
}

/// Ensure Docker dependencies are running
async fn ensure_dependencies_running(config: &Config, compose_file: Option<&Path>) -> Result<()> {
    println!("🔍 Checking dependencies...");

    // Check if storage is already healthy
    if check_storage_health(config).await {
        println!("✅ Storage service is already running");
        return Ok(());
    }

    println!(
        "⚠️  Storage service not accessible at {}:{}",
        config.storage.host, config.storage.port
    );
    println!("🐳 Starting Docker services...");

    // Find docker-compose file
    let compose_path = find_docker_compose_file(compose_file)?;
    println!("   Using: {}", compose_path.display());

    // Start services with docker compose (try both variants)
    let output = Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(&compose_path)
        .arg("up")
        .arg("-d")
        .arg("qdrant") // Only start what we need
        .output()
        .or_else(|_| {
            // Fallback to docker-compose for older installations
            Command::new("docker-compose")
                .arg("-f")
                .arg(&compose_path)
                .arg("up")
                .arg("-d")
                .arg("qdrant")
                .output()
        })
        .context("Failed to run docker compose. Is Docker installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to start Docker services: {}", stderr);
    }

    println!("⏳ Waiting for services to be healthy...");

    // Wait for services to be ready
    wait_for_storage(config, Duration::from_secs(30)).await?;

    println!("✅ All dependencies are ready!");
    Ok(())
}

/// Find the docker-compose.yml file
fn find_docker_compose_file(explicit_path: Option<&Path>) -> Result<PathBuf> {
    // If explicitly provided, use it
    if let Some(path) = explicit_path {
        if path.exists() {
            return Ok(path.to_path_buf());
        } else {
            bail!(
                "Specified docker-compose file not found: {}",
                path.display()
            );
        }
    }

    // Check common locations
    let locations = [
        PathBuf::from("docker-compose.yml"),
        PathBuf::from("docker-compose.yaml"),
        env::current_dir()?.join("docker-compose.yml"),
    ];

    for location in &locations {
        if location.exists() {
            return Ok(location.clone());
        }
    }

    // If in a git repo, check the root
    if let Ok(repo_root) = find_repository_root() {
        let repo_compose = repo_root.join("docker-compose.yml");
        if repo_compose.exists() {
            return Ok(repo_compose);
        }
    }

    bail!("Could not find docker-compose.yml. Please specify with --compose-file");
}

/// Wait for storage service to become healthy
async fn wait_for_storage(config: &Config, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    let mut attempt = 0;

    loop {
        attempt += 1;

        if check_storage_health(config).await {
            return Ok(());
        }

        if start.elapsed() > timeout {
            bail!(
                "Timeout waiting for storage service to start after {} seconds",
                timeout.as_secs()
            );
        }

        // Show progress
        if attempt % 3 == 0 {
            println!(
                "   Still waiting... ({}s elapsed)",
                start.elapsed().as_secs()
            );
        }

        sleep(Duration::from_secs(1)).await;
    }
}
