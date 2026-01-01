//! cross_module fixtures for spec validation tests
//!
//! Validates complex cross-module scenarios:
//! - Multi-hop re-exports (pub use chains)
//! - Glob re-exports (pub use path::*)
//! - Trait method vs inherent method priority
//! - Scattered impl blocks across modules
//! - Associated types resolution
//! - Prelude shadowing
//! - Generic bounds resolution
//! - Type alias chains
//! - Nested use declarations with renaming
//! - Extension traits on foreign types

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
    Visibility,
};

/// Multi-hop re-exports: resolving symbols through re-export chains
///
/// Validates:
/// - R-CALLS-FUNCTION: calls through re-export chains resolve to original definition
pub static MULTI_HOP_REEXPORTS: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::internal",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::internal::deep",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::internal::deep::actual_function",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::caller",
            visibility: Some(Visibility::Private),
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
///
/// Validates:
/// - R-CALLS-FUNCTION: calls through glob imports resolve to original definition
pub static GLOB_REEXPORTS: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::helpers",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::helpers::helper_a",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::helpers::helper_b",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::caller",
            visibility: Some(Visibility::Private),
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
///
/// Validates:
/// - R-CALLS-FUNCTION: inherent methods take priority over trait methods for method syntax
/// - R-CALLS-FUNCTION: explicit Trait::method() syntax calls trait method
/// - Q-INHERENT-METHOD: inherent methods use <Type>::method format
pub static TRAIT_VS_INHERENT_METHOD: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Formatter",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Data",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::call_inherent",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::call_trait",
            visibility: Some(Visibility::Public),
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
        // Q-INHERENT-METHOD: inherent method uses UFCS format
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::call_inherent",
            to: "<test_crate::Data>::format",
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
///
/// Validates:
/// - Methods from scattered impl blocks belong to same logical type
/// - Q-INHERENT-METHOD: methods use UFCS format regardless of definition location
/// - R-CALLS-FUNCTION: calls resolve to correct method definition
pub static SCATTERED_IMPL_BLOCKS: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::types",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::widget_display",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::widget_builder",
            visibility: Some(Visibility::Private),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::types::Widget",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::caller",
            visibility: Some(Visibility::Public),
        },
        // Q-INHERENT-METHOD: methods use UFCS format
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::types::Widget>::display",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::types::Widget>::new",
            visibility: Some(Visibility::Public),
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
            to: "<test_crate::types::Widget>::new",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::caller",
            to: "<test_crate::types::Widget>::display",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Associated types in trait implementations
///
/// Validates:
/// - Calls to trait methods resolve to specific impl when type is known
/// - Q-TRAIT-IMPL-METHOD: uses "<{type_fqn} as {trait_fqn}>::{name}" format
pub static ASSOCIATED_TYPES_RESOLUTION: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Producer",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::IntProducer",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::StringProducer",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::use_int_producer",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::use_string_producer",
            visibility: Some(Visibility::Public),
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
///
/// Validates:
/// - Local type definitions shadow prelude types
/// - R-USES-TYPE: uses of shadowed types point to local definition
pub static PRELUDE_SHADOWING: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Enum,
            qualified_name: "test_crate::Option",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_some",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_none",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_unknown",
            visibility: Some(Visibility::Public),
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
///
/// Validates:
/// - Calls on generic types with trait bounds resolve to trait method definitions
/// - Not to specific impl methods (since concrete type is unknown at call site)
pub static GENERIC_BOUNDS_RESOLUTION: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Processor",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Validator",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Data",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::process_item",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::validate_item",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::process_and_validate",
            visibility: Some(Visibility::Public),
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
///
/// Validates:
/// - E-TYPE-ALIAS: type aliases produce TypeAlias entities
/// - Calls through type aliases resolve to the underlying type's methods
/// - Q-INHERENT-METHOD: methods use UFCS format on underlying type
pub static TYPE_ALIAS_CHAINS: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::RawConfig",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "test_crate::Config",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "test_crate::AppConfig",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::TypeAlias,
            qualified_name: "test_crate::Settings",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_settings",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_with_value",
            visibility: Some(Visibility::Public),
        },
        // Methods are on RawConfig, accessed through aliases
        // Q-INHERENT-METHOD: use UFCS format
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::RawConfig>::new",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::RawConfig>::with_value",
            visibility: Some(Visibility::Public),
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
            to: "<test_crate::RawConfig>::new",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::create_with_value",
            to: "<test_crate::RawConfig>::with_value",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Nested use declarations with renaming
///
/// Validates:
/// - R-CALLS-FUNCTION: calls through renamed imports resolve to original definitions
/// - R-IMPORTS: use declarations create Imports relationships
pub static NESTED_USE_RENAMING: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::network",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::network::http",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::network::tcp",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::network::http::get",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::network::http::post",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::network::tcp::connect",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::make_requests",
            visibility: Some(Visibility::Public),
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
///
/// Validates:
/// - E-IMPL-TRAIT: trait impls on foreign types produce ImplBlock entities
/// - Calls to extension trait methods resolve correctly based on receiver type
/// - Q-TRAIT-IMPL-METHOD: uses "<{type_fqn} as {trait_fqn}>::{name}" format
/// - Note: String and str are foreign types (from std), not test_crate types
pub static EXTENSION_TRAITS: Fixture = Fixture {
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
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::StringExt",
            visibility: Some(Visibility::Public),
        },
        // Impl blocks for foreign types
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<String as test_crate::StringExt>",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<str as test_crate::StringExt>",
            visibility: None,
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::check_string",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::check_str",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::count_words_string",
            visibility: Some(Visibility::Public),
        },
        // Trait impl methods - String is a foreign type, not test_crate::String
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<String as test_crate::StringExt>::is_blank",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<String as test_crate::StringExt>::word_count",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<str as test_crate::StringExt>::is_blank",
            visibility: Some(Visibility::Public),
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<str as test_crate::StringExt>::word_count",
            visibility: Some(Visibility::Public),
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
            to: "test_crate::<String as test_crate::StringExt>",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::<str as test_crate::StringExt>",
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
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "test_crate::<String as test_crate::StringExt>",
            to: "test_crate::StringExt",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "test_crate::<str as test_crate::StringExt>",
            to: "test_crate::StringExt",
        },
        // Key test: CALLS distinguish between impls for String vs str
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::check_string",
            to: "<String as test_crate::StringExt>::is_blank",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::check_str",
            to: "<str as test_crate::StringExt>::is_blank",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::count_words_string",
            to: "<String as test_crate::StringExt>::word_count",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};
