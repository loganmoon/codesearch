//! Tests for Python utility functions
//!
//! Tests the shared utility functions used by Python entity extraction,
//! including parameter extraction, docstring parsing, and primitive detection.

use crate::common::import_map::ImportMap;
use crate::python::utils::{
    extract_base_classes, extract_decorators, extract_docstring, extract_function_calls,
    extract_python_parameters, extract_return_type, extract_type_references, filter_self_parameter,
    is_async_function, is_python_primitive,
};
use tree_sitter::Parser;

/// Helper to parse Python source and get the root node
fn parse_python(source: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .expect("Failed to set Python language");
    parser.parse(source, None).expect("Failed to parse source")
}

/// Find a node of a specific kind in the tree
fn find_node<'a>(node: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_node(child, kind) {
            return Some(found);
        }
    }
    None
}

// ============================================================================
// is_python_primitive tests
// ============================================================================

#[test]
fn test_is_python_primitive_basic_types() {
    assert!(is_python_primitive("str"));
    assert!(is_python_primitive("int"));
    assert!(is_python_primitive("float"));
    assert!(is_python_primitive("bool"));
    assert!(is_python_primitive("bytes"));
    assert!(is_python_primitive("None"));
}

#[test]
fn test_is_python_primitive_container_types() {
    assert!(is_python_primitive("list"));
    assert!(is_python_primitive("dict"));
    assert!(is_python_primitive("tuple"));
    assert!(is_python_primitive("set"));
    assert!(is_python_primitive("frozenset"));
}

#[test]
fn test_is_python_primitive_typing_module_types() {
    assert!(is_python_primitive("List"));
    assert!(is_python_primitive("Dict"));
    assert!(is_python_primitive("Tuple"));
    assert!(is_python_primitive("Set"));
    assert!(is_python_primitive("Optional"));
    assert!(is_python_primitive("Union"));
    assert!(is_python_primitive("Callable"));
    assert!(is_python_primitive("Any"));
}

#[test]
fn test_is_python_primitive_async_types() {
    assert!(is_python_primitive("Coroutine"));
    assert!(is_python_primitive("AsyncIterator"));
    assert!(is_python_primitive("AsyncGenerator"));
}

#[test]
fn test_is_python_primitive_non_primitives() {
    assert!(!is_python_primitive("MyClass"));
    assert!(!is_python_primitive("CustomType"));
    assert!(!is_python_primitive("User"));
    assert!(!is_python_primitive("Result"));
    assert!(!is_python_primitive("DataFrame"));
}

// ============================================================================
// extract_python_parameters tests
// ============================================================================

#[test]
fn test_extract_parameters_simple() {
    let source = "def foo(a, b, c): pass";
    let tree = parse_python(source);
    let params_node = find_node(tree.root_node(), "parameters").expect("Should find parameters");

    let params = extract_python_parameters(params_node, source).expect("Should extract parameters");
    assert_eq!(params.len(), 3);
    assert_eq!(params[0].0, "a");
    assert_eq!(params[1].0, "b");
    assert_eq!(params[2].0, "c");
}

#[test]
fn test_extract_parameters_with_types() {
    let source = "def foo(name: str, age: int) -> bool: pass";
    let tree = parse_python(source);
    let params_node = find_node(tree.root_node(), "parameters").expect("Should find parameters");

    let params = extract_python_parameters(params_node, source).expect("Should extract parameters");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].0, "name");
    assert_eq!(params[0].1, Some("str".to_string()));
    assert_eq!(params[1].0, "age");
    assert_eq!(params[1].1, Some("int".to_string()));
}

#[test]
fn test_extract_parameters_with_defaults() {
    let source = "def foo(a, b=10, c='default'): pass";
    let tree = parse_python(source);
    let params_node = find_node(tree.root_node(), "parameters").expect("Should find parameters");

    let params = extract_python_parameters(params_node, source).expect("Should extract parameters");
    assert_eq!(params.len(), 3);
    assert_eq!(params[0].0, "a");
    assert_eq!(params[1].0, "b");
    assert_eq!(params[2].0, "c");
}

#[test]
fn test_extract_parameters_typed_defaults() {
    let source = "def foo(name: str = 'unknown', count: int = 0): pass";
    let tree = parse_python(source);
    let params_node = find_node(tree.root_node(), "parameters").expect("Should find parameters");

    let params = extract_python_parameters(params_node, source).expect("Should extract parameters");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].0, "name");
    assert_eq!(params[0].1, Some("str".to_string()));
    assert_eq!(params[1].0, "count");
    assert_eq!(params[1].1, Some("int".to_string()));
}

