//! traits fixtures for spec validation tests

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
};

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
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Handler",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Handler::handle",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Handler::with_default",
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
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Handler",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::MyHandler",
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::MyHandler as test_crate::Handler>",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::MyHandler as test_crate::Handler>::handle",
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
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Base",
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Extended",
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
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Iterator",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Counter",
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::Counter as test_crate::Iterator>",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::Counter as test_crate::Iterator>::next",
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "test_crate::Counter::Item",
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
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Counter",
            to: "test_crate::Counter::Item",
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
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Display",
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Debug",
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Clone",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Value",
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::Value as test_crate::Display>",
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::Value as test_crate::Debug>",
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::Value as test_crate::Clone>",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::Value as test_crate::Display>::display",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::Value as test_crate::Debug>::debug",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::Value as test_crate::Clone>::clone",
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
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Transformer",
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::BoundedTransformer",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::StringToInt",
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::StringToInt as test_crate::Transformer>",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::StringToInt as test_crate::Transformer>::transform",
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
