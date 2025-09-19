//! Code Context CLI - Semantic Code Indexing System
//!
//! This binary provides the command-line interface for the codesearch system.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

mod docker;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use codesearch_core::config::Config;
use std::env;
use std::path::{Path, PathBuf};
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
    /// Initialize codesearch in the current repository
    Init,
    /// Start the MCP (Model Context Protocol) server for client integration
    Serve {
        /// Port to bind to
        #[arg(short, long, default_value = "8699")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "localhost")]
        host: String,
    },
    /// Index the repository
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
    /// Manage containerized dependencies
    #[command(subcommand)]
    Deps(DepsCommands),
}

#[derive(Subcommand)]
enum DepsCommands {
    /// Start containerized dependencies
    Start {
        /// Docker compose file to use
        #[arg(short = 'f', long)]
        compose_file: Option<String>,
    },
    /// Stop containerized dependencies
    Stop {
        /// Docker compose file to use
        #[arg(short = 'f', long)]
        compose_file: Option<String>,
    },
    /// Check status of dependencies
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose)?;

    // Execute commands
    match cli.command {
        Some(Commands::Init) => init_repository(cli.config.as_deref()).await,
        Some(Commands::Serve { port, host }) => {
            // Find repository root
            let repo_root = find_repository_root()?;
            // Load configuration
            let config = load_config(&repo_root, cli.config.as_deref()).await?;
            serve(config, host, port).await
        }
        Some(Commands::Index { force, progress }) => {
            // Find repository root
            let repo_root = find_repository_root()?;
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
        Some(Commands::Deps(deps_cmd)) => {
            handle_deps_command(deps_cmd, cli.config.as_deref()).await
        }
        None => {
            // Default behavior - show help
            println!(
                "Use 'codesearch init' to initialize a repository, or --help for more options"
            );
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

/// Initialize codesearch in a repository
async fn init_repository(config_path: Option<&Path>) -> Result<()> {
    let current_dir = env::current_dir()?;

    info!("Initializing codesearch in {:?}", current_dir);

    // Create default configuration if it doesn't exist
    let config_file = current_dir.join("codesearch.toml");
    if !config_file.exists() {
        let config = Config::default();
        config
            .save(&config_file)
            .with_context(|| format!("Failed to save config to {config_file:?}"))?;
        info!("Created default configuration at {:?}", config_file);
    }

    // Load or use provided configuration
    let config = if let Some(path) = config_path {
        Config::from_file(path)?
    } else {
        Config::from_file(&config_file)?
    };

    config.validate()?;

    // Create vector database client

    // Initialize the repository (if necessary)

    todo!("Not yet implemented")
}

/// Find the repository root directory
fn find_repository_root() -> Result<PathBuf> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;

    // Walk up the directory tree looking for .git
    let mut dir = current_dir.as_path();
    loop {
        let git_dir = dir.join(".git");
        if git_dir.exists() {
            // Check if it's a regular git repo or a worktree/submodule
            if git_dir.is_dir() {
                // Regular git repository
                return Ok(dir.to_path_buf());
            } else if git_dir.is_file() {
                // Worktree or submodule - read the gitdir pointer
                let contents =
                    std::fs::read_to_string(&git_dir).context("Failed to read .git file")?;
                if let Some(_gitdir_line) = contents.lines().find(|l| l.starts_with("gitdir:")) {
                    // This is a worktree/submodule, but we still use this as the root
                    return Ok(dir.to_path_buf());
                }
            }
        }

        // Move up one directory
        dir = dir
            .parent()
            .ok_or_else(|| anyhow!("Not inside a git repository (reached filesystem root)"))?;
    }
}

/// Load configuration from file or defaults
async fn load_config(repo_root: &Path, config_path: Option<&Path>) -> Result<Config> {
    let config_file = if let Some(path) = config_path {
        path.to_path_buf()
    } else {
        repo_root.join("codesearch.toml")
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
async fn serve(config: Config, _host: String, _port: u16) -> Result<()> {
    info!("Checking dependencies...");

    // Ensure dependencies are running
    docker::ensure_dependencies_running(&config.storage).await?;

    println!("🚀 Starting MCP server on stdio...");

    // TODO: Initialize storage connection
    // TODO: Start MCP server on stdio

    todo!("MCP server implementation")
}

/// Index the repository
async fn index_repository(_config: Config, _force: bool, _progress: bool) -> Result<()> {
    // TODO: Implement indexing once storage API is ready
    todo!("📚 Indexing not yet implemented")
}

/// Search the indexed code
async fn search_code(_config: Config, _query: String, _limit: usize) -> Result<()> {
    // TODO: Implement code search
    println!("🔍 Searching code...");
    todo!("⚠️  Code search implementation is not yet complete")
}

/// Handle dependency management commands
async fn handle_deps_command(cmd: DepsCommands, config_path: Option<&Path>) -> Result<()> {
    match cmd {
        DepsCommands::Start { compose_file } => {
            let compose_file = compose_file.or_else(|| {
                config_path
                    .and_then(|p| p.parent())
                    .map(|p| p.join("docker-compose.yml").to_string_lossy().into_owned())
            });

            docker::start_dependencies(compose_file.as_deref())?;
            println!("✅ Dependencies started successfully");
            Ok(())
        }
        DepsCommands::Stop { compose_file } => {
            let compose_file = compose_file.or_else(|| {
                config_path
                    .and_then(|p| p.parent())
                    .map(|p| p.join("docker-compose.yml").to_string_lossy().into_owned())
            });

            docker::stop_dependencies(compose_file.as_deref())?;
            println!("✅ Dependencies stopped successfully");
            Ok(())
        }
        DepsCommands::Status => {
            // Try to load config to get Qdrant settings, use defaults if not found
            let config = if let Ok(repo_root) = find_repository_root() {
                load_config(&repo_root, config_path)
                    .await
                    .unwrap_or_default()
            } else {
                Config::default()
            };

            let status = docker::get_dependencies_status(&config.storage).await?;
            println!("{status}");
            Ok(())
        }
    }
}
