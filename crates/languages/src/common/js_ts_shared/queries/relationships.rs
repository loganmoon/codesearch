//! Relationship extraction queries for JavaScript and TypeScript
//!
//! These queries are used to extract relationships between entities:
//! function calls, type usages, imports, and re-exports.

/// Query for call expressions within function bodies
///
/// Captures function calls including:
/// - Direct calls: `foo()`
/// - Method calls: `obj.bar()` (captures `bar`)
pub(crate) const CALL_EXPRESSION_QUERY: &str = r#"
(call_expression
  function: (identifier) @callee)
(call_expression
  function: (member_expression
    property: (property_identifier) @callee))
"#;

/// Query for type annotations
///
/// Captures type references in type annotations:
/// - Simple types: `x: Foo`
/// - Generic types: `x: Bar<T>` (captures `Bar`)
pub(crate) const TYPE_ANNOTATION_QUERY: &str = r#"
(type_annotation (type_identifier) @type_ref)
(type_annotation (generic_type name: (type_identifier) @type_ref))
"#;

/// Query for import statements
///
/// Captures various import forms:
/// - Default imports: `import Foo from 'module'`
/// - Named imports: `import { Foo } from 'module'`
/// - Namespace imports: `import * as Foo from 'module'`
pub(crate) const IMPORT_STATEMENT_QUERY: &str = r#"
(import_statement
  (import_clause
    (identifier) @default_import)
  source: (string) @source)

(import_statement
  (import_clause
    (named_imports
      (import_specifier
        name: (identifier) @named_import)))
  source: (string) @source)

(import_statement
  (import_clause
    (namespace_import
      (identifier) @ns_import))
  source: (string) @source)
"#;

/// Query for re-export statements
///
/// Captures:
/// - Named re-exports: `export { Foo } from 'module'`
/// - Star re-exports: `export * from 'module'`
pub(crate) const REEXPORT_STATEMENT_QUERY: &str = r#"
(export_statement
  (export_clause
    (export_specifier
      name: (identifier) @export_name))
  source: (string) @source)

(export_statement
  "*"
  source: (string) @source) @star_export
"#;
