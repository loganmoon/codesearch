//! Test for verifying Rust extractor works correctly

use codesearch_core::entities::Visibility;
use codesearch_languages::create_extractor;
use std::path::Path;

#[test]
fn test_rust_extractor_creates_and_extracts() {
    let extractor = create_extractor(
        Path::new("/tmp/test.rs"),
        "test-repo",
        None,
        None,
        Path::new("/tmp"),
    )
    .expect("Should not error")
    .expect("Should have Rust extractor");

    let source = r#"
fn test_function() -> i32 {
    42
}

pub struct TestStruct {
    field: i32,
}
"#;

    let entities = extractor
        .extract(source, Path::new("/tmp/test.rs"))
        .expect("Should extract entities");

    println!("Extracted {} entities:", entities.len());
    for e in &entities {
        println!("  - {} ({:?})", e.qualified_name, e.entity_type);
    }

    // Should extract at least the function and struct
    assert!(
        entities.len() >= 2,
        "Expected at least 2 entities (function + struct), got {}",
        entities.len()
    );
}

#[test]
fn test_rust_extractor_macro_visibility() {
    let extractor = create_extractor(
        Path::new("/tmp/lib.rs"),
        "test-repo",
        Some("test_crate"),
        Some(Path::new("/tmp")),
        Path::new("/tmp"),
    )
    .expect("Should not error")
    .expect("Should have Rust extractor");

    // This is the exact same content as the e2e fixture
    let source = r#"
#[macro_export]
macro_rules! my_macro {
    () => {};
    ($x:expr) => { $x };
}

macro_rules! private_macro {
    () => {};
}
"#;

    let entities = extractor
        .extract(source, Path::new("/tmp/lib.rs"))
        .expect("Should extract entities");

    println!("Extracted {} entities:", entities.len());
    for e in &entities {
        println!(
            "  - {} ({:?}, vis={:?})",
            e.qualified_name, e.entity_type, e.visibility
        );
    }

    // Find the macros
    let my_macro = entities.iter().find(|e| e.name == "my_macro");
    let private_macro = entities.iter().find(|e| e.name == "private_macro");

    assert!(my_macro.is_some(), "Should find my_macro");
    assert!(private_macro.is_some(), "Should find private_macro");

    let my_macro = my_macro.unwrap();
    let private_macro = private_macro.unwrap();

    // Check visibility
    assert_eq!(
        my_macro.visibility,
        Some(Visibility::Public),
        "my_macro (with #[macro_export]) should be Public, got {:?}",
        my_macro.visibility
    );
    assert_eq!(
        private_macro.visibility,
        Some(Visibility::Private),
        "private_macro (without #[macro_export]) should be Private, got {:?}",
        private_macro.visibility
    );
}
