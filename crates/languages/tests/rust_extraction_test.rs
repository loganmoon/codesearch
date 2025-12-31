//! Test for verifying Rust extractor works correctly

use codesearch_languages::{create_extractor, Extractor};
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
