//! workspace fixtures for spec validation tests
//!
//! Validates rules:
//! - E-MOD: crate root modules produce Module entities
//! - R-CONTAINS-ITEM: modules contain their items
//! - R-IMPORTS: use declarations create Imports relationships
//! - Cross-crate dependency handling

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Basic workspace with cross-crate dependencies
///
/// Validates:
/// - E-MOD: each crate root produces a Module entity
/// - R-CONTAINS-ITEM: modules contain structs and functions
/// - R-IMPORTS: use statements create Imports from module to imported entity
pub static WORKSPACE_BASIC: Fixture = Fixture {
    name: "workspace_basic",
    files: &[
        // Core crate
        (
            "crates/core/Cargo.toml",
            r#"[package]
name = "my_core"
version = "0.1.0"
edition = "2021"
"#,
        ),
        (
            "crates/core/src/lib.rs",
            r#"
pub struct CoreType {
    pub value: i32,
}

pub fn core_function() -> CoreType {
    CoreType { value: 42 }
}
"#,
        ),
        // Utils crate that depends on core
        (
            "crates/utils/Cargo.toml",
            r#"[package]
name = "my_utils"
version = "0.1.0"
edition = "2021"

[dependencies]
my_core = { path = "../core" }
"#,
        ),
        (
            "crates/utils/src/lib.rs",
            r#"
use my_core::CoreType;

pub fn process_core(ct: CoreType) -> i32 {
    ct.value * 2
}
"#,
        ),
    ],
    entities: &[
        // Core crate entities
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "my_core",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "my_core::CoreType",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "my_core::core_function",
            visibility: Some(Visibility::Public),
        },
        // Utils crate entities
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "my_utils",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "my_utils::process_core",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "my_core",
            to: "my_core::CoreType",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "my_core",
            to: "my_core::core_function",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "my_utils",
            to: "my_utils::process_core",
        },
        // Cross-crate import - R-IMPORTS: Module IMPORTS Entity
        ExpectedRelationship {
            kind: RelationshipKind::Imports,
            from: "my_utils",
            to: "my_core::CoreType",
        },
    ],
    project_type: ProjectType::Workspace,
    manifest: Some(
        r#"[workspace]
members = ["crates/*"]
resolver = "2"
"#,
    ),
};
