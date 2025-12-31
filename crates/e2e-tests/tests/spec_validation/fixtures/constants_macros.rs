//! constants_macros fixtures for spec validation tests

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

pub static CONSTANTS: Fixture = Fixture {
    name: "constants",
    files: &[(
        "lib.rs",
        r#"
pub const MAX_SIZE: usize = 100;
pub static GLOBAL_VALUE: i32 = 42;
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "test_crate::MAX_SIZE",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "test_crate::GLOBAL_VALUE",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::MAX_SIZE",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::GLOBAL_VALUE",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Declarative macros
pub static MACRO_RULES: Fixture = Fixture {
    name: "macro_rules",
    files: &[(
        "lib.rs",
        r#"
#[macro_export]
macro_rules! my_macro {
    () => {};
    ($x:expr) => { $x };
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
            kind: EntityKind::Macro,
            qualified_name: "test_crate::my_macro",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[ExpectedRelationship {
        kind: RelationshipKind::Contains,
        from: "test_crate",
        to: "test_crate::my_macro",
    }],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};
