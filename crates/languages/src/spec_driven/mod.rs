//! Spec-driven entity extraction framework
//!
//! This module provides the types and functions for extracting entities
//! from source code based on declarative specifications in YAML files.
//!
//! The core idea is that handlers are configured declaratively:
//! - Which tree-sitter query to use
//! - How to derive the entity name
//! - Which extractors to use for metadata and relationships
//! - What the qualified name template looks like
//!
//! # Architecture
//!
//! The spec-driven extraction works as follows:
//!
//! 1. YAML specs define queries, extractors, and handler configurations
//! 2. Build script (`build.rs`) generates Rust code from specs
//! 3. Generated code includes query constants and `HandlerConfig` structs
//! 4. The extraction engine uses these configs to extract entities
//!
//! # Example
//!
//! ```ignore
//! use codesearch_languages::spec_driven::{rust, engine};
//!
//! // Get handler configs from generated code
//! for config in rust::handler_configs::ALL_HANDLERS {
//!     let entities = engine::extract_with_config(config, &ctx, tree_root, source)?;
//! }
//! ```

pub mod engine;
pub mod extractors;
pub mod relationships;

use codesearch_core::entities::Visibility;

// Generated code from YAML specs
pub mod javascript {
    include!(concat!(env!("OUT_DIR"), "/javascript_generated.rs"));
}

pub mod typescript {
    include!(concat!(env!("OUT_DIR"), "/typescript_generated.rs"));
}

pub mod rust {
    include!(concat!(env!("OUT_DIR"), "/rust_generated.rs"));
}

/// Configuration for a spec-driven handler
#[derive(Debug, Clone)]
pub struct HandlerConfig {
    /// Entity rule ID from the spec (e.g., "E-FN-DECL")
    pub entity_rule: &'static str,

    /// Tree-sitter query string
    pub query: &'static str,

    /// Primary capture name in the query
    pub capture: &'static str,

    /// How to derive the entity name
    pub name_strategy: NameStrategy,

    /// Template for building qualified names
    /// Uses placeholders like {scope}, {name}, {impl_type_name}
    pub qualified_name_template: Option<&'static str>,

    /// Metadata extractor to use
    pub metadata_extractor: Option<MetadataExtractor>,

    /// Relationship extractor to use
    pub relationship_extractor: Option<RelationshipExtractor>,

    /// Override visibility (e.g., Public for trait impl members)
    pub visibility_override: Option<Visibility>,

    /// Optional template for overriding parent_scope derivation.
    /// Uses placeholders like {scope}, {abi}, etc.
    /// When set, this template determines the containment relationship parent,
    /// independent of the qualified_name_template.
    /// Useful for extern items where the parent should be the extern block but
    /// the qualified name shouldn't include the extern block path.
    pub parent_scope_template: Option<&'static str>,

    /// Optional list of scope node types to skip when building qualified names.
    /// This is used for entities like TypeScript parameter properties where the
    /// constructor scope should be skipped (e.g., `Point.x` instead of `Point.constructor.x`).
    pub skip_scopes: Option<&'static [&'static str]>,
}

/// Strategy for deriving entity names from query captures
#[derive(Debug, Clone)]
pub enum NameStrategy {
    /// Use a single capture directly
    Capture { name: &'static str },

    /// Try captures in order, use first non-empty
    Fallback { captures: &'static [&'static str] },

    /// Use a template with capture placeholders
    Template { template: &'static str },

    /// Use a static name (fixed string)
    Static { name: &'static str },

    /// Derive name from file path (for modules)
    FilePath,

    /// Use the crate name (for crate root module)
    CrateName,

    /// Use positional index (for tuple struct fields)
    PositionalIndex,
}

/// Available metadata extractors
#[derive(Debug, Clone, Copy)]
pub enum MetadataExtractor {
    FunctionMetadata,
    ArrowFunctionMetadata,
    MethodMetadata,
    ConstMetadata,
    PropertyMetadata,
    StructMetadata,
    EnumMetadata,
    TraitMetadata,
    StaticMetadata,
}

/// Available relationship extractors
#[derive(Debug, Clone, Copy)]
pub enum RelationshipExtractor {
    ExtractFunctionRelationships,
    ExtractClassRelationships,
    ExtractModuleRelationships,
    ExtractTypeRelationships,
    ExtractTraitRelationships,
    ExtractInterfaceRelationships,
    ExtractImplRelationships,
}
