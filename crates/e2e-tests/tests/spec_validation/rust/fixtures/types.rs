//! types fixtures for spec validation tests
//!
//! Validates rules:
//! - E-STRUCT: struct definitions produce Struct entities
//! - E-ENUM: enum definitions produce Enum entities
//! - E-TYPE-ALIAS: type aliases produce TypeAlias entities
//! - V-ENUM-VARIANT: enum variants inherit visibility from their enum
//! - M-STRUCT-FIELDS: struct field metadata
//! - M-ENUM-VARIANTS: enum variant metadata
//! - M-GENERIC: generic type parameter metadata
//! - M-LIFETIMES: lifetime parameter metadata
//! - M-TYPE-ALIAS-TARGET: type alias target type metadata
//! - R-USES-TYPE: type usage in fields/variants creates Uses relationships

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Struct definitions with fields
///
/// Validates:
/// - E-STRUCT: struct definitions produce Struct entities
/// - M-STRUCT-FIELDS: structs include field information
/// - R-USES-TYPE: field types create Uses relationships
pub static STRUCTS: Fixture = Fixture {
    name: "structs",
    files: &[(
        "lib.rs",
        r#"
pub struct Config {
    pub name: String,
}

pub struct Wrapper {
    pub inner: Config,
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
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Config",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Wrapper",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Config",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Wrapper",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "test_crate::Wrapper",
            to: "test_crate::Config",
        },
    ],
    project_type: ProjectType::SingleCrate,
    manifest: None,
};

/// Enum definitions
///
/// Validates:
/// - E-ENUM: enum definitions produce Enum entities
/// - V-ENUM-VARIANT: enum variants inherit visibility from their enum
/// - M-ENUM-VARIANTS: enums include variant information
pub static ENUMS: Fixture = Fixture {
    name: "enums",
    files: &[(
        "lib.rs",
        r#"
pub enum Status {
    Active,
    Inactive,
    Pending(String),
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
            kind: EntityKind::Enum,
            qualified_name: "test_crate::Status",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[ExpectedRelationship {
        kind: RelationshipKind::Contains,
        from: "test_crate",
        to: "test_crate::Status",
    }],
    project_type: ProjectType::SingleCrate,
    manifest: None,
};

/// Type aliases
///
/// Validates:
/// - E-TYPE-ALIAS: type aliases produce TypeAlias entities
/// - M-TYPE-ALIAS-TARGET: type alias tracks target type
/// - R-USES-TYPE: type alias creates Uses relationship to target type components
pub static TYPE_ALIASES: Fixture = Fixture {
    name: "type_aliases",
    files: &[(
        "lib.rs",
        r#"
pub struct Error;
pub type Result<T> = std::result::Result<T, Error>;
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Error",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "test_crate::Result",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Error",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Result",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "test_crate::Result",
            to: "test_crate::Error",
        },
    ],
    project_type: ProjectType::SingleCrate,
    manifest: None,
};

// =============================================================================
// Types Fixtures (Advanced)
// =============================================================================

/// Tuple structs and unit structs
///
/// Validates:
/// - E-STRUCT: all struct variants (named, tuple, unit) produce Struct entities
/// - M-STRUCT-FIELDS: field metadata captures tuple vs named vs unit forms
pub static TUPLE_AND_UNIT_STRUCTS: Fixture = Fixture {
    name: "tuple_and_unit_structs",
    files: &[(
        "lib.rs",
        r#"
// Unit struct (no fields)
pub struct UnitMarker;

// Tuple struct
pub struct Point(pub f64, pub f64);

// Newtype pattern
pub struct UserId(pub u64);

// Regular struct for comparison
pub struct NamedPoint {
    pub x: f64,
    pub y: f64,
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
            kind: EntityKind::Struct,
            qualified_name: "test_crate::UnitMarker",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Point",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::UserId",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::NamedPoint",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::UnitMarker",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Point",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::UserId",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::NamedPoint",
        },
    ],
    project_type: ProjectType::SingleCrate,
    manifest: None,
};

/// Complex enums with various variant types
///
/// Validates:
/// - E-ENUM: enums with complex variants produce Enum entities
/// - M-ENUM-VARIANTS: variant metadata captures different variant kinds (unit, tuple, struct)
/// - R-USES-TYPE: enum variants using other types create Uses relationships
pub static COMPLEX_ENUMS: Fixture = Fixture {
    name: "complex_enums",
    files: &[(
        "lib.rs",
        r#"
pub struct RequestData {
    pub path: String,
}

pub struct ErrorDetails {
    pub code: i32,
    pub message: String,
}

pub enum Message {
    // Unit variant
    Quit,
    // Tuple variant
    Move { x: i32, y: i32 },
    // Named struct variant
    Write(String),
    // Complex variant using other types
    Request(RequestData),
    // Tuple with multiple fields
    Color(u8, u8, u8),
    // Variant with nested enum reference
    Error(ErrorDetails),
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
            kind: EntityKind::Struct,
            qualified_name: "test_crate::RequestData",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::ErrorDetails",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "test_crate::Message",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::RequestData",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::ErrorDetails",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Message",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "test_crate::Message",
            to: "test_crate::RequestData",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "test_crate::Message",
            to: "test_crate::ErrorDetails",
        },
    ],
    project_type: ProjectType::SingleCrate,
    manifest: None,
};

/// Generic structs with type parameters
///
/// Validates:
/// - E-STRUCT: generic structs produce Struct entities
/// - M-GENERIC: struct includes type parameter information
pub static GENERIC_STRUCTS: Fixture = Fixture {
    name: "generic_structs",
    files: &[(
        "lib.rs",
        r#"
pub struct Container<T> {
    pub value: T,
}

pub struct Pair<A, B> {
    pub first: A,
    pub second: B,
}

pub struct BoundedContainer<T: Clone> {
    pub value: T,
}

pub struct MultipleConstraints<T>
where
    T: Clone + Default,
{
    pub value: T,
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
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Container",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Pair",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::BoundedContainer",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::MultipleConstraints",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Container",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Pair",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::BoundedContainer",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::MultipleConstraints",
        },
    ],
    project_type: ProjectType::SingleCrate,
    manifest: None,
};

/// Lifetimes in struct definitions and functions
///
/// Validates:
/// - M-LIFETIMES: entities include lifetime parameter information
/// - E-STRUCT: structs with lifetimes produce Struct entities
/// - E-FN-FREE: functions with lifetimes produce Function entities
pub static LIFETIMES: Fixture = Fixture {
    name: "lifetimes",
    files: &[(
        "lib.rs",
        r#"
pub struct Borrowed<'a> {
    pub data: &'a str,
}

pub struct MultipleBorrows<'a, 'b> {
    pub first: &'a str,
    pub second: &'b str,
}

pub fn borrow_data<'a>(data: &'a str) -> Borrowed<'a> {
    Borrowed { data }
}

pub fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() > b.len() { a } else { b }
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
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Borrowed",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::MultipleBorrows",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::borrow_data",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::longest",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Borrowed",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::MultipleBorrows",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::borrow_data",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::longest",
        },
    ],
    project_type: ProjectType::SingleCrate,
    manifest: None,
};
