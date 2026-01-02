//! Edge case tests for Rust extraction handlers

use super::*;
use crate::rust::handler_impls::function_handlers::handle_function_impl;
use crate::rust::handler_impls::type_handlers::handle_struct_impl;

#[test]
fn test_empty_source() {
    let source = "";

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Should handle empty source");
    assert_eq!(entities.len(), 0);
}

#[test]
fn test_only_comments() {
    let source = r#"
// This is a comment
// Another comment
/* Block comment */
/// Doc comment without code
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Should handle comment-only source");
    assert_eq!(entities.len(), 0);
}

#[test]
fn test_unicode_identifiers() {
    let source = r#"
fn 你好世界() {
    println!("Hello world in Chinese");
}

struct Données {
    名前: String,
}
"#;

    let function_entities =
        extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
            .expect("Should handle unicode in functions");
    assert_eq!(function_entities.len(), 1);
    assert_eq!(function_entities[0].name, "你好世界");

    let struct_entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Should handle unicode in structs");
    // Struct + 1 field
    assert_eq!(struct_entities.len(), 2);
    assert_eq!(struct_entities[0].name, "Données");
}

#[test]
fn test_very_long_identifier() {
    let source = r#"
fn this_is_an_extremely_long_function_name_that_goes_on_and_on_and_on_and_on_and_on_and_on_and_on_and_on_and_on() {
    // Long name but valid Rust
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Should handle long identifiers");
    assert_eq!(entities.len(), 1);
    assert!(entities[0].name.len() > 100);
}

#[test]
fn test_nested_functions() {
    let source = r#"
fn outer() {
    fn inner() {
        fn deeply_nested() {
            println!("Nested!");
        }
    }
}
"#;

    // Note: Tree-sitter queries typically don't match nested functions
    // This documents the current behavior
    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Should handle nested functions");

    // Depending on the query, this might only match the outer function
    // or all functions. Document actual behavior:
    assert!(!entities.is_empty());
    assert_eq!(entities[0].name, "outer");
}

#[test]
fn test_macro_generated_code() {
    let source = r#"
macro_rules! generate_struct {
    ($name:ident) => {
        struct $name {
            value: i32,
        }
    };
}

generate_struct!(Generated);

// The macro invocation won't be expanded by tree-sitter
// So Generated struct won't be found
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Should handle macro code");

    // Macro-generated code is not visible to tree-sitter
    assert_eq!(entities.len(), 0);
}

#[test]
fn test_incomplete_syntax() {
    // Missing closing brace
    let source = r#"
fn incomplete() {
    println!("Missing closing brace");
"#;

    // tree-sitter is error-tolerant but incomplete functions may not match queries
    let result = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl);

    // Should not panic - gracefully handles malformed code
    assert!(result.is_ok(), "Should not panic on incomplete syntax");

    // Document actual behavior: incomplete functions may or may not be extracted
    // depending on how tree-sitter error recovery works
    let entities = result.unwrap();
    // Don't assert on entity count - behavior depends on tree-sitter's error recovery
    // The important thing is we didn't panic
    if !entities.is_empty() {
        // If extracted, should have the correct name
        assert_eq!(entities[0].name, "incomplete");
    }
}

#[test]
fn test_multiple_items_in_one_line() {
    let source = "fn a() {} fn b() {} fn c() {}";

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Should handle multiple items on one line");

    assert_eq!(entities.len(), 3);
    assert_eq!(entities[0].name, "a");
    assert_eq!(entities[1].name, "b");
    assert_eq!(entities[2].name, "c");
}

#[test]
fn test_raw_identifiers() {
    let source = r#"
fn r#type() {
    let r#match = 42;
}

struct r#struct {
    r#fn: String,
}
"#;

    let function_entities =
        extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
            .expect("Should handle raw identifiers in functions");
    assert_eq!(function_entities.len(), 1);
    // Raw identifiers might be extracted with or without r# prefix
    assert!(function_entities[0].name == "type" || function_entities[0].name == "r#type");

    let struct_entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Should handle raw identifiers in structs");
    // Struct + 1 field
    assert_eq!(struct_entities.len(), 2);
}

