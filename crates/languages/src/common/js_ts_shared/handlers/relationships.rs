//! Relationship extraction for JavaScript and TypeScript
//!
//! This module provides functions for extracting IMPORTS, REEXPORTS, USES, and CALLS
//! relationships from JavaScript and TypeScript source code.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::entity_building::ExtractionContext;
use codesearch_core::entities::{
    EntityRelationshipData, ReferenceType, SourceLocation, SourceReference,
};
use std::collections::HashSet;
use tree_sitter::Node;

// =============================================================================
// Primitive type filtering
// =============================================================================

/// TypeScript/JavaScript primitive types to skip in type reference extraction
const TS_PRIMITIVE_TYPES: &[&str] = &[
    "string",
    "number",
    "boolean",
    "void",
    "null",
    "undefined",
    "never",
    "any",
    "unknown",
    "object",
    "symbol",
    "bigint",
    // Built-in generic types
    "Array",
    "Promise",
    "Map",
    "Set",
    "Record",
    "Partial",
    "Required",
    "Readonly",
    "Pick",
    "Omit",
    "Exclude",
    "Extract",
    "NonNullable",
    "ReturnType",
    "Parameters",
    "InstanceType",
];

/// Check if a type name is a primitive/built-in type
fn is_primitive_type(name: &str) -> bool {
    TS_PRIMITIVE_TYPES.contains(&name)
}

// =============================================================================
// Import extraction
// =============================================================================

