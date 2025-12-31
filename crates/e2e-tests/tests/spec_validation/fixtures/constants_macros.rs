//! constants_macros fixtures for spec validation tests
//!
//! Validates rules:
//! - E-CONST: const declarations produce Constant entities
//! - E-STATIC: static declarations produce Static entities
//! - E-MACRO-RULES: macro_rules! produces Macro entities
//! - E-CONST-ASSOC: associated constants in traits/impls produce Constant entities
//! - V-TRAIT-IMPL-CONST: associated constants in trait impls are effectively Public
//! - M-STATIC-MUTABILITY: static items track mutability

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Constant declarations
///
/// Validates:
/// - E-CONST: const NAME: Type = value; produces Constant
/// - R-CONTAINS-ITEM: Module CONTAINS Constant
pub static CONSTANTS: Fixture = Fixture {
    name: "constants",
    files: &[(
        "lib.rs",
        r#"
pub const MAX_SIZE: usize = 100;
pub const DEFAULT_NAME: &str = "default";
const PRIVATE_CONST: i32 = -1;
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
            qualified_name: "test_crate::DEFAULT_NAME",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "test_crate::PRIVATE_CONST",
            visibility: Some(Visibility::Private),
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
            to: "test_crate::DEFAULT_NAME",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::PRIVATE_CONST",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Static declarations with mutability
///
/// Validates:
/// - E-STATIC: static NAME: Type = value; produces Static
/// - M-STATIC-MUTABILITY: static items track is_mutable field
/// - R-CONTAINS-ITEM: Module CONTAINS Static
pub static STATICS: Fixture = Fixture {
    name: "statics",
    files: &[(
        "lib.rs",
        r#"
pub static IMMUTABLE_GLOBAL: i32 = 42;
pub static mut MUTABLE_GLOBAL: i32 = 0;
static PRIVATE_STATIC: &str = "private";
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Static,
            qualified_name: "test_crate::IMMUTABLE_GLOBAL",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Static,
            qualified_name: "test_crate::MUTABLE_GLOBAL",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Static,
            qualified_name: "test_crate::PRIVATE_STATIC",
            visibility: Some(Visibility::Private),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::IMMUTABLE_GLOBAL",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::MUTABLE_GLOBAL",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::PRIVATE_STATIC",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Union declarations
///
/// Validates:
/// - E-UNION: union Name { ... } produces Union
/// - M-UNION-FIELDS: unions include field information
/// - R-CONTAINS-ITEM: Module CONTAINS Union
pub static UNIONS: Fixture = Fixture {
    name: "unions",
    files: &[(
        "lib.rs",
        r#"
pub union IntOrFloat {
    pub int_val: i32,
    pub float_val: f32,
}

pub union ByteRepresentation {
    pub bytes: [u8; 8],
    pub value: u64,
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
            kind: EntityKind::Union,
            qualified_name: "test_crate::IntOrFloat",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Union,
            qualified_name: "test_crate::ByteRepresentation",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::IntOrFloat",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::ByteRepresentation",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Extern blocks with FFI declarations
///
/// Validates:
/// - E-EXTERN-BLOCK: extern "C" { ... } produces ExternBlock
/// - E-EXTERN-FN: function declaration in extern block produces Function
/// - E-EXTERN-STATIC: static declaration in extern block produces Static
/// - R-CONTAINS-EXTERN-ITEM: ExternBlock CONTAINS Function/Static
/// - R-CONTAINS-ITEM: Module CONTAINS ExternBlock
pub static EXTERN_BLOCKS: Fixture = Fixture {
    name: "extern_blocks",
    files: &[(
        "lib.rs",
        r#"
extern "C" {
    pub fn external_function(x: i32) -> i32;
    pub static EXTERNAL_VALUE: i32;
    fn private_external();
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
            kind: EntityKind::ExternBlock,
            qualified_name: "test_crate::extern \"C\"",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::external_function",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Static,
            qualified_name: "test_crate::EXTERNAL_VALUE",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::private_external",
            visibility: Some(Visibility::Private),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::extern \"C\"",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::extern \"C\"",
            to: "test_crate::external_function",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::extern \"C\"",
            to: "test_crate::EXTERNAL_VALUE",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::extern \"C\"",
            to: "test_crate::private_external",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Associated constants in traits and impls
///
/// Validates:
/// - E-CONST-ASSOC: associated constants produce Constant entities
/// - V-TRAIT-IMPL-CONST: associated constants in trait impls are effectively Public
pub static ASSOCIATED_CONSTANTS: Fixture = Fixture {
    name: "associated_constants",
    files: &[(
        "lib.rs",
        r#"
pub trait WithConstant {
    const DEFAULT: i32;
    const WITH_DEFAULT: i32 = 0;
}

pub struct MyType;

impl WithConstant for MyType {
    const DEFAULT: i32 = 42;
}

impl MyType {
    pub const INHERENT_CONST: &'static str = "inherent";
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
            qualified_name: "test_crate::WithConstant",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::MyType",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::MyType as test_crate::WithConstant>",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::impl test_crate::MyType",
            visibility: None,
        },
        // Associated constant in trait impl - effectively Public per V-TRAIT-IMPL-CONST
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "<test_crate::MyType as test_crate::WithConstant>::DEFAULT",
            visibility: Some(Visibility::Public),
        },
        // Inherent associated constant
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "<test_crate::MyType>::INHERENT_CONST",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::WithConstant",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::MyType",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::<test_crate::MyType as test_crate::WithConstant>",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::impl test_crate::MyType",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::<test_crate::MyType as test_crate::WithConstant>",
            to: "<test_crate::MyType as test_crate::WithConstant>::DEFAULT",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::impl test_crate::MyType",
            to: "<test_crate::MyType>::INHERENT_CONST",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "test_crate::<test_crate::MyType as test_crate::WithConstant>",
            to: "test_crate::WithConstant",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Declarative macros
///
/// Validates:
/// - E-MACRO-RULES: macro_rules! name { ... } produces Macro
/// - R-CONTAINS-ITEM: Module CONTAINS Macro
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

macro_rules! private_macro {
    () => {};
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
        ExpectedEntity {
            kind: EntityKind::Macro,
            qualified_name: "test_crate::private_macro",
            visibility: Some(Visibility::Private),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::my_macro",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::private_macro",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};
