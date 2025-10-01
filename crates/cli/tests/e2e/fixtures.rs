//! Test repository fixtures for E2E tests

use anyhow::{Context, Result};
use codesearch_core::entities::EntityType;
use codesearch_core::CodeEntity;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Builder for creating test repositories with custom content
pub struct TestRepositoryBuilder {
    name: String,
    files: Vec<(PathBuf, String)>,
    init_git: bool,
}

impl TestRepositoryBuilder {
    /// Create a new test repository builder
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
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

    /// Control whether to initialize git (default: true)
    pub fn with_git_init(mut self, enabled: bool) -> Self {
        self.init_git = enabled;
        self
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

/// Create a simple Rust repository with minimal entities
///
/// Expected: 2-3 functions, 1 struct (3-5 entities total)
pub async fn simple_rust_repo() -> Result<TempDir> {
    TestRepositoryBuilder::new("simple")
        .with_rust_file(
            "main.rs",
            r#"
//! Simple test module

/// Greet a person
fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}

/// A simple counter
struct Counter {
    value: i32,
}

impl Counter {
    /// Create a new counter
    fn new() -> Self {
        Self { value: 0 }
    }
}

fn main() {
    println!("{}", greet("World"));
}
"#,
        )
        .build()
        .await
}

/// Expected entities for simple_rust_repo
pub fn simple_rust_repo_expected_entities() -> Vec<ExpectedEntity> {
    vec![
        ExpectedEntity::new("greet", EntityType::Function, "main.rs"),
        ExpectedEntity::new("Counter", EntityType::Struct, "main.rs"),
        ExpectedEntity::new("Counter::new", EntityType::Method, "main.rs"),
        ExpectedEntity::new("main", EntityType::Function, "main.rs"),
    ]
}

/// Create a multi-file Rust repository with moderate complexity
///
/// Expected: 10-15 entities across multiple files
pub async fn multi_file_rust_repo() -> Result<TempDir> {
    TestRepositoryBuilder::new("multi")
        .with_rust_file(
            "main.rs",
            r#"
//! Main module

use std::collections::HashMap;

/// Main entry point
fn main() {
    println!("Hello, world!");
    let calculator = Calculator::new();
    let result = calculator.add(2, 3);
    println!("Result: {result}");
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
"#,
        )
        .with_rust_file(
            "lib.rs",
            r#"
//! Library module

pub mod utils;

/// Process some data
pub fn process_data(data: &[u8]) -> Vec<u8> {
    data.iter().map(|b| b.wrapping_add(1)).collect()
}

/// A trait for serialization
pub trait Serialize {
    fn serialize(&self) -> String;
}
"#,
        )
        .with_rust_file(
            "utils.rs",
            r#"
//! Utility functions

/// Reverse a string
pub fn reverse_string(s: &str) -> String {
    s.chars().rev().collect()
}

/// Check if a number is even
pub fn is_even(n: i32) -> bool {
    n % 2 == 0
}

/// Helper struct for formatting
pub struct Formatter {
    prefix: String,
}

impl Formatter {
    /// Create a new formatter
    pub fn new(prefix: String) -> Self {
        Self { prefix }
    }

    /// Format a message
    pub fn format(&self, msg: &str) -> String {
        format!("{}: {msg}", self.prefix)
    }
}
"#,
        )
        .build()
        .await
}

/// Expected entities for multi_file_rust_repo
pub fn multi_file_rust_repo_expected_entities() -> Vec<ExpectedEntity> {
    vec![
        ExpectedEntity::new("main", EntityType::Function, "main.rs"),
        ExpectedEntity::new("Calculator", EntityType::Struct, "main.rs"),
        ExpectedEntity::new("Calculator::new", EntityType::Method, "main.rs"),
        ExpectedEntity::new("Calculator::add", EntityType::Method, "main.rs"),
        ExpectedEntity::new("Calculator::subtract", EntityType::Method, "main.rs"),
        ExpectedEntity::new("process_data", EntityType::Function, "lib.rs"),
        ExpectedEntity::new("Serialize", EntityType::Trait, "lib.rs"),
        ExpectedEntity::new("reverse_string", EntityType::Function, "utils.rs"),
        ExpectedEntity::new("is_even", EntityType::Function, "utils.rs"),
        ExpectedEntity::new("Formatter", EntityType::Struct, "utils.rs"),
        ExpectedEntity::new("Formatter::new", EntityType::Method, "utils.rs"),
        ExpectedEntity::new("Formatter::format", EntityType::Method, "utils.rs"),
    ]
}

/// Create a complex Rust repository with realistic project structure
///
/// Expected: 20-30 entities with nested modules, traits, implementations
pub async fn complex_rust_repo() -> Result<TempDir> {
    TestRepositoryBuilder::new("complex")
        .with_rust_file(
            "main.rs",
            r#"
//! Complex application entry point

mod database;
mod models;
mod api;

use database::Database;
use models::User;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::connect("localhost:5432")?;
    let user = User::new("alice", "alice@example.com");
    db.save_user(&user)?;
    Ok(())
}
"#,
        )
        .with_rust_file(
            "models.rs",
            r#"
//! Domain models

use std::fmt;

/// Represents a user in the system
#[derive(Debug, Clone)]
pub struct User {
    pub username: String,
    pub email: String,
}

impl User {
    /// Create a new user
    pub fn new(username: &str, email: &str) -> Self {
        Self {
            username: username.to_string(),
            email: email.to_string(),
        }
    }

    /// Validate the user's email
    pub fn is_valid_email(&self) -> bool {
        self.email.contains('@')
    }
}

impl fmt::Display for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "User({}, {})", self.username, self.email)
    }
}

/// Represents a post
#[derive(Debug, Clone)]
pub struct Post {
    pub id: u64,
    pub author: String,
    pub content: String,
}

