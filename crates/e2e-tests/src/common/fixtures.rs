//! Test repository fixtures for E2E tests

use anyhow::{Context, Result};
use codesearch_core::entities::EntityType;
use codesearch_core::CodeEntity;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Builder for creating test repositories with custom content
pub struct TestRepositoryBuilder {
    files: Vec<(PathBuf, String)>,
    init_git: bool,
}

impl Default for TestRepositoryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TestRepositoryBuilder {
    /// Create a new test repository builder
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            init_git: true,
        }
    }

    /// Add a file to the repository
    pub fn with_file(mut self, path: impl AsRef<Path>, content: impl Into<String>) -> Self {
        self.files
            .push((path.as_ref().to_path_buf(), content.into()));
        self
    }

    /// Add a Rust file to the src/ directory
    pub fn with_rust_file(self, name: &str, content: &str) -> Self {
        let path = PathBuf::from("src").join(name);
        self.with_file(path, content)
    }

    /// Build the test repository
    pub async fn build(self) -> Result<TempDir> {
        let temp_dir = TempDir::new().context("Failed to create temp directory")?;
        let base = temp_dir.path();

        // Create all files
        for (path, content) in self.files {
            let full_path = base.join(&path);

            // Create parent directories if needed
            if let Some(parent) = full_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
            }

            tokio::fs::write(&full_path, content)
                .await
                .with_context(|| format!("Failed to write file: {}", full_path.display()))?;
        }

        // Initialize git if requested
        if self.init_git {
            std::process::Command::new("git")
                .current_dir(base)
                .args(["init"])
                .output()
                .context("Failed to init git repo")?;

            // Configure git user for the test repo
            std::process::Command::new("git")
                .current_dir(base)
                .args(["config", "user.email", "test@example.com"])
                .output()
                .context("Failed to configure git user email")?;

            std::process::Command::new("git")
                .current_dir(base)
                .args(["config", "user.name", "Test User"])
                .output()
                .context("Failed to configure git user name")?;
        }

        Ok(temp_dir)
    }
}

/// Expected entity definition for verification
#[derive(Debug, Clone)]
pub struct ExpectedEntity {
    pub name: String,
    pub entity_type: EntityType,
    pub file_path_contains: String,
}

impl ExpectedEntity {
    /// Create a new expected entity
    pub fn new(name: &str, entity_type: EntityType, file_path_contains: &str) -> Self {
        Self {
            name: name.to_string(),
            entity_type,
            file_path_contains: file_path_contains.to_string(),
        }
    }

    /// Check if an entity matches this expected entity
    pub fn matches(&self, entity: &CodeEntity) -> bool {
        entity.name == self.name
            && entity.entity_type == self.entity_type
            && entity
                .file_path
                .to_string_lossy()
                .contains(&self.file_path_contains)
    }
}



// =============================================================================
// Real Codebase Fixtures
// =============================================================================

/// Clone a git repository at a specific tag/branch to a temp directory
///
/// This provides real-world codebases for comprehensive E2E testing.
pub async fn git_clone(url: &str, tag: &str) -> Result<TempDir> {
    let temp_dir = TempDir::new().context("Failed to create temp directory for clone")?;

    let output = std::process::Command::new("git")
        .args(["clone", "--depth", "1", "--branch", tag, url])
        .arg(temp_dir.path())
        .output()
        .context("Failed to execute git clone")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "Failed to clone {url} at {tag}: {stderr}"
        ));
    }

    Ok(temp_dir)
}

/// Clone the `anyhow` Rust crate for testing
///
/// anyhow is a small, well-structured error handling crate with:
/// - Clear trait implementations
/// - Good module organization
/// - Comprehensive documentation
///
/// Expected: ~20-30 entities including structs, traits, impls, and functions
pub async fn real_rust_crate_anyhow() -> Result<TempDir> {
    git_clone("https://github.com/dtolnay/anyhow", "1.0.75").await
}

/// Clone the `thiserror` Rust crate for testing
///
/// thiserror is a derive macro crate with:
/// - Procedural macro implementations
/// - Trait definitions
/// - Clean module structure
///
/// Expected: ~15-25 entities
pub async fn real_rust_crate_thiserror() -> Result<TempDir> {
    git_clone("https://github.com/dtolnay/thiserror", "1.0.50").await
}

/// Clone the `python-dotenv` Python package for testing
///
/// python-dotenv is a small Python package with:
/// - Clear module structure
/// - IPython integration
/// - CLI interface
///
/// Expected: ~30-50 entities including classes, functions, and methods
pub async fn real_python_package() -> Result<TempDir> {
    git_clone("https://github.com/theskumar/python-dotenv", "v1.0.0").await
}

/// Clone the Express.js framework for testing (JavaScript)
///
/// Express is the most popular Node.js web framework with:
/// - Multiple routers and middleware
/// - Large API surface with many functions and methods
/// - Complex module/require patterns
///
/// Expected: 200+ entities with rich call/import relationships
pub async fn real_express_project() -> Result<TempDir> {
    git_clone("https://github.com/expressjs/express", "4.21.2").await
}

/// Clone the jotai library for testing (TypeScript)
///
/// jotai is a primitive and flexible state management library with:
/// - Clean TypeScript types
/// - Multiple utility modules
/// - Good variety of classes and functions
///
/// Expected: 150-300 entities with inter-module relationships
pub async fn real_jotai_project() -> Result<TempDir> {
    git_clone("https://github.com/pmndrs/jotai", "v2.9.3").await
}
