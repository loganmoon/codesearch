//! Scope patterns for JavaScript and TypeScript
//!
//! These patterns define which AST nodes contribute to qualified names.
//! When building a qualified name, the extractor walks up the AST and
//! collects names from nodes matching these patterns.

use crate::qualified_name::ScopePattern;

/// Scope patterns shared by JavaScript and TypeScript
///
/// These patterns identify AST nodes that contribute to the parent scope
/// of an entity's qualified name. For example, a method inside a class
/// will have the class name as its parent scope.
///
/// # Patterns
///
/// - `class_declaration` - Named class declarations: `class Foo { ... }`
/// - `class` - Class expressions assigned to variables: `const Foo = class { ... }`
/// - `function_declaration` - Named function declarations (for nested functions)
/// - `method_definition` - Methods in classes (for nested arrow functions)
pub const SCOPE_PATTERNS: &[ScopePattern] = &[
    // Class declarations: class Foo { ... }
    ScopePattern {
        node_kind: "class_declaration",
        field_name: "name",
    },
    // Class expressions: const Foo = class { ... }
    // The name comes from the class expression itself if named, otherwise from variable
    ScopePattern {
        node_kind: "class",
        field_name: "name",
    },
    // Function declarations contribute to scope for nested functions
    ScopePattern {
        node_kind: "function_declaration",
        field_name: "name",
    },
    // Method definitions contribute to scope for nested arrow functions
    ScopePattern {
        node_kind: "method_definition",
        field_name: "name",
    },
];

/// TypeScript-specific scope patterns
///
/// Includes all JavaScript patterns plus:
/// - `internal_module` - Namespace/module declarations: `namespace Foo { ... }`
pub const TS_SCOPE_PATTERNS: &[ScopePattern] = &[
    // Class declarations: class Foo { ... }
    ScopePattern {
        node_kind: "class_declaration",
        field_name: "name",
    },
    // Class expressions: const Foo = class { ... }
    ScopePattern {
        node_kind: "class",
        field_name: "name",
    },
    // Function declarations contribute to scope for nested functions
    ScopePattern {
        node_kind: "function_declaration",
        field_name: "name",
    },
    // Method definitions contribute to scope for nested arrow functions
    ScopePattern {
        node_kind: "method_definition",
        field_name: "name",
    },
    // TypeScript namespaces/internal modules: namespace Foo { ... }
    ScopePattern {
        node_kind: "internal_module",
        field_name: "name",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_patterns_defined() {
        assert_eq!(SCOPE_PATTERNS.len(), 4);
    }

    #[test]
    fn test_class_declaration_pattern() {
        let pattern = &SCOPE_PATTERNS[0];
        assert_eq!(pattern.node_kind, "class_declaration");
        assert_eq!(pattern.field_name, "name");
    }

    #[test]
    fn test_class_expression_pattern() {
        let pattern = &SCOPE_PATTERNS[1];
        assert_eq!(pattern.node_kind, "class");
        assert_eq!(pattern.field_name, "name");
    }

    #[test]
    fn test_function_declaration_pattern() {
        let pattern = &SCOPE_PATTERNS[2];
        assert_eq!(pattern.node_kind, "function_declaration");
        assert_eq!(pattern.field_name, "name");
    }

    #[test]
    fn test_method_definition_pattern() {
        let pattern = &SCOPE_PATTERNS[3];
        assert_eq!(pattern.node_kind, "method_definition");
        assert_eq!(pattern.field_name, "name");
    }
}
