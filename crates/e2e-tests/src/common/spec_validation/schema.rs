//! Schema types for specification-based graph validation tests

/// Project structure type for test fixtures
#[derive(Debug, Clone, Copy, Default)]
pub enum ProjectType {
    /// Single crate with src/lib.rs (default)
    #[default]
    SingleCrate,
    /// Single binary crate with src/main.rs
    BinaryCrate,
    /// Workspace with multiple member crates
    Workspace,
    /// Custom project structure (files are placed at root, not in src/)
    Custom,
}

/// A test fixture that defines source files and expected graph structure
#[derive(Debug, Clone)]
pub struct Fixture {
    /// Name of the fixture for test identification
    pub name: &'static str,
    /// Source files to create in the test repository: (relative_path, content)
    /// For SingleCrate/BinaryCrate: paths are relative to src/
    /// For Workspace/Custom: paths are relative to repo root
    pub files: &'static [(&'static str, &'static str)],
    /// Expected entities to be extracted
    pub entities: &'static [ExpectedEntity],
    /// Expected relationships to be created
    pub relationships: &'static [ExpectedRelationship],
    /// Project structure type (defaults to SingleCrate)
    pub project_type: ProjectType,
    /// Optional custom Cargo.toml content (uses default if None)
    pub cargo_toml: Option<&'static str>,
}

/// An expected entity in the graph
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExpectedEntity {
    /// Entity type label (e.g., "Function", "Struct", "Trait")
    pub entity_type: &'static str,
    /// Fully qualified name (e.g., "test_crate::module::function")
    pub qualified_name: &'static str,
}

/// An expected relationship in the graph
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExpectedRelationship {
    /// Relationship type (e.g., "CONTAINS", "CALLS", "IMPLEMENTS")
    pub rel_type: &'static str,
    /// Source entity qualified_name
    pub from: &'static str,
    /// Target entity qualified_name
    pub to: &'static str,
}

/// An actual entity retrieved from Neo4j
#[derive(Debug, Clone)]
pub struct ActualEntity {
    pub entity_id: String,
    pub entity_type: String,
    pub qualified_name: String,
    pub name: String,
}

/// An actual relationship retrieved from Neo4j
#[derive(Debug, Clone)]
pub struct ActualRelationship {
    pub rel_type: String,
    pub from_qualified_name: String,
    pub to_qualified_name: String,
}
