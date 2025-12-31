//! Schema types for specification-based graph validation tests

use codesearch_core::entities::Visibility;

/// Entity type for graph validation tests
///
/// This enum provides compile-time type safety for entity types,
/// preventing typos in test specifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Module,
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    ImplBlock,
    Constant,
    TypeAlias,
    Macro,
}

impl EntityKind {
    /// Convert to Neo4j label string
    pub fn as_neo4j_label(&self) -> &'static str {
        match self {
            EntityKind::Module => "Module",
            EntityKind::Function => "Function",
            EntityKind::Method => "Method",
            EntityKind::Struct => "Struct",
            EntityKind::Enum => "Enum",
            EntityKind::Trait => "Trait",
            EntityKind::ImplBlock => "ImplBlock",
            EntityKind::Constant => "Constant",
            EntityKind::TypeAlias => "TypeAlias",
            EntityKind::Macro => "Macro",
        }
    }
}

/// Relationship type for graph validation tests
///
/// This enum provides compile-time type safety for relationship types,
/// preventing typos in test specifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationshipKind {
    Contains,
    Calls,
    Implements,
    Associates,
    ExtendsInterface,
    InheritsFrom,
    Uses,
    Imports,
}

impl RelationshipKind {
    /// Convert to Neo4j relationship type string
    pub fn as_neo4j_type(&self) -> &'static str {
        match self {
            RelationshipKind::Contains => "CONTAINS",
            RelationshipKind::Calls => "CALLS",
            RelationshipKind::Implements => "IMPLEMENTS",
            RelationshipKind::Associates => "ASSOCIATES",
            RelationshipKind::ExtendsInterface => "EXTENDS_INTERFACE",
            RelationshipKind::InheritsFrom => "INHERITS_FROM",
            RelationshipKind::Uses => "USES",
            RelationshipKind::Imports => "IMPORTS",
        }
    }
}

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
///
/// Uses strongly-typed `EntityKind` to prevent typos in test specifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedEntity {
    /// Entity type (e.g., Function, Struct, Trait)
    pub kind: EntityKind,
    /// Fully qualified name (e.g., "test_crate::module::function")
    pub qualified_name: &'static str,
    /// Expected visibility (None = don't validate, Some = must match)
    pub visibility: Option<Visibility>,
}

/// An expected relationship in the graph
///
/// Uses strongly-typed `RelationshipKind` to prevent typos in test specifications.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExpectedRelationship {
    /// Relationship type (e.g., Contains, Calls, Implements)
    pub kind: RelationshipKind,
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
    pub visibility: Option<Visibility>,
}

/// An actual relationship retrieved from Neo4j
#[derive(Debug, Clone)]
pub struct ActualRelationship {
    pub rel_type: String,
    pub from_qualified_name: String,
    pub to_qualified_name: String,
}
