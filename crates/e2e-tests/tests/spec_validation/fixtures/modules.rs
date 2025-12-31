//! modules fixtures for spec validation tests

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Basic module declaration with file-based module
pub static BASIC_MOD: Fixture = Fixture {
    name: "basic_mod",
    files: &[
        ("lib.rs", "pub mod foo;\n"),
        ("foo.rs", "pub fn bar() {}\n"),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::foo",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::foo::bar",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::foo",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::foo",
            to: "test_crate::foo::bar",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Visibility modifiers: pub, pub(crate), private
pub static VISIBILITY: Fixture = Fixture {
    name: "visibility",
    files: &[(
        "lib.rs",
        r#"
pub fn public_fn() {}
pub(crate) fn crate_fn() {}
fn private_fn() {}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::public_fn",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::crate_fn",
            visibility: Some(Visibility::Internal),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::private_fn",
            visibility: Some(Visibility::Private),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::public_fn",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::crate_fn",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::private_fn",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Use declarations and imports
pub static USE_IMPORTS: Fixture = Fixture {
    name: "use_imports",
    files: &[
        (
            "lib.rs",
            "pub mod utils;\nuse crate::utils::helper;\n\npub fn caller() { helper(); }\n",
        ),
        ("utils.rs", "pub fn helper() {}\n"),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::utils",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::caller",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::utils::helper",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::utils",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::caller",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::utils",
            to: "test_crate::utils::helper",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Re-exports with pub use
pub static REEXPORTS: Fixture = Fixture {
    name: "reexports",
    files: &[
        ("lib.rs", "mod internal;\npub use internal::helper;\n"),
        ("internal.rs", "pub fn helper() {}\n"),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::internal",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::internal::helper",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::internal",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::internal",
            to: "test_crate::internal::helper",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

// =============================================================================
// Module System Fixtures (Advanced)
// =============================================================================

/// Deep module nesting (3+ levels) with mixed inline and file-based modules
pub static DEEP_MODULE_NESTING: Fixture = Fixture {
    name: "deep_module_nesting",
    files: &[(
        "lib.rs",
        r#"
pub mod level1 {
    pub mod level2 {
        pub mod level3 {
            pub fn deep_function() {}
        }
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
            kind: EntityKind::Module,
            qualified_name: "test_crate::level1",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::level1::level2",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::level1::level2::level3",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::level1::level2::level3::deep_function",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::level1",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::level1",
            to: "test_crate::level1::level2",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::level1::level2",
            to: "test_crate::level1::level2::level3",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::level1::level2::level3",
            to: "test_crate::level1::level2::level3::deep_function",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Mixed inline and file-based modules with directory structure
pub static MIXED_MODULE_STRUCTURE: Fixture = Fixture {
    name: "mixed_module_structure",
    files: &[
        (
            "lib.rs",
            r#"
pub mod api;          // file-based
pub mod utils {       // inline
    pub fn helper() {}
}
"#,
        ),
        (
            "api/mod.rs",
            r#"
pub mod handlers;     // file-based inside directory
pub fn api_root() {}
"#,
        ),
        (
            "api/handlers.rs",
            r#"
pub fn handle_request() {}
"#,
        ),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::api",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::api::handlers",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::utils",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::api::api_root",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::api::handlers::handle_request",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::utils::helper",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::api",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::utils",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::api",
            to: "test_crate::api::handlers",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::api",
            to: "test_crate::api::api_root",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::api::handlers",
            to: "test_crate::api::handlers::handle_request",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::utils",
            to: "test_crate::utils::helper",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Self and super references in modules
pub static SELF_SUPER_REFERENCES: Fixture = Fixture {
    name: "self_super_references",
    files: &[(
        "lib.rs",
        r#"
pub fn root_fn() {}

pub mod child {
    use super::root_fn;

    pub fn child_fn() {
        root_fn();
    }

    pub mod grandchild {
        use super::super::root_fn;
        use super::child_fn;

        pub fn grandchild_fn() {
            root_fn();
            child_fn();
        }
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
            kind: EntityKind::Module,
            qualified_name: "test_crate::child",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::child::grandchild",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::root_fn",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::child::child_fn",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::child::grandchild::grandchild_fn",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::root_fn",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::child",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::child",
            to: "test_crate::child::child_fn",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::child",
            to: "test_crate::child::grandchild",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::child::grandchild",
            to: "test_crate::child::grandchild::grandchild_fn",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::child::child_fn",
            to: "test_crate::root_fn",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::child::grandchild::grandchild_fn",
            to: "test_crate::root_fn",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::child::grandchild::grandchild_fn",
            to: "test_crate::child::child_fn",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};
