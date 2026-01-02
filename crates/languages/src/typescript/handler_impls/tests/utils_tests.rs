//! Tests for TypeScript utility functions
//!
//! Tests the TypeScript-specific utility functions for type reference extraction
//! and primitive detection.

use crate::common::import_map::ImportMap;
use crate::typescript::utils::{extract_type_references, is_ts_primitive};
use tree_sitter::Parser;

/// Helper to parse TypeScript source and get the root node
fn parse_ts(source: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .expect("Failed to set TypeScript language");
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
// is_ts_primitive tests
// ============================================================================

#[test]
fn test_is_ts_primitive_basic_types() {
    assert!(is_ts_primitive("string"));
    assert!(is_ts_primitive("number"));
    assert!(is_ts_primitive("boolean"));
    assert!(is_ts_primitive("any"));
    assert!(is_ts_primitive("void"));
    assert!(is_ts_primitive("null"));
    assert!(is_ts_primitive("undefined"));
}

#[test]
fn test_is_ts_primitive_case_insensitive() {
    assert!(is_ts_primitive("String"));
    assert!(is_ts_primitive("NUMBER"));
    assert!(is_ts_primitive("Boolean"));
    assert!(is_ts_primitive("VOID"));
}

#[test]
fn test_is_ts_primitive_additional_types() {
    assert!(is_ts_primitive("never"));
    assert!(is_ts_primitive("unknown"));
    assert!(is_ts_primitive("object"));
    assert!(is_ts_primitive("symbol"));
    assert!(is_ts_primitive("bigint"));
    assert!(is_ts_primitive("array"));
    assert!(is_ts_primitive("function"));
    assert!(is_ts_primitive("promise"));
}

#[test]
fn test_is_ts_primitive_utility_types() {
    assert!(is_ts_primitive("readonly"));
    assert!(is_ts_primitive("record"));
    assert!(is_ts_primitive("partial"));
    assert!(is_ts_primitive("required"));
    assert!(is_ts_primitive("pick"));
    assert!(is_ts_primitive("omit"));
    assert!(is_ts_primitive("exclude"));
    assert!(is_ts_primitive("extract"));
    assert!(is_ts_primitive("returntype"));
    assert!(is_ts_primitive("parameters"));
}

#[test]
fn test_is_ts_primitive_non_primitives() {
    assert!(!is_ts_primitive("MyClass"));
    assert!(!is_ts_primitive("CustomType"));
    assert!(!is_ts_primitive("User"));
    assert!(!is_ts_primitive("Result"));
    assert!(!is_ts_primitive("IUserService"));
}

// ============================================================================
// extract_type_references tests
// ============================================================================

#[test]
fn test_extract_type_references_parameter() {
    let source = "function foo(user: User): void {}";
    let tree = parse_ts(source);
    let func_node =
        find_node(tree.root_node(), "function_declaration").expect("Should find function");
    let import_map = ImportMap::new(".");

    let types = extract_type_references(func_node, source, &import_map, None);
    assert!(types.iter().any(|t| t.target().contains("User")));
    // void is primitive, should not be included
    assert!(!types.iter().any(|t| t.target().contains("void")));
}

#[test]
fn test_extract_type_references_return_type() {
    let source = "function getUsers(): UserList { return []; }";
    let tree = parse_ts(source);
    let func_node =
        find_node(tree.root_node(), "function_declaration").expect("Should find function");
    let import_map = ImportMap::new(".");

    let types = extract_type_references(func_node, source, &import_map, None);
    assert!(types.iter().any(|t| t.target().contains("UserList")));
}

#[test]
fn test_extract_type_references_generic() {
    let source = "function foo(): Promise<User> { return Promise.resolve(null); }";
    let tree = parse_ts(source);
    let func_node =
        find_node(tree.root_node(), "function_declaration").expect("Should find function");
    let import_map = ImportMap::new(".");

    let types = extract_type_references(func_node, source, &import_map, None);
    // Promise is primitive, User is not
    assert!(types.iter().any(|t| t.target().contains("User")));
}

#[test]
fn test_extract_type_references_filters_primitives() {
    let source = "function foo(name: string, age: number): boolean { return true; }";
    let tree = parse_ts(source);
    let func_node =
        find_node(tree.root_node(), "function_declaration").expect("Should find function");
    let import_map = ImportMap::new(".");

    let types = extract_type_references(func_node, source, &import_map, None);
    // All primitives should be filtered out
    assert!(types.is_empty());
}

#[test]
fn test_extract_type_references_multiple() {
    let source = "function process(user: User, settings: Settings): Result {}";
    let tree = parse_ts(source);
    let func_node =
        find_node(tree.root_node(), "function_declaration").expect("Should find function");
    let import_map = ImportMap::new(".");

    let types = extract_type_references(func_node, source, &import_map, None);
    assert!(types.iter().any(|t| t.target().contains("User")));
    assert!(types.iter().any(|t| t.target().contains("Settings")));
    assert!(types.iter().any(|t| t.target().contains("Result")));
}

#[test]
fn test_extract_type_references_dedup() {
    let source = "function foo(a: User, b: User, c: User): User {}";
    let tree = parse_ts(source);
    let func_node =
        find_node(tree.root_node(), "function_declaration").expect("Should find function");
    let import_map = ImportMap::new(".");

    let types = extract_type_references(func_node, source, &import_map, None);
    // Should only have one entry for User
    let user_count = types.iter().filter(|t| t.target().contains("User")).count();
    assert_eq!(user_count, 1);
}

#[test]
fn test_extract_type_references_scoped() {
    let source = "function foo(): Namespace.Type { return null; }";
    let tree = parse_ts(source);
    let func_node =
        find_node(tree.root_node(), "function_declaration").expect("Should find function");
    let import_map = ImportMap::new(".");

    let types = extract_type_references(func_node, source, &import_map, None);
    // Should capture the full scoped type
    assert!(types.iter().any(|t| t.target().contains("Namespace.Type")));
}

#[test]
fn test_extract_implements_types() {
    use crate::typescript::handler_impls::type_handlers::test_extract_implements_types;

    // Test basic implements
    let source = r#"class Resource implements Disposable, Serializable {
    dispose(): void {}
}"#;
    let tree = parse_ts(source);
    let class_node = find_node(tree.root_node(), "class_declaration").expect("Should find class");

    let types = test_extract_implements_types(class_node, source).expect("Should extract types");
    assert_eq!(
        types.len(),
        2,
        "Expected 2 implements types, got: {types:?}"
    );
    assert!(types.contains(&"Disposable".to_string()));
    assert!(types.contains(&"Serializable".to_string()));
}

