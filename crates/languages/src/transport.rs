//! Transport model for intermediate entity representation
//!
//! This module provides serializable intermediate representations for entities
//! extracted from source code that bridge the extraction and storage stages.

use codesearch_core::entities::{CodeEntityBuilder, EntityMetadata, Language, SourceLocation, Visibility};
use codesearch_core::CodeEntity;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Re-export EntityVariant to make it available from this module
pub use crate::generic_entities::EntityVariant;

/// Intermediate representation of an extracted entity
///
/// This structure serves as a transport model between the extraction
/// and transformation stages of the indexing pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityData {
    /// Simple name of the entity
    pub name: String,

    /// Fully qualified name including namespace/module path
    pub qualified_name: String,

    /// Path to the source file containing this entity
    pub file_path: PathBuf,

    /// Source location within the file
    pub location: SourceLocation,

    /// Visibility modifier
    pub visibility: Visibility,

    /// Documentation comment if present
    pub documentation: Option<String>,

    /// Raw source code content
    pub content: Option<String>,

    /// List of dependencies (qualified names of other entities)
    pub dependencies: Vec<String>,

    /// Language-specific variant data
    pub variant: EntityVariant,

    /// Relationships to other entities
    pub relationships: Vec<RelationshipData>,
}

/// Represents a relationship between two entities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipData {
    /// Source entity qualified name
    pub from: String,

    /// Target entity qualified name
    pub to: String,

    /// Type of relationship
    pub relationship_type: RelationshipType,

    /// Optional metadata about the relationship
    pub metadata: Option<RelationshipMetadata>,
}

/// Types of relationships between entities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RelationshipType {
    /// Entity calls/invokes another entity
    Calls,

    /// Entity imports/uses another entity
    Imports,

    /// Entity implements a trait/interface
    Implements,

    /// Entity extends/inherits from another
    Extends,

    /// Entity contains another (e.g., module contains function)
    Contains,

    /// Entity references another (general reference)
    References,

    /// Entity depends on another
    DependsOn,
}

/// Optional metadata for relationships
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipMetadata {
    /// Line number where the relationship occurs
    pub line_number: Option<usize>,

    /// Additional context about the relationship
    pub context: Option<String>,
}

impl EntityData {
    /// Create a new EntityData instance
    pub fn new(
        name: String,
        qualified_name: String,
        file_path: PathBuf,
        location: SourceLocation,
        variant: EntityVariant,
    ) -> Self {
        Self {
            name,
            qualified_name,
            file_path,
            location,
            visibility: Visibility::Private,
            documentation: None,
            content: None,
            dependencies: Vec::new(),
            variant,
            relationships: Vec::new(),
        }
    }

    /// Add a relationship to this entity
    pub fn add_relationship(&mut self, relationship: RelationshipData) {
        self.relationships.push(relationship);
    }

    /// Set the visibility of this entity
    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// Set the documentation for this entity
    pub fn with_documentation(mut self, doc: Option<String>) -> Self {
        self.documentation = doc;
        self
    }

    /// Set the content for this entity
    pub fn with_content(mut self, content: Option<String>) -> Self {
        self.content = content;
        self
    }

    /// Add dependencies to this entity
    pub fn with_dependencies(mut self, deps: Vec<String>) -> Self {
        self.dependencies = deps;
        self
    }
}

/// Convert EntityData to CodeEntity
impl From<EntityData> for CodeEntity {
    fn from(entity: EntityData) -> Self {
        let entity_type = entity.variant.entity_type();
        let metadata = entity.variant.into_metadata();
        let signature = entity.variant.extract_signature();
        let file_path = entity.file_path.clone();
        let location = entity.location.clone();

        CodeEntityBuilder::default()
            .entity_id(format!("{}#{}", file_path.display(), entity.qualified_name))
            .name(entity.name)
            .qualified_name(entity.qualified_name)
            .entity_type(entity_type)
            .location(entity.location)
            .visibility(entity.visibility)
            .documentation_summary(entity.documentation)
            .content(entity.content)
            .dependencies(entity.dependencies)
            .metadata(metadata)
            .signature(signature)
            .language(match &entity.variant {
                EntityVariant::Rust(_) => Language::Rust,
                // Add other languages as they're implemented
            })
            .file_path(file_path)
            .line_range((location.start_line, location.end_line))
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("Failed to build CodeEntity from EntityData: {}", e);
                // Panic is appropriate here as this indicates a programming error
                // All required fields should be present in EntityData
                panic!("Failed to convert EntityData to CodeEntity: {}", e);
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::entities::RustEntityVariant;

    #[test]
    fn test_entity_data_serialization() {
        let entity = EntityData::new(
            "test_function".to_string(),
            "module::test_function".to_string(),
            PathBuf::from("src/lib.rs"),
            SourceLocation {
                start_line: 10,
                end_line: 15,
                start_column: 0,
                end_column: 0,
            },
            EntityVariant::Rust(RustEntityVariant::Function {
                is_async: false,
                is_unsafe: false,
                is_const: false,
                generics: vec![],
                parameters: vec![],
                return_type: None,
            }),
        );

        // Test serialization
        let json = serde_json::to_string(&entity).expect("Failed to serialize EntityData");

        // Test deserialization
        let deserialized: EntityData =
            serde_json::from_str(&json).expect("Failed to deserialize EntityData");

        assert_eq!(deserialized.name, entity.name);
        assert_eq!(deserialized.qualified_name, entity.qualified_name);
    }

    #[test]
    fn test_relationship_data_serialization() {
        let relationship = RelationshipData {
            from: "module::function_a".to_string(),
            to: "module::function_b".to_string(),
            relationship_type: RelationshipType::Calls,
            metadata: Some(RelationshipMetadata {
                line_number: Some(42),
                context: Some("Direct function call".to_string()),
            }),
        };

        let json =
            serde_json::to_string(&relationship).expect("Failed to serialize RelationshipData");

        let deserialized: RelationshipData =
            serde_json::from_str(&json).expect("Failed to deserialize RelationshipData");

        assert_eq!(deserialized.from, relationship.from);
        assert_eq!(deserialized.to, relationship.to);
    }
}