#[test]
fn test_extract_parameters_variadic() {
    let source = "def foo(*args, **kwargs): pass";
    let tree = parse_python(source);
    let params_node = find_node(tree.root_node(), "parameters").expect("Should find parameters");

    let params = extract_python_parameters(params_node, source).expect("Should extract parameters");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].0, "*args");
    assert_eq!(params[1].0, "**kwargs");
}

#[test]
fn test_extract_parameters_keyword_only() {
    let source = "def foo(a, *, b, c): pass";
    let tree = parse_python(source);
    let params_node = find_node(tree.root_node(), "parameters").expect("Should find parameters");

    let params = extract_python_parameters(params_node, source).expect("Should extract parameters");
    assert_eq!(params.len(), 4);
    assert_eq!(params[0].0, "a");
    assert_eq!(params[1].0, "*");
    assert_eq!(params[2].0, "b");
    assert_eq!(params[3].0, "c");
}

#[test]
fn test_extract_parameters_positional_only() {
    let source = "def foo(a, b, /, c): pass";
    let tree = parse_python(source);
    let params_node = find_node(tree.root_node(), "parameters").expect("Should find parameters");

    let params = extract_python_parameters(params_node, source).expect("Should extract parameters");
    assert_eq!(params.len(), 4);
    assert_eq!(params[0].0, "a");
    assert_eq!(params[1].0, "b");
    assert_eq!(params[2].0, "/");
    assert_eq!(params[3].0, "c");
}

// ============================================================================
// extract_docstring tests
// ============================================================================

#[test]
fn test_extract_docstring_simple() {
    let source = r#"
def foo():
    """This is a docstring."""
    pass
"#;
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");

    let doc = extract_docstring(func_node, source);
    assert!(doc.is_some());
    assert_eq!(doc.unwrap(), "This is a docstring.");
}

#[test]
fn test_extract_docstring_multiline() {
    let source = r#"
def foo():
    """
    A multiline docstring.

    Args:
        x: The x parameter
    """
    pass
"#;
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");

    let doc = extract_docstring(func_node, source);
    assert!(doc.is_some());
    let doc_text = doc.unwrap();
    assert!(doc_text.contains("multiline docstring"));
    assert!(doc_text.contains("Args:"));
}

#[test]
fn test_extract_docstring_none() {
    let source = "def foo(): pass";
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");

    let doc = extract_docstring(func_node, source);
    assert!(doc.is_none());
}

#[test]
fn test_extract_docstring_single_quotes() {
    let source = r#"
def foo():
    '''Single quote docstring.'''
    pass
"#;
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");

    let doc = extract_docstring(func_node, source);
    assert!(doc.is_some());
    assert_eq!(doc.unwrap(), "Single quote docstring.");
}

// ============================================================================
// extract_decorators tests
// ============================================================================

#[test]
fn test_extract_decorators_simple() {
    let source = r#"
@staticmethod
def foo(): pass
"#;
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");

    let decorators = extract_decorators(func_node, source);
    assert_eq!(decorators.len(), 1);
    assert_eq!(decorators[0], "staticmethod");
}

#[test]
fn test_extract_decorators_multiple() {
    let source = r#"
@classmethod
@lru_cache(maxsize=128)
def foo(cls): pass
"#;
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");

    let decorators = extract_decorators(func_node, source);
    assert_eq!(decorators.len(), 2);
    assert!(decorators.iter().any(|d| d == "classmethod"));
    assert!(decorators.iter().any(|d| d.contains("lru_cache")));
}

#[test]
fn test_extract_decorators_none() {
    let source = "def foo(): pass";
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");

    let decorators = extract_decorators(func_node, source);
    assert!(decorators.is_empty());
}

// ============================================================================
// is_async_function tests
// ============================================================================

#[test]
fn test_is_async_function_true() {
    let source = "async def foo(): pass";
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");

    assert!(is_async_function(func_node));
}

#[test]
fn test_is_async_function_false() {
    let source = "def foo(): pass";
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");

    assert!(!is_async_function(func_node));
}

// ============================================================================
// extract_base_classes tests
// ============================================================================

#[test]
fn test_extract_base_classes_single() {
    let source = "class Foo(Bar): pass";
    let tree = parse_python(source);
    let class_node =
        find_node(tree.root_node(), "class_definition").expect("Should find class_definition");

    let bases = extract_base_classes(class_node, source);
    assert_eq!(bases.len(), 1);
    assert_eq!(bases[0], "Bar");
}

#[test]
fn test_extract_base_classes_multiple() {
    let source = "class Foo(Bar, Baz, Qux): pass";
    let tree = parse_python(source);
    let class_node =
        find_node(tree.root_node(), "class_definition").expect("Should find class_definition");

    let bases = extract_base_classes(class_node, source);
    assert_eq!(bases.len(), 3);
    assert_eq!(bases[0], "Bar");
    assert_eq!(bases[1], "Baz");
    assert_eq!(bases[2], "Qux");
}

