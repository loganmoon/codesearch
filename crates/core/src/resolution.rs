//! Relationship resolution types for the generic resolver framework
//!
//! This module defines the configuration types used by the GenericResolver
//! to handle different relationship types across languages.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::entities::{EntityType, RelationshipType};

/// Lookup strategy for resolving entity references
///
/// Strategies are tried in order until a match is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupStrategy {
    /// Match by fully qualified name (e.g., "crate::module::Function")
    QualifiedName,
    /// Match by path-based entity identifier (e.g., "src.module.Function")
    PathEntityIdentifier,
    /// Match by pre-computed call aliases (for Rust UFCS: Type::method -> <Type as Trait>::method)
    CallAliases,
    /// Match by simple name only if unambiguous (exactly one entity with that name)
    UniqueSimpleName,
    /// Match by simple name when multiple entities share the name.
    /// First match wins; logs a warning on ambiguity but still creates the relationship.
    SimpleName,
}

/// Definition of a relationship type for resolution
///
/// This struct configures how a specific relationship type is resolved,
/// including source/target entity types, relationship name,
/// and the lookup strategy chain to use.
#[derive(Debug, Clone)]
pub struct RelationshipDef {
    /// Name of this relationship definition (for logging/debugging)
    pub name: &'static str,
    /// Entity types that can be sources of this relationship
    pub source_types: &'static [EntityType],
    /// Entity types that can be targets of this relationship
    pub target_types: &'static [EntityType],
    /// The relationship type
    pub forward_rel: RelationshipType,
    /// Ordered list of lookup strategies to try
    pub lookup_strategies: &'static [LookupStrategy],
}

impl RelationshipDef {
    /// Create a new relationship definition
    pub const fn new(
        name: &'static str,
        source_types: &'static [EntityType],
        target_types: &'static [EntityType],
        forward_rel: RelationshipType,
        lookup_strategies: &'static [LookupStrategy],
    ) -> Self {
        Self {
            name,
            source_types,
            target_types,
            forward_rel,
            lookup_strategies,
        }
    }

    /// Validate the relationship definition
    ///
    /// Returns an error message if the definition is invalid.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.is_empty() {
            return Err("RelationshipDef name cannot be empty");
        }
        if self.source_types.is_empty() {
            return Err("RelationshipDef source_types cannot be empty");
        }
        if self.target_types.is_empty() {
            return Err("RelationshipDef target_types cannot be empty");
        }
        if self.lookup_strategies.is_empty() {
            return Err("RelationshipDef lookup_strategies cannot be empty");
        }
        Ok(())
    }
}

/// Standard relationship definitions used across languages
pub mod definitions {
    use super::*;

    /// All callable entity types (functions, methods)
    pub const CALLABLE_TYPES: &[EntityType] = &[EntityType::Function, EntityType::Method];

    /// All type entity types (struct, enum, class, interface, trait, type alias)
    pub const TYPE_TYPES: &[EntityType] = &[
        EntityType::Struct,
        EntityType::Enum,
        EntityType::Class,
        EntityType::Interface,
        EntityType::Trait,
        EntityType::TypeAlias,
    ];

    /// All impl block types
    pub const IMPL_TYPES: &[EntityType] = &[EntityType::Impl];

    /// All module types
    pub const MODULE_TYPES: &[EntityType] = &[EntityType::Module];

    /// CALLS relationship: Function/Method calls other Function/Method
    pub const CALLS: RelationshipDef = RelationshipDef::new(
        "calls",
        CALLABLE_TYPES,
        CALLABLE_TYPES,
        RelationshipType::Calls,
        &[
            LookupStrategy::QualifiedName,
            LookupStrategy::CallAliases,
            LookupStrategy::UniqueSimpleName,
        ],
    );

    /// USES relationship: Entity uses a type
    pub const USES: RelationshipDef = RelationshipDef::new(
        "uses",
        &[
            EntityType::Function,
            EntityType::Method,
            EntityType::Struct,
            EntityType::Enum,
            EntityType::Class,
            EntityType::Interface,
            EntityType::Trait,
            EntityType::TypeAlias,
            EntityType::Impl,
        ],
        TYPE_TYPES,
        RelationshipType::Uses,
        &[LookupStrategy::QualifiedName, LookupStrategy::SimpleName],
    );

    /// IMPLEMENTS relationship: Impl block implements a trait for a type
    pub const IMPLEMENTS: RelationshipDef = RelationshipDef::new(
        "implements",
        IMPL_TYPES,
        &[EntityType::Trait, EntityType::Interface],
        RelationshipType::Implements,
        &[LookupStrategy::QualifiedName],
    );

    /// ASSOCIATES relationship: Impl block associates with a type
    pub const ASSOCIATES: RelationshipDef = RelationshipDef::new(
        "associates",
        IMPL_TYPES,
        TYPE_TYPES,
        RelationshipType::Associates,
        &[LookupStrategy::QualifiedName],
    );

