//! Module-related queries for JavaScript and TypeScript
//!
//! ES Modules use import/export statements for module management.

/// Query for import statements
///
/// Matches:
/// - `import foo from 'module'`
/// - `import { foo, bar } from 'module'`
/// - `import * as foo from 'module'`
/// - `import 'module'` (side-effect import)
pub(crate) const _IMPORT_QUERY: &str = r#"
(import_statement
  source: (string) @source
  (import_clause
    [
      (identifier) @default
      (named_imports
        (import_specifier
          name: (identifier) @specifier_name
          alias: (identifier)? @specifier_alias)*)
      (namespace_import
        (identifier) @namespace)
    ]?)?) @import
"#;

/// Query for export statements
///
/// Matches:
/// - `export { foo, bar }`
/// - `export { foo as bar }`
/// - `export { foo } from 'module'` (re-export)
/// - `export * from 'module'` (re-export all)
/// - `export * as foo from 'module'` (namespace re-export)
pub(crate) const _EXPORT_QUERY: &str = r#"
(export_statement
  source: (string)? @source
  [
    (export_clause
      (export_specifier
        name: (identifier) @name
        alias: (identifier)? @alias)*)
    (namespace_export
      (identifier)? @namespace)
  ]?) @export
"#;

/// Query for dynamic imports
///
/// Matches:
/// - `import('module')`
/// - `await import('module')`
pub(crate) const _DYNAMIC_IMPORT_QUERY: &str = r#"
(call_expression
  function: (import)
  arguments: (arguments
    (string) @source)) @dynamic_import
"#;