#[test]
fn test_attributes_and_macros() {
    let source = r#"
#[test]
#[should_panic(expected = "error")]
#[cfg(test)]
fn test_function() {
    panic!("error");
}

#[repr(C)]
#[derive(Debug)]
struct FFIStruct {
    value: i32,
}
"#;

    let function_entities =
        extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
            .expect("Should handle attributes");
    assert_eq!(function_entities.len(), 1);

    let struct_entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Should handle attributes");
    // Struct + 1 field
    assert_eq!(struct_entities.len(), 2);
}

#[test]
fn test_const_generics() {
    let source = r#"
struct Array<T, const N: usize> {
    data: [T; N],
}

fn fixed_array<const SIZE: usize>() -> [u8; SIZE] {
    [0; SIZE]
}
"#;

    let struct_entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Should handle const generics in structs");
    // Struct + 1 field
    assert_eq!(struct_entities.len(), 2);

    let function_entities =
        extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
            .expect("Should handle const generics in functions");
    assert_eq!(function_entities.len(), 1);
}

#[test]
fn test_extremely_nested_types() {
    let source = r#"
fn complex_return() -> Result<Option<Vec<HashMap<String, Box<dyn Fn() -> Result<(), Error>>>>>, Error> {
    Ok(None)
}

struct Nested {
    field: Arc<Mutex<RefCell<Option<Box<Vec<String>>>>>>,
}
"#;

    let function_entities =
        extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
            .expect("Should handle complex nested types");
    assert_eq!(function_entities.len(), 1);

    // Check that the complex return type is captured
    let entity = &function_entities[0];
    if let Some(sig) = &entity.signature {
        assert!(sig.return_type.is_some());
        let ret_type = sig.return_type.as_ref().unwrap();
        assert!(ret_type.contains("Result"));
        assert!(ret_type.contains("Option"));
        assert!(ret_type.contains("Vec"));
    }
}

#[test]
fn test_multibyte_utf8_in_struct_fields() {
    use crate::rust::handler_impls::type_handlers::handle_struct_impl;

    // Test UTF-8 safety fixes in type_handlers.rs
    let source = r#"
struct User {
    名前: String,
    年齢: u32,
    pub メール: String,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Should not panic with multi-byte UTF-8 in struct fields");

    // Struct + 3 fields
    assert_eq!(entities.len(), 4);
    assert_eq!(entities[0].name, "User");

    // Verify fields were extracted as separate entities
    let field_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == codesearch_core::EntityType::Property)
        .collect();
    assert_eq!(field_entities.len(), 3);
}

#[test]
fn test_multibyte_utf8_in_enum_variants() {
    use crate::rust::handler_impls::type_handlers::handle_enum_impl;

    // Test UTF-8 safety in enum variant parsing
    let source = r#"
enum 状態 {
    成功(String),
    失敗 = 1,
    待機中,
}
"#;

    let entities = extract_with_handler(source, queries::ENUM_QUERY, handle_enum_impl)
        .expect("Should not panic with multi-byte UTF-8 in enum variants");

    // Enum + 3 variants
    assert_eq!(entities.len(), 4);
    assert_eq!(entities[0].name, "状態");

    // Verify variants were extracted as separate entities
    let variant_entities: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == codesearch_core::EntityType::EnumVariant)
        .collect();
    assert_eq!(variant_entities.len(), 3);
}

#[test]
fn test_multibyte_utf8_in_function_parameters() {
    use crate::rust::handler_impls::function_handlers::handle_function_impl;

    // Test UTF-8 safety in parameter extraction
    let source = r#"
fn プロセス(名前: String, 年齢: u32) -> String {
    format!("{名前} is {年齢} years old")
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function_impl)
        .expect("Should not panic with multi-byte UTF-8 in parameters");

    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].name, "プロセス");

    // Verify parameters were extracted
    if let Some(sig) = &entities[0].signature {
        assert_eq!(sig.parameters.len(), 2);
    }
}

#[test]
fn test_derive_with_multibyte_utf8() {
    use crate::rust::handler_impls::type_handlers::handle_struct_impl;

    // Test UTF-8 safety in derive attribute parsing
    let source = r#"
#[derive(Debug, Clone)]
struct データ {
    値: i32,
}
"#;

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct_impl)
        .expect("Should not panic with multi-byte UTF-8 in derives");

    // Struct + 1 field
    assert_eq!(entities.len(), 2);
    // Verify derives were extracted
    assert!(!entities[0].metadata.decorators.is_empty());
}
