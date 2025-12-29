//! Specification-based graph validation tests
//!
//! These tests validate that the code graph extraction pipeline correctly
//! identifies entities and relationships from Rust source code by comparing
//! against hand-verified expected specifications.
//!
//! Run with: cargo test --manifest-path crates/e2e-tests/Cargo.toml test_spec_validation -- --ignored

use anyhow::Result;
use codesearch_e2e_tests::common::spec_validation::{
    run_spec_validation, EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType,
    RelationshipKind,
};

// =============================================================================
// Module System Fixtures (Basic)
// =============================================================================

/// Basic module declaration with file-based module
static BASIC_MOD: Fixture = Fixture {
    name: "basic_mod",
    files: &[
        ("lib.rs", "pub mod foo;\n"),
        ("foo.rs", "pub fn bar() {}\n"),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::foo",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::foo::bar",
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
static VISIBILITY: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::public_fn",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::crate_fn",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::private_fn",
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
static USE_IMPORTS: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::utils",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::caller",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::utils::helper",
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
static REEXPORTS: Fixture = Fixture {
    name: "reexports",
    files: &[
        ("lib.rs", "mod internal;\npub use internal::helper;\n"),
        ("internal.rs", "pub fn helper() {}\n"),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::internal",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::internal::helper",
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
static DEEP_MODULE_NESTING: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::level1",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::level1::level2",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::level1::level2::level3",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::level1::level2::level3::deep_function",
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
static MIXED_MODULE_STRUCTURE: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::api",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::api::handlers",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::utils",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::api::api_root",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::api::handlers::handle_request",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::utils::helper",
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
static SELF_SUPER_REFERENCES: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::child",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::child::grandchild",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::root_fn",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::child::child_fn",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::child::grandchild::grandchild_fn",
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

// =============================================================================
// Functions & Calls Fixtures (Basic)
// =============================================================================

/// Free functions with calls between them
static FREE_FUNCTIONS: Fixture = Fixture {
    name: "free_functions",
    files: &[(
        "lib.rs",
        r#"
pub fn caller() {
    callee();
}

pub fn callee() {}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::caller",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::callee",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::caller",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::callee",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::caller",
            to: "test_crate::callee",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Methods in inherent impl blocks
static METHODS: Fixture = Fixture {
    name: "methods",
    files: &[(
        "lib.rs",
        r#"
pub struct Foo;

impl Foo {
    pub fn method(&self) {}

    pub fn associated() {}
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Foo",
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::impl test_crate::Foo",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Foo::method",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::Foo::associated",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Foo",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::impl test_crate::Foo",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::impl test_crate::Foo",
            to: "test_crate::Foo::method",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::impl test_crate::Foo",
            to: "test_crate::Foo::associated",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Cross-module function calls
static CROSS_MODULE_CALLS: Fixture = Fixture {
    name: "cross_module_calls",
    files: &[
        (
            "lib.rs",
            r#"
pub mod utils;

pub fn main_caller() {
    utils::helper();
}
"#,
        ),
        ("utils.rs", "pub fn helper() {}\n"),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::utils",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::main_caller",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::utils::helper",
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
            to: "test_crate::main_caller",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::utils",
            to: "test_crate::utils::helper",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::main_caller",
            to: "test_crate::utils::helper",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

// =============================================================================
// Functions & Calls Fixtures (Advanced)
// =============================================================================

/// Multiple impl blocks for the same type
static MULTIPLE_IMPL_BLOCKS: Fixture = Fixture {
    name: "multiple_impl_blocks",
    files: &[(
        "lib.rs",
        r#"
pub struct Counter {
    value: i32,
}

// First impl block - constructors
impl Counter {
    pub fn new() -> Self {
        Self { value: 0 }
    }

    pub fn with_value(value: i32) -> Self {
        Self { value }
    }
}

// Second impl block - methods
impl Counter {
    pub fn increment(&mut self) {
        self.value += 1;
    }

    pub fn get(&self) -> i32 {
        self.value
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
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Counter",
        },
        // Note: Two separate impl blocks, both for Counter
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Counter::new",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Counter::with_value",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Counter::increment",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Counter::get",
        },
    ],
    relationships: &[ExpectedRelationship {
        kind: RelationshipKind::Contains,
        from: "test_crate",
        to: "test_crate::Counter",
    }],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Async functions
static ASYNC_FUNCTIONS: Fixture = Fixture {
    name: "async_functions",
    files: &[(
        "lib.rs",
        r#"
pub async fn async_caller() {
    async_callee().await;
}

pub async fn async_callee() {}

pub struct AsyncService;

impl AsyncService {
    pub async fn process(&self) {
        async_callee().await;
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
            kind: EntityKind::Function,
            qualified_name: "test_crate::async_caller",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::async_callee",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::AsyncService",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::AsyncService::process",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::async_caller",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::async_callee",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::AsyncService",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::async_caller",
            to: "test_crate::async_callee",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::AsyncService::process",
            to: "test_crate::async_callee",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Method chaining / builder pattern
static BUILDER_PATTERN: Fixture = Fixture {
    name: "builder_pattern",
    files: &[(
        "lib.rs",
        r#"
pub struct ConfigBuilder {
    name: Option<String>,
    value: Option<i32>,
}

pub struct Config {
    pub name: String,
    pub value: i32,
}

impl ConfigBuilder {
    pub fn new() -> Self {
        Self { name: None, value: None }
    }

    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }

    pub fn value(mut self, value: i32) -> Self {
        self.value = Some(value);
        self
    }

    pub fn build(self) -> Config {
        Config {
            name: self.name.unwrap_or_default(),
            value: self.value.unwrap_or(0),
        }
    }
}

pub fn create_config() -> Config {
    ConfigBuilder::new()
        .name("test")
        .value(42)
        .build()
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::ConfigBuilder",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Config",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::ConfigBuilder::new",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::ConfigBuilder::name",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::ConfigBuilder::value",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::ConfigBuilder::build",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_config",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::ConfigBuilder",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Config",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::create_config",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::create_config",
            to: "test_crate::ConfigBuilder::new",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::create_config",
            to: "test_crate::ConfigBuilder::name",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::create_config",
            to: "test_crate::ConfigBuilder::value",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::create_config",
            to: "test_crate::ConfigBuilder::build",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Recursive function calls
static RECURSIVE_CALLS: Fixture = Fixture {
    name: "recursive_calls",
    files: &[(
        "lib.rs",
        r#"
pub fn factorial(n: u64) -> u64 {
    if n <= 1 {
        1
    } else {
        n * factorial(n - 1)
    }
}

pub fn mutually_recursive_a(n: u32) -> u32 {
    if n == 0 { 0 } else { mutually_recursive_b(n - 1) }
}

pub fn mutually_recursive_b(n: u32) -> u32 {
    if n == 0 { 0 } else { mutually_recursive_a(n - 1) + 1 }
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::factorial",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::mutually_recursive_a",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::mutually_recursive_b",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::factorial",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::mutually_recursive_a",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::mutually_recursive_b",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::factorial",
            to: "test_crate::factorial",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::mutually_recursive_a",
            to: "test_crate::mutually_recursive_b",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::mutually_recursive_b",
            to: "test_crate::mutually_recursive_a",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

// =============================================================================
// Types Fixtures (Basic)
// =============================================================================

/// Struct definitions with fields that use other types
static STRUCTS: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Config",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Wrapper",
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
    cargo_toml: None,
};

/// Enum definitions
static ENUMS: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "test_crate::Status",
        },
    ],
    relationships: &[ExpectedRelationship {
        kind: RelationshipKind::Contains,
        from: "test_crate",
        to: "test_crate::Status",
    }],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Type aliases
static TYPE_ALIASES: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Error",
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "test_crate::Result",
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
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

// =============================================================================
// Types Fixtures (Advanced)
// =============================================================================

/// Tuple structs and unit structs
static TUPLE_AND_UNIT_STRUCTS: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::UnitMarker",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Point",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::UserId",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::NamedPoint",
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
    cargo_toml: None,
};

/// Complex enums with various variant types
static COMPLEX_ENUMS: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::RequestData",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::ErrorDetails",
        },
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "test_crate::Message",
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
    cargo_toml: None,
};

/// Generic structs with type parameters
static GENERIC_STRUCTS: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Container",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Pair",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::BoundedContainer",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::MultipleConstraints",
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
    cargo_toml: None,
};

/// Lifetimes in struct definitions and functions
static LIFETIMES: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Borrowed",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::MultipleBorrows",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::borrow_data",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::longest",
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
    cargo_toml: None,
};

// =============================================================================
// Traits Fixtures (Basic)
// =============================================================================

/// Trait definitions
static TRAIT_DEF: Fixture = Fixture {
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
    ],
    relationships: &[ExpectedRelationship {
        kind: RelationshipKind::Contains,
        from: "test_crate",
        to: "test_crate::Handler",
    }],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Trait implementations
static TRAIT_IMPL: Fixture = Fixture {
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
static SUPERTRAITS: Fixture = Fixture {
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
static ASSOCIATED_TYPES: Fixture = Fixture {
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
static MULTIPLE_TRAIT_IMPLS: Fixture = Fixture {
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
static GENERIC_TRAIT: Fixture = Fixture {
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

// =============================================================================
// Other Fixtures
// =============================================================================

/// Constants and statics
static CONSTANTS: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "test_crate::MAX_SIZE",
        },
        ExpectedEntity {
            kind: EntityKind::Constant,
            qualified_name: "test_crate::GLOBAL_VALUE",
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
static MACRO_RULES: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Macro,
            qualified_name: "test_crate::my_macro",
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

// =============================================================================
// Hard but Feasible Resolution Fixtures
// =============================================================================

/// Multi-hop re-exports: following chains of pub use
static MULTI_HOP_REEXPORTS: Fixture = Fixture {
    name: "multi_hop_reexports",
    files: &[
        (
            "lib.rs",
            r#"
mod internal;
pub use internal::actual_function;

fn caller() {
    actual_function();
}
"#,
        ),
        (
            "internal/mod.rs",
            r#"
mod deep;
pub use deep::actual_function;
"#,
        ),
        (
            "internal/deep.rs",
            r#"
pub fn actual_function() {}
"#,
        ),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::internal",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::internal::deep",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::internal::deep::actual_function",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::caller",
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
            from: "test_crate",
            to: "test_crate::caller",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::internal",
            to: "test_crate::internal::deep",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::internal::deep",
            to: "test_crate::internal::deep::actual_function",
        },
        // The key test: does CALLS resolve through the re-export chain?
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::caller",
            to: "test_crate::internal::deep::actual_function",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Glob re-exports: resolving symbols imported via *
static GLOB_REEXPORTS: Fixture = Fixture {
    name: "glob_reexports",
    files: &[
        (
            "lib.rs",
            r#"
mod helpers;
pub use helpers::*;

fn caller() {
    helper_a();
    helper_b();
}
"#,
        ),
        (
            "helpers.rs",
            r#"
pub fn helper_a() {}
pub fn helper_b() {}
"#,
        ),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::helpers",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::helpers::helper_a",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::helpers::helper_b",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::caller",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::helpers",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::caller",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::helpers",
            to: "test_crate::helpers::helper_a",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::helpers",
            to: "test_crate::helpers::helper_b",
        },
        // Key test: do CALLS resolve through glob imports?
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::caller",
            to: "test_crate::helpers::helper_a",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::caller",
            to: "test_crate::helpers::helper_b",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Trait method vs inherent method priority
static TRAIT_VS_INHERENT_METHOD: Fixture = Fixture {
    name: "trait_vs_inherent_method",
    files: &[(
        "lib.rs",
        r#"
pub trait Formatter {
    fn format(&self) -> String;
}

pub struct Data {
    pub value: i32,
}

impl Data {
    pub fn format(&self) -> String {
        format!("Data: {}", self.value)
    }
}

impl Formatter for Data {
    fn format(&self) -> String {
        format!("Formatted: {}", self.value)
    }
}

pub fn call_inherent(d: &Data) -> String {
    d.format()  // Should call inherent method (priority)
}

pub fn call_trait(d: &Data) -> String {
    Formatter::format(d)  // Explicitly calls trait method
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
            qualified_name: "test_crate::Formatter",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Data",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::call_inherent",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::call_trait",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Formatter",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Data",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::call_inherent",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::call_trait",
        },
        // Key tests: different CALLS targets for same method name
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::call_inherent",
            to: "test_crate::Data::format",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::call_trait",
            to: "test_crate::Formatter::format",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Impl blocks scattered across multiple modules
static SCATTERED_IMPL_BLOCKS: Fixture = Fixture {
    name: "scattered_impl_blocks",
    files: &[
        (
            "lib.rs",
            r#"
pub mod types;
mod widget_display;
mod widget_builder;

use types::Widget;

pub fn caller() {
    let w = Widget::new(1);
    w.display();
}
"#,
        ),
        (
            "types.rs",
            r#"
pub struct Widget {
    pub id: u32,
}
"#,
        ),
        (
            "widget_display.rs",
            r#"
use crate::types::Widget;

impl Widget {
    pub fn display(&self) {
        println!("Widget {}", self.id);
    }
}
"#,
        ),
        (
            "widget_builder.rs",
            r#"
use crate::types::Widget;

impl Widget {
    pub fn new(id: u32) -> Self {
        Widget { id }
    }
}
"#,
        ),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::types",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::widget_display",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::widget_builder",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::types::Widget",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::caller",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::types::Widget::display",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::types::Widget::new",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::types",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::caller",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::types",
            to: "test_crate::types::Widget",
        },
        // Key test: calls resolve to methods defined in different modules
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::caller",
            to: "test_crate::types::Widget::new",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::caller",
            to: "test_crate::types::Widget::display",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Associated types in trait implementations
static ASSOCIATED_TYPES_RESOLUTION: Fixture = Fixture {
    name: "associated_types_resolution",
    files: &[(
        "lib.rs",
        r#"
pub trait Producer {
    type Output;
    fn produce(&self) -> Self::Output;
}

pub struct IntProducer;
pub struct StringProducer;

impl Producer for IntProducer {
    type Output = i32;
    fn produce(&self) -> Self::Output { 42 }
}

impl Producer for StringProducer {
    type Output = String;
    fn produce(&self) -> Self::Output { String::from("hello") }
}

pub fn use_int_producer(p: &IntProducer) -> i32 {
    p.produce()
}

pub fn use_string_producer(p: &StringProducer) -> String {
    p.produce()
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
            qualified_name: "test_crate::Producer",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::IntProducer",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::StringProducer",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::use_int_producer",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::use_string_producer",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Producer",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::IntProducer",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::StringProducer",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::use_int_producer",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::use_string_producer",
        },
        // Key test: calls should resolve to the specific impl's method
        // Methods in trait impls are named with full <Type as Trait>::method syntax
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::use_int_producer",
            to: "<test_crate::IntProducer as test_crate::Producer>::produce",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::use_string_producer",
            to: "<test_crate::StringProducer as test_crate::Producer>::produce",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Prelude shadowing: local definitions shadow std prelude
static PRELUDE_SHADOWING: Fixture = Fixture {
    name: "prelude_shadowing",
    files: &[(
        "lib.rs",
        r#"
// Shadow std::option::Option with our own
pub enum Option<T> {
    Some(T),
    None,
    Unknown,  // Extra variant not in std
}

pub fn create_some() -> Option<i32> {
    Option::Some(42)  // Uses local Option
}

pub fn create_none() -> Option<i32> {
    Option::None  // Uses local Option
}

pub fn create_unknown() -> Option<i32> {
    Option::Unknown  // Only exists in local Option
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "test_crate::Option",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_some",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_none",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_unknown",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Option",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::create_some",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::create_none",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::create_unknown",
        },
        // Key test: USES should point to local Option, not std::option::Option
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "test_crate::create_some",
            to: "test_crate::Option",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "test_crate::create_none",
            to: "test_crate::Option",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Uses,
            from: "test_crate::create_unknown",
            to: "test_crate::Option",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Generic trait bounds affecting method resolution
static GENERIC_BOUNDS_RESOLUTION: Fixture = Fixture {
    name: "generic_bounds_resolution",
    files: &[(
        "lib.rs",
        r#"
pub trait Processor {
    fn process(&self) -> i32;
}

pub trait Validator {
    fn validate(&self) -> bool;
}

pub struct Data {
    pub value: i32,
}

impl Processor for Data {
    fn process(&self) -> i32 { self.value * 2 }
}

impl Validator for Data {
    fn validate(&self) -> bool { self.value > 0 }
}

pub fn process_item<T: Processor>(item: &T) -> i32 {
    item.process()  // Calls Processor::process
}

pub fn validate_item<T: Validator>(item: &T) -> bool {
    item.validate()  // Calls Validator::validate
}

pub fn process_and_validate<T: Processor + Validator>(item: &T) -> (i32, bool) {
    (item.process(), item.validate())  // Both traits in scope
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
            qualified_name: "test_crate::Processor",
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Validator",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Data",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::process_item",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::validate_item",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::process_and_validate",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Processor",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Validator",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Data",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::process_item",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::validate_item",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::process_and_validate",
        },
        // Key test: CALLS should point to trait methods (not concrete impls, since T is generic)
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::process_item",
            to: "test_crate::Processor::process",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::validate_item",
            to: "test_crate::Validator::validate",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::process_and_validate",
            to: "test_crate::Processor::process",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::process_and_validate",
            to: "test_crate::Validator::validate",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Type alias chains
static TYPE_ALIAS_CHAINS: Fixture = Fixture {
    name: "type_alias_chains",
    files: &[(
        "lib.rs",
        r#"
pub struct RawConfig {
    pub value: i32,
}

pub type Config = RawConfig;
pub type AppConfig = Config;
pub type Settings = AppConfig;

impl Settings {
    pub fn new() -> Self {
        RawConfig { value: 0 }
    }

    pub fn with_value(value: i32) -> Self {
        RawConfig { value }
    }
}

pub fn create_settings() -> Settings {
    Settings::new()
}

pub fn create_with_value() -> AppConfig {
    AppConfig::with_value(42)
}
"#,
    )],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::RawConfig",
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "test_crate::Config",
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "test_crate::AppConfig",
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "test_crate::Settings",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_settings",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_with_value",
        },
        // Methods are on RawConfig, accessed through aliases
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::RawConfig::new",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::RawConfig::with_value",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::RawConfig",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Config",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::AppConfig",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Settings",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::create_settings",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::create_with_value",
        },
        // Key test: calls through type aliases resolve to the underlying type's methods
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::create_settings",
            to: "test_crate::RawConfig::new",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::create_with_value",
            to: "test_crate::RawConfig::with_value",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Nested use declarations with renaming
static NESTED_USE_RENAMING: Fixture = Fixture {
    name: "nested_use_renaming",
    files: &[
        (
            "lib.rs",
            r#"
pub mod network;

use network::{
    http::{get as http_get, post as http_post},
    tcp::connect as tcp_connect,
};

pub fn make_requests() {
    http_get();
    http_post();
    tcp_connect();
}
"#,
        ),
        (
            "network/mod.rs",
            r#"
pub mod http;
pub mod tcp;
"#,
        ),
        (
            "network/http.rs",
            r#"
pub fn get() {}
pub fn post() {}
"#,
        ),
        (
            "network/tcp.rs",
            r#"
pub fn connect() {}
"#,
        ),
    ],
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::network",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::network::http",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::network::tcp",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::network::http::get",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::network::http::post",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::network::tcp::connect",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::make_requests",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::network",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::make_requests",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::network",
            to: "test_crate::network::http",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::network",
            to: "test_crate::network::tcp",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::network::http",
            to: "test_crate::network::http::get",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::network::http",
            to: "test_crate::network::http::post",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::network::tcp",
            to: "test_crate::network::tcp::connect",
        },
        // Key test: CALLS resolve through renamed imports to original functions
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::make_requests",
            to: "test_crate::network::http::get",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::make_requests",
            to: "test_crate::network::http::post",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::make_requests",
            to: "test_crate::network::tcp::connect",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Extension traits: adding methods to foreign types
static EXTENSION_TRAITS: Fixture = Fixture {
    name: "extension_traits",
    files: &[(
        "lib.rs",
        r#"
pub trait StringExt {
    fn is_blank(&self) -> bool;
    fn word_count(&self) -> usize;
}

impl StringExt for String {
    fn is_blank(&self) -> bool {
        self.trim().is_empty()
    }
    fn word_count(&self) -> usize {
        self.split_whitespace().count()
    }
}

impl StringExt for str {
    fn is_blank(&self) -> bool {
        self.trim().is_empty()
    }
    fn word_count(&self) -> usize {
        self.split_whitespace().count()
    }
}

pub fn check_string(s: String) -> bool {
    s.is_blank()  // Calls StringExt for String
}

pub fn check_str(s: &str) -> bool {
    s.is_blank()  // Calls StringExt for str
}

pub fn count_words_string(s: String) -> usize {
    s.word_count()  // Calls StringExt for String
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
            qualified_name: "test_crate::StringExt",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::check_string",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::check_str",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::count_words_string",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::StringExt",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::check_string",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::check_str",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::count_words_string",
        },
        // Key test: CALLS should distinguish between impls for String vs str
        // Methods are named with full <Type as Trait>::method syntax
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::check_string",
            to: "<test_crate::String as test_crate::StringExt>::is_blank",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::check_str",
            to: "<test_crate::str as test_crate::StringExt>::is_blank",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::count_words_string",
            to: "<test_crate::String as test_crate::StringExt>::word_count",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

// =============================================================================
// Workspace Fixtures
// =============================================================================

/// Workspace with multiple crates and cross-crate dependencies
static WORKSPACE_BASIC: Fixture = Fixture {
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
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "my_core::CoreType",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "my_core::core_function",
        },
        // Utils crate entities
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "my_utils",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "my_utils::process_core",
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
        // Cross-crate import
        ExpectedRelationship {
            kind: RelationshipKind::Imports,
            from: "my_utils::process_core",
            to: "my_core::CoreType",
        },
    ],
    project_type: ProjectType::Workspace,
    cargo_toml: Some(
        r#"[workspace]
members = ["crates/*"]
resolver = "2"
"#,
    ),
};

// =============================================================================
// Test Functions - Basic
// =============================================================================

#[tokio::test]
#[ignore] // Requires Docker
async fn test_spec_validation_basic_mod() -> Result<()> {
    run_spec_validation(&BASIC_MOD).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_visibility() -> Result<()> {
    run_spec_validation(&VISIBILITY).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_use_imports() -> Result<()> {
    run_spec_validation(&USE_IMPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_reexports() -> Result<()> {
    run_spec_validation(&REEXPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_free_functions() -> Result<()> {
    run_spec_validation(&FREE_FUNCTIONS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_methods() -> Result<()> {
    run_spec_validation(&METHODS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_cross_module_calls() -> Result<()> {
    run_spec_validation(&CROSS_MODULE_CALLS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_structs() -> Result<()> {
    run_spec_validation(&STRUCTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_enums() -> Result<()> {
    run_spec_validation(&ENUMS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_type_aliases() -> Result<()> {
    run_spec_validation(&TYPE_ALIASES).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_trait_def() -> Result<()> {
    run_spec_validation(&TRAIT_DEF).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_trait_impl() -> Result<()> {
    run_spec_validation(&TRAIT_IMPL).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_supertraits() -> Result<()> {
    run_spec_validation(&SUPERTRAITS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_constants() -> Result<()> {
    run_spec_validation(&CONSTANTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_macro_rules() -> Result<()> {
    run_spec_validation(&MACRO_RULES).await
}

// =============================================================================
// Test Functions - Advanced Module System
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_deep_module_nesting() -> Result<()> {
    run_spec_validation(&DEEP_MODULE_NESTING).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_mixed_module_structure() -> Result<()> {
    run_spec_validation(&MIXED_MODULE_STRUCTURE).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_self_super_references() -> Result<()> {
    run_spec_validation(&SELF_SUPER_REFERENCES).await
}

// =============================================================================
// Test Functions - Advanced Functions & Calls
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_multiple_impl_blocks() -> Result<()> {
    run_spec_validation(&MULTIPLE_IMPL_BLOCKS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_async_functions() -> Result<()> {
    run_spec_validation(&ASYNC_FUNCTIONS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_builder_pattern() -> Result<()> {
    run_spec_validation(&BUILDER_PATTERN).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_recursive_calls() -> Result<()> {
    run_spec_validation(&RECURSIVE_CALLS).await
}

// =============================================================================
// Test Functions - Advanced Types
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_tuple_and_unit_structs() -> Result<()> {
    run_spec_validation(&TUPLE_AND_UNIT_STRUCTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_complex_enums() -> Result<()> {
    run_spec_validation(&COMPLEX_ENUMS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_generic_structs() -> Result<()> {
    run_spec_validation(&GENERIC_STRUCTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_lifetimes() -> Result<()> {
    run_spec_validation(&LIFETIMES).await
}

// =============================================================================
// Test Functions - Advanced Traits
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_associated_types() -> Result<()> {
    run_spec_validation(&ASSOCIATED_TYPES).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_multiple_trait_impls() -> Result<()> {
    run_spec_validation(&MULTIPLE_TRAIT_IMPLS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_generic_trait() -> Result<()> {
    run_spec_validation(&GENERIC_TRAIT).await
}

// =============================================================================
// Test Functions - Workspace
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_workspace_basic() -> Result<()> {
    run_spec_validation(&WORKSPACE_BASIC).await
}

// =============================================================================
// Test Functions - Hard but Feasible Resolution
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_spec_validation_multi_hop_reexports() -> Result<()> {
    run_spec_validation(&MULTI_HOP_REEXPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_glob_reexports() -> Result<()> {
    run_spec_validation(&GLOB_REEXPORTS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_trait_vs_inherent_method() -> Result<()> {
    run_spec_validation(&TRAIT_VS_INHERENT_METHOD).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_scattered_impl_blocks() -> Result<()> {
    run_spec_validation(&SCATTERED_IMPL_BLOCKS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_associated_types_resolution() -> Result<()> {
    run_spec_validation(&ASSOCIATED_TYPES_RESOLUTION).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_prelude_shadowing() -> Result<()> {
    run_spec_validation(&PRELUDE_SHADOWING).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_generic_bounds_resolution() -> Result<()> {
    run_spec_validation(&GENERIC_BOUNDS_RESOLUTION).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_type_alias_chains() -> Result<()> {
    run_spec_validation(&TYPE_ALIAS_CHAINS).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_nested_use_renaming() -> Result<()> {
    run_spec_validation(&NESTED_USE_RENAMING).await
}

#[tokio::test]
#[ignore]
async fn test_spec_validation_extension_traits() -> Result<()> {
    run_spec_validation(&EXTENSION_TRAITS).await
}