    /// EXTENDS relationship: Trait extends another trait (supertraits)
    pub const EXTENDS: RelationshipDef = RelationshipDef::new(
        "extends",
        &[EntityType::Trait, EntityType::Interface],
        &[EntityType::Trait, EntityType::Interface],
        RelationshipType::ExtendsInterface,
        &[LookupStrategy::QualifiedName],
    );

    /// INHERITS relationship: Class inherits from another class
    pub const INHERITS: RelationshipDef = RelationshipDef::new(
        "inherits",
        &[EntityType::Class],
        &[EntityType::Class],
        RelationshipType::InheritsFrom,
        &[LookupStrategy::QualifiedName, LookupStrategy::SimpleName],
    );

    /// IMPORTS relationship: Entity imports another entity
    ///
    /// Note: The old ImportsResolver processed any entity with an `imports` attribute,
    /// not just modules. We maintain that behavior for backward compatibility.
    pub const IMPORTS: RelationshipDef = RelationshipDef::new(
        "imports",
        &[
            EntityType::Module,
            EntityType::Function,
            EntityType::Method,
            EntityType::Class,
            EntityType::Struct,
            EntityType::Enum,
            EntityType::Trait,
            EntityType::Interface,
            EntityType::TypeAlias,
            EntityType::Impl,
        ],
        &[
            EntityType::Module,
            EntityType::Function,
            EntityType::Class,
            EntityType::Struct,
            EntityType::Enum,
            EntityType::Trait,
            EntityType::Interface,
            EntityType::TypeAlias,
            EntityType::Constant,
        ],
        RelationshipType::Imports,
        &[
            LookupStrategy::QualifiedName,
            LookupStrategy::PathEntityIdentifier,
            LookupStrategy::SimpleName,
        ],
    );

    /// CONTAINS relationship: Parent scope contains child entity
    pub const CONTAINS: RelationshipDef = RelationshipDef::new(
        "contains",
        &[
            EntityType::Module,
            EntityType::Class,
            EntityType::Struct,
            EntityType::Enum,
            EntityType::Trait,
            EntityType::Interface,
            EntityType::Impl,
        ],
        &[
            EntityType::Function,
            EntityType::Method,
            EntityType::Class,
            EntityType::Struct,
            EntityType::Enum,
            EntityType::Trait,
            EntityType::Interface,
            EntityType::Constant,
            EntityType::TypeAlias,
            EntityType::Module,
        ],
        RelationshipType::Contains,
        &[LookupStrategy::QualifiedName],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relationship_def_creation() {
        let def = RelationshipDef::new(
            "test",
            &[EntityType::Function],
            &[EntityType::Method],
            RelationshipType::Calls,
            &[LookupStrategy::QualifiedName],
        );

        assert_eq!(def.name, "test");
        assert_eq!(def.source_types, &[EntityType::Function]);
        assert_eq!(def.target_types, &[EntityType::Method]);
        assert_eq!(def.forward_rel, RelationshipType::Calls);
        assert_eq!(def.lookup_strategies, &[LookupStrategy::QualifiedName]);
    }

    #[test]
    fn test_standard_definitions() {
        // CALLS should have the right forward relationship
        assert_eq!(definitions::CALLS.forward_rel, RelationshipType::Calls);

        // USES should target type entities
        assert!(!definitions::USES.target_types.is_empty());
    }

    #[test]
    fn test_validation_valid() {
        let def = RelationshipDef::new(
            "test",
            &[EntityType::Function],
            &[EntityType::Method],
            RelationshipType::Calls,
            &[LookupStrategy::QualifiedName],
        );
        assert!(def.validate().is_ok());
    }

    #[test]
    fn test_validation_empty_name() {
        let def = RelationshipDef::new(
            "",
            &[EntityType::Function],
            &[EntityType::Method],
            RelationshipType::Calls,
            &[LookupStrategy::QualifiedName],
        );
        assert_eq!(def.validate(), Err("RelationshipDef name cannot be empty"));
    }

    #[test]
    fn test_validation_empty_source_types() {
        let def = RelationshipDef::new(
            "test",
            &[],
            &[EntityType::Method],
            RelationshipType::Calls,
            &[LookupStrategy::QualifiedName],
        );
        assert_eq!(
            def.validate(),
            Err("RelationshipDef source_types cannot be empty")
        );
    }

    #[test]
    fn test_validation_all_standard_definitions() {
        // All standard definitions should be valid
        assert!(definitions::CALLS.validate().is_ok());
        assert!(definitions::USES.validate().is_ok());
        assert!(definitions::IMPLEMENTS.validate().is_ok());
        assert!(definitions::ASSOCIATES.validate().is_ok());
        assert!(definitions::EXTENDS.validate().is_ok());
        assert!(definitions::INHERITS.validate().is_ok());
        assert!(definitions::IMPORTS.validate().is_ok());
        assert!(definitions::CONTAINS.validate().is_ok());
    }
}
