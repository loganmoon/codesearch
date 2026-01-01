use derive_builder::Builder;
use im::HashMap as ImHashMap;
use serde::{Deserialize, Deserializer, Serialize};
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
    Static,
    Union,
    ExternBlock,
    Variable,
    TypeAlias,
    Macro,
}

/// Source location information
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// Type of reference to another entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ReferenceType {
    /// Function or method call
    Call,
    /// Type annotation or usage
    TypeUsage,
    /// Import statement
    Import,
    /// extends/implements relationship
    Extends,
    /// General usage (field types, etc.)
    Uses,
}

/// Compute the simple name (last path segment) from a qualified reference.
///
/// Handles both Rust-style `::` separators and dot notation `.` separators.
/// Returns the original string if no separator is found.
fn compute_simple_name(target: &str) -> String {
    target
        .rsplit("::")
        .next()
        .or_else(|| target.rsplit('.').next())
        .unwrap_or(target)
        .to_string()
}

/// A reference from one entity to another at a specific source location.
///
/// Captures call sites, type annotations, imports, etc. The `target` field
/// contains the best-effort qualified name, which may be:
/// - Fully resolved for internal references (e.g., "crate::module::function")
/// - Partially resolved or external for cross-crate references
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SourceReference {
    /// Qualified name of the target entity.
    /// May be fully resolved (internal) or partial/external (cross-crate).
    pub target: String,

    /// Pre-computed simple name (last path segment, without generics).
    /// For "std::collections::HashMap<K, V>", this would be "HashMap".
    /// For "external.lodash.debounce", this would be "debounce".
    #[serde(default)]
    pub simple_name: String,

    /// Whether this references an external dependency (outside the repository).
    /// Set during extraction based on import resolution context.
    #[serde(default)]
    pub is_external: bool,

    /// Location of the reference in source (line/column)
    pub location: SourceLocation,
    /// Type of reference
    pub ref_type: ReferenceType,
}

impl SourceReference {
    /// Create a new SourceReference with all fields explicitly provided.
    ///
    /// This is the primary constructor - `simple_name` should be extracted
    /// directly from the AST node (not computed from the target string).
    ///
    /// # Panics
    /// Panics if `target` or `simple_name` is empty after trimming whitespace.
    pub fn new(
        target: impl Into<String>,
        simple_name: impl Into<String>,
        is_external: bool,
        location: SourceLocation,
        ref_type: ReferenceType,
    ) -> Self {
        let target = target.into();
        let simple_name = simple_name.into();
        assert!(
            !target.trim().is_empty(),
            "SourceReference target must be non-empty"
        );
        assert!(
            !simple_name.trim().is_empty(),
            "SourceReference simple_name must be non-empty"
        );
        Self {
            target,
            simple_name,
            is_external,
            location,
            ref_type,
        }
    }
}

/// Custom deserializer for SourceReference that recomputes simple_name from target
/// if it's missing or empty. This ensures backward compatibility with older
/// serialized data that didn't include simple_name.
impl<'de> Deserialize<'de> for SourceReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct SourceReferenceHelper {
            target: String,
            #[serde(default)]
            simple_name: String,
            #[serde(default)]
            is_external: bool,
            location: SourceLocation,
            ref_type: ReferenceType,
        }

        let helper = SourceReferenceHelper::deserialize(deserializer)?;

        // Recompute simple_name from target if missing or empty
        let simple_name = if helper.simple_name.is_empty() {
            compute_simple_name(&helper.target)
        } else {
            helper.simple_name
        };

        Ok(SourceReference {
            target: helper.target,
            simple_name,
            is_external: helper.is_external,
            location: helper.location,
            ref_type: helper.ref_type,
        })
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
    /// Structured generic bounds: maps type parameter name to list of trait bounds.
    /// E.g., for `<T: Clone + Send>` this would be `{"T": ["Clone", "Send"]}`.
    pub generic_bounds: ImHashMap<String, Vec<String>>,
    pub decorators: Vec<String>,
    pub attributes: ImHashMap<String, String>,
}

/// Typed relationship data extracted from source code.
///
/// This struct provides an explicit typed contract between the languages crate
/// (which extracts entities) and the outbox-processor crate (which resolves
/// relationships). Each field corresponds to a specific relationship type.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EntityRelationshipData {
    /// Function/method calls made by this entity.
    /// Resolved to CALLS relationships in Neo4j.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<SourceReference>,

    /// Type references used by this entity (parameters, return types, field types).
    /// Resolved to USES relationships in Neo4j.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses_types: Vec<SourceReference>,

    /// Imported modules/entities.
    /// Resolved to IMPORTS relationships in Neo4j.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<SourceReference>,

    /// Trait/interface being implemented (for impl blocks).
    /// Resolved to IMPLEMENTS relationships in Neo4j.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implements_trait: Option<SourceReference>,

    /// Type this impl block is for (for ASSOCIATES relationship).
    /// Resolved to ASSOCIATES relationships in Neo4j.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub for_type: Option<SourceReference>,

    /// Parent class/interface for inheritance (JS/TS extends, Python bases).
    /// Resolved to INHERITS_FROM relationships in Neo4j.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extends: Vec<SourceReference>,

    /// Trait supertraits (Rust trait bounds like `trait Foo: Bar`).
    /// Resolved to EXTENDS_INTERFACE relationships in Neo4j.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supertraits: Vec<SourceReference>,

    /// Pre-computed call aliases for language-specific resolution.
    /// E.g., Rust UFCS: "TypeFQN::method" for "<TypeFQN as TraitFQN>::method".
    /// Computed during extraction to keep outbox-processor language-agnostic.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub call_aliases: Vec<String>,
}

impl EntityRelationshipData {
    /// Check if all relationship data is empty
    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
            && self.uses_types.is_empty()
            && self.imports.is_empty()
            && self.implements_trait.is_none()
            && self.for_type.is_none()
            && self.extends.is_empty()
            && self.supertraits.is_empty()
            && self.call_aliases.is_empty()
    }
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

    /// Full qualified name of the entity (semantic, package-relative)
    /// e.g., "jotai.utils.helpers.formatNumber" or "codesearch_core::entities::CodeEntity"
    /// Used for LSP validation, graph edge resolution, and semantic lookups.
    pub qualified_name: String,

    /// File-path-based identifier for import resolution
    /// e.g., "website.src.pages.index" or "crates.core.src.entities"
    /// Used for resolving relative imports and file-based lookups.
    #[builder(default = "None")]
    pub path_entity_identifier: Option<String>,

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

    /// Visibility modifier (None means visibility doesn't apply to this entity type)
    #[builder(default = "None")]
    pub visibility: Option<Visibility>,

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

    /// Typed relationship data for graph resolution.
    /// This field provides explicit typed data for relationship resolution,
    /// replacing the implicit JSON-encoded data in metadata.attributes.
    #[serde(default)]
    #[builder(default = "EntityRelationshipData::default()")]
    pub relationships: EntityRelationshipData,
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
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
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
    DefinesEntity,
    /// Impl block associates with its target type
    Associates,
    /// Trait extends another trait (supertraits)
    ExtendsInterface,
}

/// Represents a relationship between code entities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeRelationship {
    pub relationship_type: RelationshipType,
    pub from_entity_id: String,
    pub to_entity_id: String,
    pub properties: ImHashMap<String, String>,
}
