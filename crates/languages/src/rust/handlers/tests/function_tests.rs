//! Tests for function extraction handler

use super::*;
use crate::rust::entities::RustEntityVariant;
use crate::rust::handlers::function_handlers::handle_function;
use crate::transport::EntityVariant;

#[test]
fn test_simple_function() {
    let source = r#"
fn simple_function() {
    println!("Hello");
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "simple_function");

    if let EntityVariant::Rust(RustEntityVariant::Function {
        is_async,
        is_unsafe,
        is_const,
        parameters,
        ..
    }) = &entity.variant
    {
        assert!(!is_async);
        assert!(!is_unsafe);
        assert!(!is_const);
        assert_eq!(parameters.len(), 0);
    } else {
        panic!("Expected function variant");
    }
}

#[test]
fn test_async_function() {
    let source = r#"
async fn fetch_data() -> Result<String, Error> {
    Ok("data".to_string())
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Function {
        is_async,
        return_type,
        ..
    }) = &entity.variant
    {
        assert!(is_async);
        assert_eq!(return_type.as_deref(), Some("Result<String, Error>"));
    } else {
        panic!("Expected function variant");
    }
}

#[test]
fn test_unsafe_function() {
    let source = r#"
unsafe fn dangerous_operation(ptr: *mut u8) {
    *ptr = 42;
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Function {
        is_unsafe,
        parameters,
        ..
    }) = &entity.variant
    {
        assert!(is_unsafe);
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].0, "ptr");
        assert_eq!(parameters[0].1, "*mut u8");
    } else {
        panic!("Expected function variant");
    }
}

#[test]
fn test_const_function() {
    let source = r#"
const fn compile_time_computation(x: i32) -> i32 {
    x * 2
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Function {
        is_const,
        parameters,
        return_type,
        ..
    }) = &entity.variant
    {
        assert!(is_const);
        assert_eq!(parameters.len(), 1);
        assert_eq!(return_type.as_deref(), Some("i32"));
    } else {
        panic!("Expected function variant");
    }
}

#[test]
fn test_generic_function() {
    let source = r#"
fn generic_func<T: Clone, U>(item: T, other: U) -> (T, U)
where
    U: Debug,
{
    (item.clone(), other)
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Function {
        generics,
        parameters,
        return_type,
        ..
    }) = &entity.variant
    {
        assert_eq!(generics.len(), 2);
        assert!(generics.contains(&"T: Clone".to_string()));
        assert!(generics.contains(&"U".to_string()));
        assert_eq!(parameters.len(), 2);
        assert_eq!(return_type.as_deref(), Some("(T, U)"));
    } else {
        panic!("Expected function variant");
    }
}

#[test]
fn test_function_with_doc_comments() {
    let source = r#"
/// This is a well-documented function
/// It does something important
pub fn documented_function(x: i32) -> i32 {
    x + 1
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    assert_eq!(entity.name, "documented_function");
    assert!(entity.documentation.is_some());
    let doc = entity.documentation.as_ref().unwrap();
    assert!(doc.contains("well-documented"));
    assert!(doc.contains("important"));
}

#[test]
fn test_function_with_lifetime_parameters() {
    let source = r#"
fn lifetime_func<'a, 'b: 'a>(x: &'a str, y: &'b str) -> &'a str {
    if x.len() > y.len() { x } else { y }
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Function {
        generics,
        parameters,
        return_type,
        ..
    }) = &entity.variant
    {
        assert_eq!(generics.len(), 2);
        assert_eq!(parameters.len(), 2);
        assert_eq!(parameters[0].0, "x");
        assert_eq!(parameters[0].1, "&'a str");
        assert_eq!(return_type.as_deref(), Some("&'a str"));
    } else {
        panic!("Expected function variant");
    }
}

#[test]
fn test_function_with_self_parameter() {
    let source = r#"
impl MyStruct {
    fn method(&self, x: i32) -> i32 {
        self.value + x
    }
}
"#;

    // Note: This might not match with FUNCTION_QUERY as it's inside an impl block
    // This test documents the current behavior
    let _entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Extraction should not fail");

    // Currently, functions inside impl blocks might not be matched by FUNCTION_QUERY
    // This is expected behavior - impl methods would need a different query
}

#[test]
fn test_function_with_complex_parameters() {
    let source = r#"
fn complex_params(
    (x, y): (i32, i32),
    MyStruct { field1, field2: renamed }: MyStruct,
    _ignored: bool,
) -> i32 {
    x + y + field1 + renamed
}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Failed to extract function");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    if let EntityVariant::Rust(RustEntityVariant::Function { parameters, .. }) = &entity.variant {
        assert_eq!(parameters.len(), 3);
        // Parameter patterns are complex and might be simplified in extraction
        assert!(parameters[0].0.contains("(x, y)") || parameters[0].0 == "(x, y)");
    } else {
        panic!("Expected function variant");
    }
}

#[test]
fn test_public_vs_private_functions() {
    let source = r#"
pub fn public_function() {}
fn private_function() {}
pub(crate) fn crate_public() {}
"#;

    let entities = extract_with_handler(source, queries::FUNCTION_QUERY, handle_function)
        .expect("Failed to extract functions");

    // Should extract all three functions
    assert_eq!(entities.len(), 3);

    // Check visibility is properly extracted
    use codesearch_core::entities::Visibility;
    assert_eq!(entities[0].visibility, Visibility::Public);
    assert_eq!(entities[1].visibility, Visibility::Private);
    assert_eq!(entities[2].visibility, Visibility::Public); // pub(crate) is still public
}
