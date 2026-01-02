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
/// - E-PROPERTY: struct fields produce Property entities
/// - R-CONTAINS-PROPERTY: structs contain their fields
/// - R-USES-TYPE: field types create Uses relationships (from Property entities)
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
            kind: EntityKind::Property,
            qualified_name: "test_crate::Config::name",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Wrapper",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::Wrapper::inner",
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
            from: "test_crate::Config",
            to: "test_crate::Config::name",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Wrapper",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Wrapper",
            to: "test_crate::Wrapper::inner",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "test_crate::Wrapper::inner",
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
/// - E-ENUM-VARIANT: enum variants produce EnumVariant entities
/// - V-ENUM-VARIANT: enum variants inherit visibility from their enum
/// - R-CONTAINS-ENUM-VARIANT: enums contain their variants
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
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "test_crate::Status::Active",
            visibility: None, // Variants inherit visibility, stored as None
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "test_crate::Status::Inactive",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "test_crate::Status::Pending",
            visibility: None,
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Status",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Status",
            to: "test_crate::Status::Active",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Status",
            to: "test_crate::Status::Inactive",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Status",
            to: "test_crate::Status::Pending",
        },
    ],
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
/// - E-PROPERTY: struct fields produce Property entities (including tuple fields)
/// - R-CONTAINS-PROPERTY: structs contain their fields
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
        // Point tuple struct with 2 fields
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Point",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::Point::0",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::Point::1",
            visibility: Some(Visibility::Public),
        },
        // UserId newtype with 1 field
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::UserId",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::UserId::0",
            visibility: Some(Visibility::Public),
        },
        // NamedPoint with 2 named fields
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::NamedPoint",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::NamedPoint::x",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::NamedPoint::y",
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
            from: "test_crate::Point",
            to: "test_crate::Point::0",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Point",
            to: "test_crate::Point::1",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::UserId",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::UserId",
            to: "test_crate::UserId::0",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::NamedPoint",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::NamedPoint",
            to: "test_crate::NamedPoint::x",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::NamedPoint",
            to: "test_crate::NamedPoint::y",
        },
    ],
    project_type: ProjectType::SingleCrate,
    manifest: None,
};

/// Complex enums with various variant types
///
/// Validates:
/// - E-ENUM: enums with complex variants produce Enum entities
/// - E-ENUM-VARIANT: each variant produces an EnumVariant entity
/// - R-CONTAINS-ENUM-VARIANT: enums contain their variants
/// - R-USES-TYPE: variants using other types create Uses relationships
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
    // Struct variant
    Move { x: i32, y: i32 },
    // Tuple variant
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
            kind: EntityKind::Property,
            qualified_name: "test_crate::RequestData::path",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::ErrorDetails",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::ErrorDetails::code",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::ErrorDetails::message",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "test_crate::Message",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "test_crate::Message::Quit",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "test_crate::Message::Move",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "test_crate::Message::Write",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "test_crate::Message::Request",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "test_crate::Message::Color",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::EnumVariant,
            qualified_name: "test_crate::Message::Error",
            visibility: None,
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
            from: "test_crate::RequestData",
            to: "test_crate::RequestData::path",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::ErrorDetails",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::ErrorDetails",
            to: "test_crate::ErrorDetails::code",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::ErrorDetails",
            to: "test_crate::ErrorDetails::message",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Message",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Message",
            to: "test_crate::Message::Quit",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Message",
            to: "test_crate::Message::Move",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Message",
            to: "test_crate::Message::Write",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Message",
            to: "test_crate::Message::Request",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Message",
            to: "test_crate::Message::Color",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Message",
            to: "test_crate::Message::Error",
        },
        // USES relationships now come from the variant entities
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "test_crate::Message::Request",
            to: "test_crate::RequestData",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "test_crate::Message::Error",
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
/// - E-PROPERTY: struct fields produce Property entities
/// - M-GENERIC: struct includes type parameter information
/// - R-CONTAINS-PROPERTY: structs contain their fields
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
            kind: EntityKind::Property,
            qualified_name: "test_crate::Container::value",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Pair",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::Pair::first",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::Pair::second",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::BoundedContainer",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::BoundedContainer::value",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::MultipleConstraints",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::MultipleConstraints::value",
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
            from: "test_crate::Container",
            to: "test_crate::Container::value",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Pair",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Pair",
            to: "test_crate::Pair::first",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::Pair",
            to: "test_crate::Pair::second",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::BoundedContainer",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::BoundedContainer",
            to: "test_crate::BoundedContainer::value",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::MultipleConstraints",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::MultipleConstraints",
            to: "test_crate::MultipleConstraints::value",
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
/// - E-PROPERTY: struct fields produce Property entities
/// - E-FN-FREE: functions with lifetimes produce Function entities
/// - R-CONTAINS-PROPERTY: structs contain their fields
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
            kind: EntityKind::Property,
            qualified_name: "test_crate::Borrowed::data",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::MultipleBorrows",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::MultipleBorrows::first",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Property,
            qualified_name: "test_crate::MultipleBorrows::second",
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
            from: "test_crate::Borrowed",
            to: "test_crate::Borrowed::data",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::MultipleBorrows",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::MultipleBorrows",
            to: "test_crate::MultipleBorrows::first",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::MultipleBorrows",
            to: "test_crate::MultipleBorrows::second",
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