#[test]
fn test_extract_base_classes_none() {
    let source = "class Foo: pass";
    let tree = parse_python(source);
    let class_node =
        find_node(tree.root_node(), "class_definition").expect("Should find class_definition");

    let bases = extract_base_classes(class_node, source);
    assert!(bases.is_empty());
}

// ============================================================================
// extract_return_type tests
// ============================================================================

#[test]
fn test_extract_return_type_simple() {
    let source = "def foo() -> str: pass";
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");

    let ret_type = extract_return_type(func_node, source);
    assert!(ret_type.is_some());
    assert_eq!(ret_type.unwrap(), "str");
}

#[test]
fn test_extract_return_type_complex() {
    let source = "def foo() -> Optional[List[User]]: pass";
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");

    let ret_type = extract_return_type(func_node, source);
    assert!(ret_type.is_some());
    assert!(ret_type.unwrap().contains("Optional"));
}

#[test]
fn test_extract_return_type_none() {
    let source = "def foo(): pass";
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");

    let ret_type = extract_return_type(func_node, source);
    assert!(ret_type.is_none());
}

// ============================================================================
// filter_self_parameter tests
// ============================================================================

#[test]
fn test_filter_self_parameter() {
    let params = vec![
        ("self".to_string(), None),
        ("name".to_string(), Some("str".to_string())),
        ("age".to_string(), Some("int".to_string())),
    ];

    let filtered = filter_self_parameter(params);
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].0, "name");
    assert_eq!(filtered[1].0, "age");
}

#[test]
fn test_filter_cls_parameter() {
    let params = vec![
        ("cls".to_string(), None),
        ("value".to_string(), Some("int".to_string())),
    ];

    let filtered = filter_self_parameter(params);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].0, "value");
}

// ============================================================================
// extract_function_calls tests
// ============================================================================

#[test]
fn test_extract_function_calls_bare() {
    let source = r#"
def foo():
    bar()
    baz()
"#;
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");
    let import_map = ImportMap::new(".");

    let calls = extract_function_calls(func_node, source, &import_map, None);
    assert!(calls.iter().any(|c| c.target.contains("bar")));
    assert!(calls.iter().any(|c| c.target.contains("baz")));
}

#[test]
fn test_extract_function_calls_method() {
    let source = r#"
def foo():
    obj.method()
    self.helper()
"#;
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");
    let import_map = ImportMap::new(".");

    let calls = extract_function_calls(func_node, source, &import_map, None);
    assert!(calls.iter().any(|c| c.target.contains("method")));
    assert!(calls.iter().any(|c| c.target.contains("helper")));
}

#[test]
fn test_extract_function_calls_dedup() {
    let source = r#"
def foo():
    bar()
    bar()
    bar()
"#;
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");
    let import_map = ImportMap::new(".");

    let calls = extract_function_calls(func_node, source, &import_map, None);
    let bar_count = calls.iter().filter(|c| c.target.contains("bar")).count();
    assert_eq!(bar_count, 1);
}

// ============================================================================
// extract_type_references tests
// ============================================================================

#[test]
fn test_extract_type_references_simple() {
    let source = "def foo(user: User) -> None: pass";
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");
    let import_map = ImportMap::new(".");

    let types = extract_type_references(func_node, source, &import_map, None);
    assert!(types.iter().any(|t| t.target.contains("User")));
}

#[test]
fn test_extract_type_references_generic() {
    let source = "def foo() -> List[User]: pass";
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");
    let import_map = ImportMap::new(".");

    let types = extract_type_references(func_node, source, &import_map, None);
    // List is primitive, User is not
    assert!(types.iter().any(|t| t.target.contains("User")));
}

#[test]
fn test_extract_type_references_filters_primitives() {
    let source = "def foo(name: str, count: int) -> bool: pass";
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");
    let import_map = ImportMap::new(".");

    let types = extract_type_references(func_node, source, &import_map, None);
    // All primitives should be filtered out
    assert!(types.is_empty());
}

#[test]
fn test_extract_type_references_multiple() {
    let source = "def foo(user: User, admin: Admin) -> Result: pass";
    let tree = parse_python(source);
    let func_node = find_node(tree.root_node(), "function_definition")
        .expect("Should find function_definition");
    let import_map = ImportMap::new(".");

    let types = extract_type_references(func_node, source, &import_map, None);
    assert!(types.iter().any(|t| t.target.contains("User")));
    assert!(types.iter().any(|t| t.target.contains("Admin")));
    assert!(types.iter().any(|t| t.target.contains("Result")));
}
