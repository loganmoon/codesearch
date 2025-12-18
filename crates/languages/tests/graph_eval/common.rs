//! Shared helpers for graph evaluation tests
//!
//! This module provides common utilities for cloning repositories and evaluating
//! TSG extraction on codebases using cross-file resolution.

use git2::Repository;
use std::path::{Path, PathBuf};

/// Clone a git repository to a target directory, or use existing cache
pub fn clone_repo(repo_url: &str, target_dir: &Path) -> Result<PathBuf, git2::Error> {
    if target_dir.exists() {
        println!("Using cached repository at {}", target_dir.display());
        return Ok(target_dir.to_path_buf());
    }

    println!("Cloning {} to {}...", repo_url, target_dir.display());
    Repository::clone(repo_url, target_dir)?;
    println!("Clone complete.");

    Ok(target_dir.to_path_buf())
}