#[test]
fn test_extract_implements_types_with_export() {
    use crate::typescript::handler_impls::type_handlers::test_extract_implements_types;

    // Test with export statement
    let source = r#"export class Resource implements Disposable, Serializable {
    dispose(): void {}
}"#;
    let tree = parse_ts(source);
    let class_node = find_node(tree.root_node(), "class_declaration").expect("Should find class");

    let types = test_extract_implements_types(class_node, source).expect("Should extract types");
    assert_eq!(
        types.len(),
        2,
        "Expected 2 implements types, got: {types:?}"
    );
    assert!(types.contains(&"Disposable".to_string()));
    assert!(types.contains(&"Serializable".to_string()));
}

#[test]
fn test_interface_members_structure() {
    // Test the tree structure for interface members
    let source = r#"export interface User {
    id: number;
    name: string;
    email?: string;
    readonly createdAt: Date;
    greet(): string;
    updateEmail(email: string): void;
}"#;
    let tree = parse_ts(source);

    fn print_tree(node: tree_sitter::Node, source: &str, depth: usize) {
        let indent = "  ".repeat(depth);
        let text: String = source[node.start_byte()..node.end_byte()]
            .chars()
            .take(50)
            .collect();
        println!("{}{} [{}]", indent, node.kind(), text.replace('\n', "\\n"));

        for child in node.children(&mut node.walk()) {
            print_tree(child, source, depth + 1);
        }
    }

    println!("\n=== TypeScript interface with members ===");
    print_tree(tree.root_node(), source, 0);
}
