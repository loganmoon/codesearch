//! functions fixtures for spec validation tests

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Free functions with calls between them
pub static FREE_FUNCTIONS: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::caller",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::callee",
            visibility: Some(Visibility::Public),
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
pub static METHODS: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Foo",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::impl test_crate::Foo",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Foo::method",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::Foo::associated",
            visibility: Some(Visibility::Public),
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
pub static CROSS_MODULE_CALLS: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::utils",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::main_caller",
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
pub static MULTIPLE_IMPL_BLOCKS: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Counter",
            visibility: Some(Visibility::Public),
        },
        // Both impl blocks merge into a single ImplBlock entity
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::impl test_crate::Counter",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Counter::new",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Counter::with_value",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Counter::increment",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::Counter::get",
            visibility: Some(Visibility::Public),
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Counter",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::impl test_crate::Counter",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::impl test_crate::Counter",
            to: "test_crate::Counter::new",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::impl test_crate::Counter",
            to: "test_crate::Counter::with_value",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::impl test_crate::Counter",
            to: "test_crate::Counter::increment",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate::impl test_crate::Counter",
            to: "test_crate::Counter::get",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Async functions
pub static ASYNC_FUNCTIONS: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::async_caller",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::async_callee",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::AsyncService",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::AsyncService::process",
            visibility: Some(Visibility::Public),
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
pub static BUILDER_PATTERN: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::ConfigBuilder",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Config",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::ConfigBuilder::new",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::ConfigBuilder::name",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::ConfigBuilder::value",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "test_crate::ConfigBuilder::build",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_config",
            visibility: Some(Visibility::Public),
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
pub static RECURSIVE_CALLS: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::factorial",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::mutually_recursive_a",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::mutually_recursive_b",
            visibility: Some(Visibility::Public),
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
