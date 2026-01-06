//! Variable and constant queries for JavaScript and TypeScript

/// Query for const declarations at module level
///
/// Matches:
/// - `const foo = 1`
/// - `const { a, b } = obj` (destructuring)
/// - `export const foo = 1`
///
/// Note: This query excludes const declarations that are function expressions
/// or arrow functions (those are handled by function queries).
pub(crate) const CONST_QUERY: &str = r#"
(lexical_declaration
  kind: "const"
  (variable_declarator
    name: (identifier) @name
    value: (_) @value)) @const
  (#not-match? @value "^(function|async function|\\(|\\w+\\s*=>)")

(export_statement
  declaration: (lexical_declaration
    kind: "const"
    (variable_declarator
      name: (identifier) @name
      value: (_) @value))) @const
  (#not-match? @value "^(function|async function|\\(|\\w+\\s*=>)")
"#;

/// Query for let declarations at module level
///
/// Matches:
/// - `let foo = 1`
/// - `let foo`
/// - `export let foo = 1`
pub(crate) const LET_QUERY: &str = r#"
(lexical_declaration
  kind: "let"
  (variable_declarator
    name: (identifier) @name
    value: (_)? @value)) @let

(export_statement
  declaration: (lexical_declaration
    kind: "let"
    (variable_declarator
      name: (identifier) @name
      value: (_)? @value))) @let
"#;

/// Query for var declarations at module level
///
/// Matches:
/// - `var foo = 1`
/// - `var foo`
/// - `export var foo = 1`
pub(crate) const VAR_QUERY: &str = r#"
(variable_declaration
  (variable_declarator
    name: (identifier) @name
    value: (_)? @value)) @var

(export_statement
  declaration: (variable_declaration
    (variable_declarator
      name: (identifier) @name
      value: (_)? @value))) @var
"#;
