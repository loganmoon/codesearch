//! TypeScript-specific queries
//!
//! These queries are specific to TypeScript and not applicable to JavaScript.

/// Query for interface declarations
///
/// Matches:
/// - `interface Foo {}`
/// - `interface Foo extends Bar {}`
/// - `export interface Foo {}`
pub const INTERFACE_QUERY: &str = r#"
[
  (interface_declaration
    name: (type_identifier) @name
    (extends_type_clause
      (type_identifier) @extends)?
    body: (interface_body) @body) @interface

  (export_statement
    declaration: (interface_declaration
      name: (type_identifier) @name
      (extends_type_clause
        (type_identifier) @extends)?
      body: (interface_body) @body)) @interface
]
"#;

/// Query for type alias declarations
///
/// Matches:
/// - `type Foo = string`
/// - `type Foo<T> = T[]`
/// - `export type Foo = Bar`
pub const TYPE_ALIAS_QUERY: &str = r#"
[
  (type_alias_declaration
    name: (type_identifier) @name
    type_parameters: (type_parameters)? @type_params
    value: (_) @value) @type_alias

  (export_statement
    declaration: (type_alias_declaration
      name: (type_identifier) @name
      type_parameters: (type_parameters)? @type_params
      value: (_) @value)) @type_alias
]
"#;

/// Query for enum declarations
///
/// Matches:
/// - `enum Color { Red, Green, Blue }`
/// - `const enum Direction { Up, Down }`
/// - `export enum Status { Active, Inactive }`
pub const ENUM_QUERY: &str = r#"
[
  (enum_declaration
    name: (identifier) @name
    body: (enum_body) @body) @enum

  (export_statement
    declaration: (enum_declaration
      name: (identifier) @name
      body: (enum_body) @body)) @enum
]
"#;

/// Query for namespace declarations (internal modules)
///
/// Matches:
/// - `namespace Foo {}`
/// - `module Bar {}`
/// - `export namespace Foo {}`
pub const NAMESPACE_QUERY: &str = r#"
[
  (internal_module
    name: (identifier) @name
    body: (statement_block) @body) @namespace

  (internal_module
    name: (nested_identifier) @name
    body: (statement_block) @body) @namespace

  (export_statement
    declaration: (internal_module
      name: (identifier) @name
      body: (statement_block) @body)) @namespace

  (export_statement
    declaration: (internal_module
      name: (nested_identifier) @name
      body: (statement_block) @body)) @namespace
]
"#;

/// Query for ambient declarations (declare statements)
///
/// Matches:
/// - `declare function foo(): void`
/// - `declare const bar: string`
/// - `declare class Baz {}`
pub const AMBIENT_DECLARATION_QUERY: &str = r#"
(ambient_declaration
  [
    (function_signature
      name: (identifier) @name) @value
    (variable_declaration
      (variable_declarator
        name: (identifier) @name)) @value
    (class_declaration
      name: (identifier) @name) @value
  ]) @ambient
"#;