/// Extract import path from a string literal node (removing quotes)
fn extract_import_path(string_node: Node, source: &str) -> Option<String> {
    let text = &source[string_node.byte_range()];
    // Remove quotes (both single and double)
    let path = text
        .trim_start_matches(['"', '\''])
        .trim_end_matches(['"', '\'']);
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// Derive a qualified name from an import path and imported name
fn derive_import_qualified_name(import_path: &str, name: &str) -> String {
    // Convert relative path to module name
    // "./utils" -> "utils"
    // "../lib/helpers" -> "lib.helpers"
    // "@scope/package" -> "@scope/package"
    let module_part = import_path
        .trim_start_matches("./")
        .trim_start_matches("../")
        .replace('/', ".");

    // Remove file extension if present
    let module_part = module_part
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .trim_end_matches(".js")
        .trim_end_matches(".jsx");

    format!("{module_part}.{name}")
}

/// Extract imports from a module node
///
/// Handles:
/// - Named imports: `import { foo, bar } from './module'`
/// - Default imports: `import Foo from './module'`
/// - Namespace imports: `import * as Utils from './module'`
/// - Side-effect imports are ignored: `import './module'`
pub(crate) fn extract_imports(module_node: Node, source: &str) -> Vec<SourceReference> {
    let mut imports = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = module_node.walk();

    for child in module_node.children(&mut cursor) {
        if child.kind() != "import_statement" {
            continue;
        }

        // Get the source path (the string after 'from')
        let Some(source_node) = child.child_by_field_name("source") else {
            continue;
        };
        let Some(import_path) = extract_import_path(source_node, source) else {
            continue;
        };

        // Get the import clause (contains the actual imports)
        let Some(import_clause) = child
            .children(&mut child.walk())
            .find(|n| n.kind() == "import_clause")
        else {
            continue;
        };

        // Process each part of the import clause
        let mut clause_cursor = import_clause.walk();
        for clause_child in import_clause.children(&mut clause_cursor) {
            match clause_child.kind() {
                // Default import: import Foo from './module'
                "identifier" => {
                    let name = &source[clause_child.byte_range()];
                    let qualified_name = derive_import_qualified_name(&import_path, name);
                    if seen.insert(qualified_name.clone()) {
                        if let Ok(source_ref) = SourceReference::builder()
                            .target(qualified_name)
                            .simple_name(name.to_string())
                            .location(SourceLocation::from_tree_sitter_node(clause_child))
                            .ref_type(ReferenceType::Import)
                            .build()
                        {
                            imports.push(source_ref);
                        }
                    }
                }
                // Named imports: import { foo, bar as baz } from './module'
                "named_imports" => {
                    let mut specifier_cursor = clause_child.walk();
                    for specifier in clause_child.children(&mut specifier_cursor) {
                        if specifier.kind() != "import_specifier" {
                            continue;
                        }
                        // Get the imported name (not the local alias)
                        if let Some(name_node) = specifier.child_by_field_name("name") {
                            let name = &source[name_node.byte_range()];
                            let qualified_name = derive_import_qualified_name(&import_path, name);
                            if seen.insert(qualified_name.clone()) {
                                if let Ok(source_ref) = SourceReference::builder()
                                    .target(qualified_name)
                                    .simple_name(name.to_string())
                                    .location(SourceLocation::from_tree_sitter_node(name_node))
                                    .ref_type(ReferenceType::Import)
                                    .build()
                                {
                                    imports.push(source_ref);
                                }
                            }
                        }
                    }
                }
                // Namespace import: import * as Utils from './module'
                "namespace_import" => {
                    // For namespace imports, the target is the module itself
                    let module_name = import_path
                        .trim_start_matches("./")
                        .trim_start_matches("../")
                        .replace('/', ".")
                        .trim_end_matches(".ts")
                        .trim_end_matches(".tsx")
                        .trim_end_matches(".js")
                        .trim_end_matches(".jsx")
                        .to_string();

                    if seen.insert(module_name.clone()) {
                        // Get the alias name for simple_name
                        let simple_name = clause_child
                            .children(&mut clause_child.walk())
                            .find(|n| n.kind() == "identifier")
                            .map(|n| source[n.byte_range()].to_string())
                            .unwrap_or_else(|| module_name.clone());

                        if let Ok(source_ref) = SourceReference::builder()
                            .target(module_name)
                            .simple_name(simple_name)
                            .location(SourceLocation::from_tree_sitter_node(clause_child))
                            .ref_type(ReferenceType::Import)
                            .build()
                        {
                            imports.push(source_ref);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    imports
}

// =============================================================================
// Re-export extraction
// =============================================================================

/// Extract re-exports from a module node
///
/// Handles:
/// - Named re-exports: `export { foo, bar } from './module'`
/// - Star re-exports: `export * from './module'`
/// - Namespace re-exports: `export * as Utils from './module'`
pub(crate) fn extract_reexports(module_node: Node, source: &str) -> Vec<SourceReference> {
    let mut reexports = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = module_node.walk();

    for child in module_node.children(&mut cursor) {
        if child.kind() != "export_statement" {
            continue;
        }

        // Only process re-exports (those with a 'source' field)
        let Some(source_node) = child.child_by_field_name("source") else {
            continue;
        };
        let Some(export_path) = extract_import_path(source_node, source) else {
            continue;
        };

        // Check for star export or namespace export
        let has_star = child
            .children(&mut child.walk())
            .any(|n| n.kind() == "*" || &source[n.byte_range()] == "*");

        let namespace_export = child
            .children(&mut child.walk())
            .find(|n| n.kind() == "namespace_export");

        if let Some(ns_export) = namespace_export {
            // Namespace re-export: export * as Utils from './module'
            let module_name = export_path
                .trim_start_matches("./")
                .trim_start_matches("../")
                .replace('/', ".")
                .trim_end_matches(".ts")
                .trim_end_matches(".tsx")
                .trim_end_matches(".js")
                .trim_end_matches(".jsx")
                .to_string();

            if seen.insert(module_name.clone()) {
                let simple_name = ns_export
                    .children(&mut ns_export.walk())
                    .find(|n| n.kind() == "identifier")
                    .map(|n| source[n.byte_range()].to_string())
                    .unwrap_or_else(|| module_name.clone());

                if let Ok(source_ref) = SourceReference::builder()
                    .target(module_name)
                    .simple_name(simple_name)
                    .location(SourceLocation::from_tree_sitter_node(ns_export))
                    .ref_type(ReferenceType::Reexport)
                    .build()
                {
                    reexports.push(source_ref);
                }
            }
        } else if has_star {
            // Star re-export: export * from './module'
            let module_name = export_path
                .trim_start_matches("./")
                .trim_start_matches("../")
                .replace('/', ".")
                .trim_end_matches(".ts")
                .trim_end_matches(".tsx")
                .trim_end_matches(".js")
                .trim_end_matches(".jsx")
                .to_string();

            if seen.insert(module_name.clone()) {
                if let Ok(source_ref) = SourceReference::builder()
                    .target(module_name.clone())
                    .simple_name(module_name)
                    .location(SourceLocation::from_tree_sitter_node(child))
                    .ref_type(ReferenceType::Reexport)
                    .build()
                {
                    reexports.push(source_ref);
                }
            }
        } else if let Some(export_clause) = child
            .children(&mut child.walk())
            .find(|n| n.kind() == "export_clause")
        {
            // Named re-exports: export { foo, bar } from './module'
            let mut specifier_cursor = export_clause.walk();
            for specifier in export_clause.children(&mut specifier_cursor) {
                if specifier.kind() != "export_specifier" {
                    continue;
                }
                // Get the exported name
                if let Some(name_node) = specifier.child_by_field_name("name") {
                    let name = &source[name_node.byte_range()];
                    let qualified_name = derive_import_qualified_name(&export_path, name);
                    if seen.insert(qualified_name.clone()) {
                        if let Ok(source_ref) = SourceReference::builder()
                            .target(qualified_name)
                            .simple_name(name.to_string())
                            .location(SourceLocation::from_tree_sitter_node(name_node))
                            .ref_type(ReferenceType::Reexport)
                            .build()
                        {
                            reexports.push(source_ref);
                        }
                    }
                }
            }
        }
    }

    reexports
}

// =============================================================================
// Type reference extraction (USES)
// =============================================================================

/// Extract type references from a node (for USES relationships)
///
/// Walks the AST looking for type annotations and extracts referenced type names,
/// filtering out primitive types.
pub(crate) fn extract_type_references(node: Node, source: &str) -> Vec<SourceReference> {
    let mut refs = Vec::new();
    let mut seen = HashSet::new();
    extract_type_refs_recursive(node, source, &mut refs, &mut seen);
    refs
}

fn extract_type_refs_recursive(
    node: Node,
    source: &str,
    refs: &mut Vec<SourceReference>,
    seen: &mut HashSet<String>,
) {
    match node.kind() {
        "type_identifier" => {
            let name = &source[node.byte_range()];
            if !is_primitive_type(name) && seen.insert(name.to_string()) {
                if let Ok(source_ref) = SourceReference::builder()
                    .target(name.to_string())
                    .simple_name(name.to_string())
                    .location(SourceLocation::from_tree_sitter_node(node))
                    .ref_type(ReferenceType::TypeUsage)
                    .build()
                {
                    refs.push(source_ref);
                }
            }
        }
        "generic_type" => {
            // Extract base type name from generic: Foo<T> -> Foo
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = &source[name_node.byte_range()];
                if !is_primitive_type(name) && seen.insert(name.to_string()) {
                    if let Ok(source_ref) = SourceReference::builder()
                        .target(name.to_string())
                        .simple_name(name.to_string())
                        .location(SourceLocation::from_tree_sitter_node(name_node))
                        .ref_type(ReferenceType::TypeUsage)
                        .build()
                    {
                        refs.push(source_ref);
                    }
                }
            }
            // Also process type arguments
            if let Some(type_args) = node.child_by_field_name("type_arguments") {
                extract_type_refs_recursive(type_args, source, refs, seen);
            }
        }
        _ => {
            // Recurse into children
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_type_refs_recursive(child, source, refs, seen);
            }
        }
    }
}

// =============================================================================
// Function call extraction (CALLS)
// =============================================================================

/// Extract function calls from a node (for CALLS relationships)
///
/// Walks the AST looking for call expressions and extracts the callee names.
pub(crate) fn extract_function_calls(node: Node, source: &str) -> Vec<SourceReference> {
    let mut refs = Vec::new();
    let mut seen = HashSet::new();
    extract_calls_recursive(node, source, &mut refs, &mut seen);
    refs
}

fn extract_calls_recursive(
    node: Node,
    source: &str,
    refs: &mut Vec<SourceReference>,
    seen: &mut HashSet<String>,
) {
    if node.kind() == "call_expression" {
        if let Some(callee) = node.child_by_field_name("function") {
            let (target, simple_name, location) = match callee.kind() {
                // Bare function call: foo()
                "identifier" => {
                    let name = source[callee.byte_range()].to_string();
                    (name.clone(), name, callee)
                }
                // Method call: obj.method() or Module.func()
                "member_expression" => {
                    let text = source[callee.byte_range()].to_string();
                    // Get the property (method name) for simple_name
                    let simple = callee
                        .child_by_field_name("property")
                        .map(|n| source[n.byte_range()].to_string())
                        .unwrap_or_else(|| text.clone());
                    (text, simple, callee)
                }
                // Other cases (like call().method())
                _ => {
                    let text = source[callee.byte_range()].to_string();
                    (text.clone(), text, callee)
                }
            };

            if seen.insert(target.clone()) {
                if let Ok(source_ref) = SourceReference::builder()
                    .target(target)
                    .simple_name(simple_name)
                    .location(SourceLocation::from_tree_sitter_node(location))
                    .ref_type(ReferenceType::Call)
                    .build()
                {
                    refs.push(source_ref);
                }
            }
        }
    }

    // Recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_calls_recursive(child, source, refs, seen);
    }
}

// =============================================================================
// Combined relationship extraction for entities
// =============================================================================

/// Extract relationships for function/method entities (CALLS + USES)
///
/// This function is designed to be used with the `define_handler!` macro's
/// `relationships:` parameter.
pub(crate) fn extract_function_relationships(
    ctx: &ExtractionContext,
    node: Node,
) -> EntityRelationshipData {
    // Find the function body to search for calls
    let body_node = find_function_body(node);

    EntityRelationshipData {
        calls: body_node
            .map(|b| extract_function_calls(b, ctx.source))
            .unwrap_or_default(),
        uses_types: extract_type_references(node, ctx.source),
        ..Default::default()
    }
}

/// Find the body node of a function (statement_block or expression)
fn find_function_body(node: Node) -> Option<Node> {
    // Try to find body field directly
    if let Some(body) = node.child_by_field_name("body") {
        return Some(body);
    }

    // For arrow functions, the body might be a direct expression
    let mut cursor = node.walk();
    let result = node
        .children(&mut cursor)
        .find(|child| child.kind() == "statement_block");
    result
}

/// Extract relationships for module entities (IMPORTS + REEXPORTS)
pub(crate) fn extract_module_relationships(node: Node, source: &str) -> EntityRelationshipData {
    EntityRelationshipData {
        imports: extract_imports(node, source),
        reexports: extract_reexports(node, source),
        ..Default::default()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn parse_ts(source: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .ok();
        parser.parse(source, None).unwrap()
    }

    #[test]
    fn test_extract_named_imports() {
        let source = r#"import { foo, bar } from './utils';"#;
        let tree = parse_ts(source);
        let imports = extract_imports(tree.root_node(), source);

        assert_eq!(imports.len(), 2);
        assert!(imports.iter().any(|r| r.simple_name() == "foo"));
        assert!(imports.iter().any(|r| r.simple_name() == "bar"));
    }

    #[test]
    fn test_extract_default_import() {
        let source = r#"import Utils from './utils';"#;
        let tree = parse_ts(source);
        let imports = extract_imports(tree.root_node(), source);

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].simple_name(), "Utils");
    }

    #[test]
    fn test_extract_namespace_import() {
        let source = r#"import * as Utils from './utils';"#;
        let tree = parse_ts(source);
        let imports = extract_imports(tree.root_node(), source);

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].simple_name(), "Utils");
        assert_eq!(imports[0].target(), "utils");
    }

    #[test]
    fn test_extract_star_reexport() {
        let source = r#"export * from './internal';"#;
        let tree = parse_ts(source);
        let reexports = extract_reexports(tree.root_node(), source);

        assert_eq!(reexports.len(), 1);
        assert_eq!(reexports[0].target(), "internal");
    }

    #[test]
    fn test_extract_named_reexport() {
        let source = r#"export { foo, bar } from './internal';"#;
        let tree = parse_ts(source);
        let reexports = extract_reexports(tree.root_node(), source);

        assert_eq!(reexports.len(), 2);
        assert!(reexports.iter().any(|r| r.simple_name() == "foo"));
        assert!(reexports.iter().any(|r| r.simple_name() == "bar"));
    }

    #[test]
    fn test_extract_type_references() {
        let source = r#"
            function process(user: User): Result<User> {
                const data: Data = {};
                return data;
            }
        "#;
        let tree = parse_ts(source);
        let refs = extract_type_references(tree.root_node(), source);

        // Should find User (twice but deduplicated), Result, Data
        assert!(refs.iter().any(|r| r.simple_name() == "User"));
        assert!(refs.iter().any(|r| r.simple_name() == "Result"));
        assert!(refs.iter().any(|r| r.simple_name() == "Data"));
        // Should not include primitives
        assert!(!refs.iter().any(|r| r.simple_name() == "string"));
    }

    #[test]
    fn test_extract_function_calls() {
        let source = r#"
            function pipeline() {
                const a = step1();
                const b = step2(a);
                obj.method();
            }
        "#;
        let tree = parse_ts(source);
        let calls = extract_function_calls(tree.root_node(), source);

        assert!(calls.iter().any(|r| r.simple_name() == "step1"));
        assert!(calls.iter().any(|r| r.simple_name() == "step2"));
        assert!(calls.iter().any(|r| r.simple_name() == "method"));
    }
}