impl Post {
    /// Create a new post
    pub fn new(id: u64, author: String, content: String) -> Self {
        Self { id, author, content }
    }

    /// Get a preview of the post
    pub fn preview(&self, len: usize) -> String {
        self.content.chars().take(len).collect()
    }
}
"#,
        )
        .with_rust_file(
            "database.rs",
            r#"
//! Database operations

use crate::models::{User, Post};
use std::error::Error;

/// Database connection
pub struct Database {
    connection_string: String,
}

impl Database {
    /// Connect to the database
    pub fn connect(connection_string: &str) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            connection_string: connection_string.to_string(),
        })
    }

    /// Save a user to the database
    pub fn save_user(&self, user: &User) -> Result<(), Box<dyn Error>> {
        println!("Saving user: {user}");
        Ok(())
    }

    /// Fetch a user by username
    pub fn get_user(&self, username: &str) -> Result<Option<User>, Box<dyn Error>> {
        println!("Fetching user: {username}");
        Ok(None)
    }

    /// Save a post
    pub fn save_post(&self, post: &Post) -> Result<(), Box<dyn Error>> {
        println!("Saving post: {}", post.id);
        Ok(())
    }
}

/// Trait for entities that can be persisted
pub trait Persistable {
    fn save(&self, db: &Database) -> Result<(), Box<dyn Error>>;
}
"#,
        )
        .with_rust_file(
            "api.rs",
            r#"
//! API handlers

use crate::database::Database;
use crate::models::User;

/// Handle user creation request
pub fn create_user(db: &Database, username: &str, email: &str) -> Result<User, String> {
    let user = User::new(username, email);

    if !user.is_valid_email() {
        return Err("Invalid email".to_string());
    }

    db.save_user(&user).map_err(|e| e.to_string())?;
    Ok(user)
}

/// Handle user fetch request
pub fn get_user(db: &Database, username: &str) -> Result<Option<User>, String> {
    db.get_user(username).map_err(|e| e.to_string())
}

/// Health check endpoint
pub fn health_check() -> &'static str {
    "OK"
}
"#,
        )
        .build()
        .await
}

/// Expected entities for complex_rust_repo
pub fn complex_rust_repo_expected_entities() -> Vec<ExpectedEntity> {
    vec![
        ExpectedEntity::new("main", EntityType::Function, "main.rs"),
        ExpectedEntity::new("User", EntityType::Struct, "models.rs"),
        ExpectedEntity::new("User::new", EntityType::Method, "models.rs"),
        ExpectedEntity::new("User::is_valid_email", EntityType::Method, "models.rs"),
        ExpectedEntity::new("Post", EntityType::Struct, "models.rs"),
        ExpectedEntity::new("Post::new", EntityType::Method, "models.rs"),
        ExpectedEntity::new("Post::preview", EntityType::Method, "models.rs"),
        ExpectedEntity::new("Database", EntityType::Struct, "database.rs"),
        ExpectedEntity::new("Database::connect", EntityType::Method, "database.rs"),
        ExpectedEntity::new("Database::save_user", EntityType::Method, "database.rs"),
        ExpectedEntity::new("Database::get_user", EntityType::Method, "database.rs"),
        ExpectedEntity::new("Database::save_post", EntityType::Method, "database.rs"),
        ExpectedEntity::new("Persistable", EntityType::Trait, "database.rs"),
        ExpectedEntity::new("create_user", EntityType::Function, "api.rs"),
        ExpectedEntity::new("get_user", EntityType::Function, "api.rs"),
        ExpectedEntity::new("health_check", EntityType::Function, "api.rs"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_builder_creates_files() -> Result<()> {
        let repo = TestRepositoryBuilder::new("test")
            .with_file("test.txt", "content")
            .with_rust_file("lib.rs", "fn test() {}")
            .build()
            .await?;

        assert!(repo.path().join("test.txt").exists());
        assert!(repo.path().join("src/lib.rs").exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_simple_repo_has_expected_structure() -> Result<()> {
        let repo = simple_rust_repo().await?;
        assert!(repo.path().join("src/main.rs").exists());

        let content = tokio::fs::read_to_string(repo.path().join("src/main.rs")).await?;
        assert!(content.contains("fn greet"));
        assert!(content.contains("struct Counter"));

        Ok(())
    }

    #[tokio::test]
    async fn test_multi_file_repo_has_all_files() -> Result<()> {
        let repo = multi_file_rust_repo().await?;
        assert!(repo.path().join("src/main.rs").exists());
        assert!(repo.path().join("src/lib.rs").exists());
        assert!(repo.path().join("src/utils.rs").exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_expected_entity_matches() {
        use codesearch_core::entities::{Language, SourceLocation, Visibility};

        let entity = CodeEntity {
            entity_id: "test::greet".to_string(),
            name: "greet".to_string(),
            qualified_name: "test::greet".to_string(),
            parent_scope: None,
            entity_type: EntityType::Function,
            dependencies: Vec::new(),
            cyclomatic_complexity: None,
            lines_of_code: 3,
            documentation_summary: None,
            file_path: PathBuf::from("/tmp/test/src/main.rs"),
            location: SourceLocation {
                start_line: 1,
                end_line: 3,
                start_column: 0,
                end_column: 0,
            },
            line_range: (1, 3),
            visibility: Visibility::Public,
            language: Language::Rust,
            signature: None,
            content: Some(
                "fn greet(name: &str) -> String {\n    format!(\"Hello, {name}!\")\n}".to_string(),
            ),
            metadata: Default::default(),
        };

        let expected = ExpectedEntity::new("greet", EntityType::Function, "main.rs");
        assert!(expected.matches(&entity));
    }
}
