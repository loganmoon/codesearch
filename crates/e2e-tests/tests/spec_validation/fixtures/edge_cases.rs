//! edge_cases fixtures for spec validation tests

use super::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
};

/// Tests that `<Type as Trait>::method()` syntax creates proper impl block structure
/// Note: CALLS relationships for UFCS are not yet resolved - this test validates entity extraction
pub static UFCS_EXPLICIT: Fixture = Fixture {
    name: "ufcs_explicit",
    files: &[(
        "lib.rs",
        r#"
pub trait Processor {
    fn process(&self) -> i32;
}

pub struct Data {
    value: i32,
}

impl Processor for Data {
    fn process(&self) -> i32 {
        self.value * 2
    }
}

pub fn use_ufcs(data: &Data) -> i32 {
    // Explicit UFCS call syntax
    <Data as Processor>::process(data)
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
            kind: EntityKind::Struct,
            qualified_name: "test_crate::Data",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::use_ufcs",
        },
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::Data as test_crate::Processor>",
        },
        ExpectedEntity {
            kind: EntityKind::Method,
            qualified_name: "<test_crate::Data as test_crate::Processor>::process",
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
            to: "test_crate::Data",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::use_ufcs",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::<test_crate::Data as test_crate::Processor>",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "test_crate::<test_crate::Data as test_crate::Processor>",
            to: "test_crate::Processor",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Const generics: types parameterized by constant values
pub static CONST_GENERICS: Fixture = Fixture {
    name: "const_generics",
    files: &[(
        "lib.rs",
        r#"
pub struct FixedArray<const N: usize> {
    data: [i32; N],
}

impl<const N: usize> FixedArray<N> {
    pub fn new() -> Self {
        Self { data: [0; N] }
    }

    pub fn len(&self) -> usize {
        N
    }
}

pub fn create_small() -> FixedArray<10> {
    FixedArray::new()
}

pub fn create_large() -> FixedArray<1000> {
    FixedArray::new()
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
            qualified_name: "test_crate::FixedArray",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_small",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::create_large",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::FixedArray",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::create_small",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::create_large",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Blanket impl declarations: impl<T> Trait for T where T: OtherTrait
/// Tests that blanket impls and concrete impls create proper impl block structures
pub static BLANKET_IMPL: Fixture = Fixture {
    name: "blanket_impl",
    files: &[(
        "lib.rs",
        r#"
pub trait Printable {
    fn to_string(&self) -> String;
}

pub trait Debug {
    fn debug(&self) -> String;
}

// Blanket impl: any Debug is also Printable
impl<T: Debug> Printable for T {
    fn to_string(&self) -> String {
        self.debug()
    }
}

pub struct MyType {
    value: i32,
}

impl Debug for MyType {
    fn debug(&self) -> String {
        format!("MyType({})", self.value)
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
            qualified_name: "test_crate::Printable",
        },
        ExpectedEntity {
            kind: EntityKind::Trait,
            qualified_name: "test_crate::Debug",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::MyType",
        },
        // Blanket impl creates an impl block with generic parameter in name
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name:
                "test_crate::<test_crate::T as test_crate::Printable where T: test_crate::Debug>",
        },
        // Concrete impl for MyType
        ExpectedEntity {
            kind: EntityKind::ImplBlock,
            qualified_name: "test_crate::<test_crate::MyType as test_crate::Debug>",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Printable",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Debug",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::MyType",
        },
        // Blanket impl implements Printable
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "test_crate::<test_crate::T as test_crate::Printable where T: test_crate::Debug>",
            to: "test_crate::Printable",
        },
        // MyType impl block implements Debug
        ExpectedRelationship {
            kind: RelationshipKind::Implements,
            from: "test_crate::<test_crate::MyType as test_crate::Debug>",
            to: "test_crate::Debug",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Pattern matching on enum variants in function bodies
pub static PATTERN_MATCHING: Fixture = Fixture {
    name: "pattern_matching",
    files: &[(
        "lib.rs",
        r#"
pub enum Message {
    Text(String),
    Number(i32),
    Quit,
}

pub fn process_message(msg: Message) -> String {
    match msg {
        Message::Text(s) => format!("Text: {}", s),
        Message::Number(n) => format!("Number: {}", n),
        Message::Quit => "Quit".to_string(),
    }
}

pub fn is_quit(msg: &Message) -> bool {
    matches!(msg, Message::Quit)
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
            qualified_name: "test_crate::Message",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::process_message",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::is_quit",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::Message",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::process_message",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::is_quit",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Tests #[path = "..."] attribute for custom module paths.
/// Note: Current behavior derives qualified names from physical file paths,
/// not from the logical module path declared by #[path]. This documents the
/// actual system behavior.
pub static CUSTOM_MODULE_PATHS: Fixture = Fixture {
    name: "custom_module_paths",
    files: &[
        (
            "lib.rs",
            r#"
#[path = "impl/special.rs"]
mod special;

pub use special::SpecialType;

pub fn use_special() -> SpecialType {
    special::create_special()
}
"#,
        ),
        (
            "impl/special.rs",
            r#"
pub struct SpecialType {
    value: i32,
}

pub fn create_special() -> SpecialType {
    SpecialType { value: 42 }
}
"#,
        ),
    ],
    // Note: Entities in impl/special.rs get qualified names based on physical file path
    // (test_crate::impl::special::*) rather than the logical module name (test_crate::special::*)
    entities: &[
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate",
        },
        ExpectedEntity {
            kind: EntityKind::Module,
            qualified_name: "test_crate::special",
        },
        ExpectedEntity {
            kind: EntityKind::Struct,
            qualified_name: "test_crate::impl::special::SpecialType",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::impl::special::create_special",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::use_special",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::special",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::use_special",
        },
        // Calls relationship resolves to physical path
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::use_special",
            to: "test_crate::impl::special::create_special",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};

/// Closures as syntactic entities in function calls
/// Tests that functions accepting closures and closure usage are properly modeled
pub static CLOSURES: Fixture = Fixture {
    name: "closures",
    files: &[(
        "lib.rs",
        r#"
pub fn apply<F: Fn(i32) -> i32>(f: F) -> i32 {
    f(5)
}

pub fn caller() -> i32 {
    apply(|x| x + 1)
}

pub fn with_captured() -> i32 {
    let base = 10;
    apply(|x| x + base)
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
            qualified_name: "test_crate::apply",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::caller",
        },
        ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test_crate::with_captured",
        },
    ],
    relationships: &[
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::apply",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::caller",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test_crate",
            to: "test_crate::with_captured",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::caller",
            to: "test_crate::apply",
        },
        ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test_crate::with_captured",
            to: "test_crate::apply",
        },
    ],
    project_type: ProjectType::SingleCrate,
    cargo_toml: None,
};
