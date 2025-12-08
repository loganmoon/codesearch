use derive_builder::Builder;
use im::HashMap as ImHashMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use strum_macros::{Display, EnumString};

// String interning type alias (using String for now, can be optimized to Arc<str> later)
pub type InternedString = String;

/// Type of code entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Function,
    Method,
    Class,
    Struct,
    Interface,
    Trait,
    Impl,
    Enum,
    Module,
    Package,
    Constant,
    Variable,
    TypeAlias,
    Macro,
}

/// Source location information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SourceLocation {
    pub start_line: usize,
    pub end_line: usize,
    pub start_column: usize,
    pub end_column: usize,
}

impl SourceLocation {
    /// Create a SourceLocation from tree-sitter node positions
    pub fn from_tree_sitter_node(node: tree_sitter::Node) -> Self {
        let start = node.start_position();
        let end = node.end_position();

        Self {
            start_line: start.row + 1,
            end_line: end.row + 1,
            start_column: start.column,
            end_column: end.column,
        }
    }
}

/// Visibility modifiers for entities
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Internal,
}

/// Programming language enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    CSharp,
    Cpp,
    Unknown,
}

/// Entity metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EntityMetadata {
    pub is_async: bool,
    pub is_abstract: bool,
    pub is_static: bool,
    pub is_const: bool,
    pub is_generic: bool,
    pub generic_params: Vec<String>,
    pub decorators: Vec<String>,
    pub attributes: ImHashMap<String, String>,
}

/// Represents a code entity extracted from source code
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[builder(setter(into))]
pub struct CodeEntity {
    /// Unique identifier for the entity
    pub entity_id: String,

    /// Repository identifier (UUID)
    pub repository_id: String,

    /// Simple name of the entity
    pub name: String,

    /// Full qualified name of the entity (e.g., "module.class.method")
    pub qualified_name: String,

    /// Parent scope of this entity (e.g., containing class or module)
    #[builder(default = "None")]
    pub parent_scope: Option<String>,

    /// Type of the entity
    pub entity_type: EntityType,

    /// List of dependencies (imports, function calls, type references)
    #[builder(default = "Vec::new()")]
    pub dependencies: Vec<String>,

    /// Documentation summary extracted from comments
    #[builder(default = "None")]
    pub documentation_summary: Option<String>,

    /// Source file path
    pub file_path: PathBuf,

    /// Source location in the file
    pub location: SourceLocation,

    /// Visibility modifier
    #[builder(default = "Visibility::Public")]
    pub visibility: Visibility,

    /// Programming language
    pub language: Language,

    /// Function/method signature (parameters and return type)
    #[builder(default = "None")]
    pub signature: Option<FunctionSignature>,

    /// Raw content of the entity
    #[builder(default = "None")]
    pub content: Option<String>,

    /// Language-specific metadata
    #[builder(default = "EntityMetadata::default()")]
    pub metadata: EntityMetadata,
}

/// Function signature information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FunctionSignature {
    /// Parameter names and types
    pub parameters: Vec<(String, Option<String>)>, // (name, type)

    /// Return type if specified
    pub return_type: Option<String>,

    /// Whether the function is async
    pub is_async: bool,

    /// Generic type parameters
    pub generics: Vec<String>,
}

/// Relationship types between code entities
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipType {
    Contains,
    Calls,
    Imports,
    InheritsFrom,
    Implements,
    Defines,
    Uses,
    Returns,
    AcceptsParameter,
    ThrowsException,
    DefinesEntity, // chunk-to-entity
}

/// Represents a relationship between code entities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeRelationship {
    pub relationship_type: RelationshipType,
    pub from_entity_id: String,
    pub to_entity_id: String,
    pub properties: ImHashMap<String, String>,
}
