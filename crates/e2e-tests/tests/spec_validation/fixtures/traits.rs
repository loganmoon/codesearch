//! traits fixtures for spec validation tests
//!
//! Validates rules:
//! - E-TRAIT: trait definitions produce Trait entities
//! - E-METHOD-TRAIT-DEF: trait method definitions produce Method entities
//! - E-METHOD-TRAIT-IMPL: trait method implementations produce Method entities
//! - E-IMPL-TRAIT: trait impl blocks produce ImplBlock entities
//! - E-TYPE-ALIAS-ASSOC: associated type definitions produce TypeAlias entities
//! - V-TRAIT-METHOD-DEF: trait method definitions have no visibility (None)
//! - V-TRAIT-IMPL-METHOD: trait impl methods are effectively Public
//! - V-TRAIT-IMPL-ASSOC-TYPE: associated types in impls are effectively Public
//! - Q-IMPL-TRAIT: trait impls use "<{type_fqn} as {trait_fqn}>" format
//! - Q-TRAIT-IMPL-METHOD: trait impl methods use "<{type_fqn} as {trait_fqn}>::{name}" format
//! - Q-ASSOC-TYPE: associated types use "<{type_fqn} as {trait_fqn}>::{name}" format
//! - Q-TRAIT-METHOD-DEF: trait method definitions use "{trait_fqn}::{name}" format
//! - R-IMPLEMENTS: impl block IMPLEMENTS trait
//! - R-EXTENDS-INTERFACE: subtrait EXTENDS_INTERFACE supertrait
//! - R-CONTAINS-TRAIT-MEMBER: trait CONTAINS method definitions
//! - R-CONTAINS-IMPL-MEMBER: impl block CONTAINS method implementations
//! - R-CONTAINS-ASSOC-TYPE: trait/impl CONTAINS associated types
//! - M-TRAIT-BOUNDS: trait includes supertrait bounds metadata
//! - M-TRAIT-METHODS: trait includes method definition metadata
//! - M-TRAIT-ASSOC-TYPES: trait includes associated type metadata

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Trait definitions with method declarations
///
/// Validates:
/// - E-TRAIT: trait definition produces Trait entity
/// - E-METHOD-TRAIT-DEF: trait method definition produces Method entity
/// - V-TRAIT-METHOD-DEF: trait method definitions have no visibility (None)
/// - Q-TRAIT-METHOD-DEF: trait methods use "{trait_fqn}::{name}" format
/// - R-CONTAINS-TRAIT-MEMBER: Trait CONTAINS Method
/// - M-TRAIT-METHODS: trait includes method definition metadata
pub static TRAIT_DEF: Fixture = Fixture {
    name: "trait_def",
    files: &[(
        "lib.rs",
        r#"
pub trait Handler {
    fn handle(&self);
    fn with_default(&self) {}
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Handler",
            visibility: Some(Visibility::Public),
        },
        // V-TRAIT-METHOD-DEF: trait method definitions have no visibility
        // Q-TRAIT-METHOD-DEF: trait methods use "{trait_fqn}::{name}" format
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Handler::handle",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Handler::with_default",
            visibility: None,
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Handler",
        },
        // R-CONTAINS-TRAIT-MEMBER: Trait CONTAINS method definitions
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Handler",
            to: "test_crate::Handler::handle",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Handler",
            to: "test_crate::Handler::with_default",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Trait implementations
///
/// Validates:
/// - E-IMPL-TRAIT: trait impl block produces ImplBlock entity
/// - E-METHOD-TRAIT-IMPL: trait method implementation produces Method entity
/// - V-TRAIT-IMPL-METHOD: trait impl methods are effectively Public
/// - Q-IMPL-TRAIT: impl block uses "{module}::<{type_fqn} as {trait_fqn}>" format
/// - Q-TRAIT-IMPL-METHOD: impl methods use "<{type_fqn} as {trait_fqn}>::{name}" format
/// - R-IMPLEMENTS: impl block IMPLEMENTS trait
/// - R-CONTAINS-IMPL-MEMBER: impl block CONTAINS method implementations
pub static TRAIT_IMPL: Fixture = Fixture {
    name: "trait_impl",
    files: &[(
        "lib.rs",
        r#"
pub trait Handler {
    fn handle(&self);
}

pub struct MyHandler;

impl Handler for MyHandler {
    fn handle(&self) {}
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Handler",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::MyHandler",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::MyHandler as test_crate::Handler>",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::MyHandler as test_crate::Handler>::handle",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Handler",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::MyHandler",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::<test_crate::MyHandler as test_crate::Handler>",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::<test_crate::MyHandler as test_crate::Handler>",
            to: "<test_crate::MyHandler as test_crate::Handler>::handle",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "test_crate::<test_crate::MyHandler as test_crate::Handler>",
            to: "test_crate::Handler",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Supertraits (trait bounds)
///
/// Validates:
/// - R-EXTENDS-INTERFACE: subtrait EXTENDS_INTERFACE supertrait
/// - M-TRAIT-BOUNDS: trait includes supertrait bounds metadata
pub static SUPERTRAITS: Fixture = Fixture {
    name: "supertraits",
    files: &[(
        "lib.rs",
        r#"
pub trait Base {
    fn base_method(&self);
}

pub trait Extended: Base {
    fn extended_method(&self);
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Base",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Extended",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Base",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Extended",
        },
        ExpectedRelationship {
            kind: RelationshipKind::ExtendsInterface,
            from: "test_crate::Extended",
            to: "test_crate::Base",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

// =============================================================================
// Traits Fixtures (Advanced)
// =============================================================================

/// Traits with associated types
///
/// Validates:
/// - E-TYPE-ALIAS-ASSOC: associated type definition produces TypeAlias entity
/// - V-TRAIT-IMPL-ASSOC-TYPE: associated types in trait impls are effectively Public
/// - Q-ASSOC-TYPE: associated types use "<{type_fqn} as {trait_fqn}>::{name}" format
/// - R-CONTAINS-ASSOC-TYPE: impl block CONTAINS associated type definition
/// - M-TRAIT-ASSOC-TYPES: trait includes associated type metadata
pub static ASSOCIATED_TYPES: Fixture = Fixture {
    name: "associated_types",
    files: &[(
        "lib.rs",
        r#"
pub trait Iterator {
    type Item;
    fn next(&mut self) -> Option<Self::Item>;
}

pub struct Counter {
    count: u32,
}

impl Iterator for Counter {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        self.count += 1;
        Some(self.count)
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Iterator",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Counter",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::Counter as test_crate::Iterator>",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::Counter as test_crate::Iterator>::next",
            visibility: Some(Visibility::Public),
        },
        // Q-ASSOC-TYPE: associated types use UFCS format
        // V-TRAIT-IMPL-ASSOC-TYPE: effectively Public
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "<test_crate::Counter as test_crate::Iterator>::Item",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Iterator",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Counter",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::<test_crate::Counter as test_crate::Iterator>",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::<test_crate::Counter as test_crate::Iterator>",
            to: "<test_crate::Counter as test_crate::Iterator>::next",
        },
        // R-CONTAINS-ASSOC-TYPE: impl block contains associated type
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::<test_crate::Counter as test_crate::Iterator>",
            to: "<test_crate::Counter as test_crate::Iterator>::Item",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "test_crate::<test_crate::Counter as test_crate::Iterator>",
            to: "test_crate::Iterator",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Multiple trait implementations for one type
///
/// Validates:
/// - Multiple impl blocks for same type implementing different traits
/// - Each impl block produces distinct ImplBlock entity
/// - Each impl block has separate IMPLEMENTS relationship
pub static MULTIPLE_TRAIT_IMPLS: Fixture = Fixture {
    name: "multiple_trait_impls",
    files: &[(
        "lib.rs",
        r#"
pub trait Display {
    fn display(&self) -> String;
}

pub trait Debug {
    fn debug(&self) -> String;
}

pub trait Clone {
    fn clone(&self) -> Self;
}

pub struct Value {
    pub data: i32,
}

impl Display for Value {
    fn display(&self) -> String {
        format!("{}", self.data)
    }
}

impl Debug for Value {
    fn debug(&self) -> String {
        format!("Value {{ data: {} }}", self.data)
    }
}

impl Clone for Value {
    fn clone(&self) -> Self {
        Value { data: self.data }
    }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Display",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Debug",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Clone",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Value",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::Value as test_crate::Display>",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::Value as test_crate::Debug>",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::Value as test_crate::Clone>",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::Value as test_crate::Display>::display",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::Value as test_crate::Debug>::debug",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::Value as test_crate::Clone>::clone",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Display",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Debug",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Clone",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Value",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::<test_crate::Value as test_crate::Display>",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::<test_crate::Value as test_crate::Debug>",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::<test_crate::Value as test_crate::Clone>",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::<test_crate::Value as test_crate::Display>",
            to: "<test_crate::Value as test_crate::Display>::display",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::<test_crate::Value as test_crate::Debug>",
            to: "<test_crate::Value as test_crate::Debug>::debug",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::<test_crate::Value as test_crate::Clone>",
            to: "<test_crate::Value as test_crate::Clone>::clone",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "test_crate::<test_crate::Value as test_crate::Display>",
            to: "test_crate::Display",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "test_crate::<test_crate::Value as test_crate::Debug>",
            to: "test_crate::Debug",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "test_crate::<test_crate::Value as test_crate::Clone>",
            to: "test_crate::Clone",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Generic trait with bounds
///
/// Validates:
/// - E-TRAIT: generic traits produce Trait entities
/// - M-GENERIC: trait includes generic parameter information
/// - M-TRAIT-BOUNDS: trait parameter bounds are tracked
pub static GENERIC_TRAIT: Fixture = Fixture {
    name: "generic_trait",
    files: &[(
        "lib.rs",
        r#"
pub trait Transformer<T, U> {
    fn transform(&self, input: T) -> U;
}

pub struct StringToInt;

impl Transformer<String, i32> for StringToInt {
    fn transform(&self, input: String) -> i32 {
        input.parse().unwrap_or(0)
    }
}

pub trait BoundedTransformer<T: Clone, U: Default> {
    fn transform_bounded(&self, input: T) -> U;
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Transformer",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::BoundedTransformer",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::StringToInt",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::StringToInt as test_crate::Transformer>",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::StringToInt as test_crate::Transformer>::transform",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Transformer",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::BoundedTransformer",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::StringToInt",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::<test_crate::StringToInt as test_crate::Transformer>",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::<test_crate::StringToInt as test_crate::Transformer>",
            to: "<test_crate::StringToInt as test_crate::Transformer>::transform",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "test_crate::<test_crate::StringToInt as test_crate::Transformer>",
            to: "test_crate::Transformer",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};
