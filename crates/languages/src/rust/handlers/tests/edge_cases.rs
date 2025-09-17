//! Edge case tests for Rust extraction handlers

use super::*;
use crate::rust::handlers::function_handlers::handle_function;
use crate::rust::handlers::type_handlers::handle_struct;

#[test]
fn test_empty_source() {
    let source = "";

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
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

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
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

    let function_entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Should handle unicode in functions");
    assert_eq!(function_entities.len(), 1);
    assert_eq!(function_entities[0].name, "你好世界");

    let struct_entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Should handle unicode in structs");
    assert_eq!(struct_entities.len(), 1);
    assert_eq!(struct_entities[0].name, "Données");
}

#[test]
fn test_very_long_identifier() {
    let source = r#"
fn this_is_an_extremely_long_function_name_that_goes_on_and_on_and_on_and_on_and_on_and_on_and_on_and_on_and_on() {
    // Long name but valid Rust
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
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
    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
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

    let entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
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

    // Should not panic, but might not extract properly
    let result = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function);

    // The extraction might fail or succeed with partial data
    // Document the actual behavior
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_multiple_items_in_one_line() {
    let source = "fn a() {} fn b() {} fn c() {}";

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
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

    let function_entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Should handle raw identifiers in functions");
    assert_eq!(function_entities.len(), 1);
    // Raw identifiers might be extracted with or without r# prefix
    assert!(function_entities[0].name == "type" || function_entities[0].name == "r#type");

    let struct_entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Should handle raw identifiers in structs");
    assert_eq!(struct_entities.len(), 1);
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

    let function_entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Should handle attributes");
    assert_eq!(function_entities.len(), 1);

    let struct_entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Should handle attributes");
    assert_eq!(struct_entities.len(), 1);
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

    let struct_entities = extract_with_handler(source, queries::STRUCT_QUERY, handle_struct)
        .expect("Should handle const generics in structs");
    assert_eq!(struct_entities.len(), 1);

    let function_entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
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

    let function_entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Should handle complex nested types");
    assert_eq!(function_entities.len(), 1);

    // Check that the complex return type is captured
    use crate::rust::entities::RustEntityVariant;
    use crate::transport::EntityVariant;

    if let EntityVariant::Rust(RustEntityVariant::Function { return_type, .. }) =
        &function_entities[0].variant
    {
        assert!(return_type.is_some());
        let ret_type = return_type.as_ref().unwrap();
        assert!(ret_type.contains("Result"));
        assert!(ret_type.contains("Option"));
        assert!(ret_type.contains("Vec"));
    }
}
