//! TypeScript-specific queries
//!
//! These queries are specific to TypeScript and not applicable to JavaScript.
//! TypeScript uses `type_identifier` for type names (classes, interfaces, etc.)
//! instead of `identifier` which JavaScript uses.

/// Query for class declarations (TypeScript version)
///
/// TypeScript uses `type_identifier` for class names.
/// Matches:
/// - `class Foo {}`
/// - `class Foo extends Bar {}`
/// - `export class Foo {}`
pub(crate) const TS_CLASS_DECLARATION_QUERY: &str = r#"
[
  (class_declaration
    name: (type_identifier) @name
    (class_heritage
      (extends_clause
        value: (_) @extends))?
    body: (class_body) @body) @class

  (export_statement
    declaration: (class_declaration
      name: (type_identifier) @name
      (class_heritage
        (extends_clause
          value: (_) @extends))?
      body: (class_body) @body)) @class
]
"#;

/// Query for class expressions assigned to variables (TypeScript version)
///
/// Matches:
/// - `const Foo = class {}`
/// - `const Foo = class Bar {}`
/// - `let Foo = class extends Base {}`
pub(crate) const TS_CLASS_EXPRESSION_QUERY: &str = r#"
(lexical_declaration
  (variable_declarator
    name: (identifier) @name
    value: (class
      name: (type_identifier)? @class_name
      (class_heritage
        (extends_clause
          value: (_) @extends))?
      body: (class_body) @body))) @class

(variable_declaration
  (variable_declarator
    name: (identifier) @name
    value: (class
      name: (type_identifier)? @class_name
      (class_heritage
        (extends_clause
          value: (_) @extends))?
      body: (class_body) @body))) @class
"#;

/// Query for class fields/properties (TypeScript version)
///
/// TypeScript only uses `public_field_definition` (not `field_definition`).
/// Matches:
/// - `field = value`
/// - `static field = value`
/// - `#privateField = value`
/// - `field` (no initializer)
pub(crate) const TS_PROPERTY_QUERY: &str = r#"
(class_body
  (public_field_definition
    name: [
      (property_identifier) @name
      (private_property_identifier) @name
    ]
    value: (_)? @value) @property)
"#;

/// Query for interface declarations
///
/// Matches:
/// - `interface Foo {}`
/// - `interface Foo extends Bar {}`
/// - `export interface Foo {}`
pub(crate) const INTERFACE_QUERY: &str = r#"
[
  ;; Interface without extends
  (interface_declaration
    name: (type_identifier) @name
    body: (interface_body) @body) @interface

  ;; Interface with extends
  (interface_declaration
    name: (type_identifier) @name
    (extends_type_clause) @extends_clause
    body: (interface_body) @body) @interface

  ;; Exported interface without extends
  (export_statement
    declaration: (interface_declaration
      name: (type_identifier) @name
      body: (interface_body) @body)) @interface

  ;; Exported interface with extends
  (export_statement
    declaration: (interface_declaration
      name: (type_identifier) @name
      (extends_type_clause) @extends_clause
      body: (interface_body) @body)) @interface
]
"#;

/// Query for type alias declarations
///
/// Matches:
/// - `type Foo = string`
/// - `type Foo<T> = T[]`
/// - `export type Foo = Bar`
pub(crate) const TYPE_ALIAS_QUERY: &str = r#"
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
pub(crate) const ENUM_QUERY: &str = r#"
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

/// Query for enum members (variants)
///
/// Matches enum members inside enum bodies:
/// - `Red` (simple member, no value)
/// - `Active = 1` (member with numeric value)
/// - `Info = "INFO"` (member with string value)
///
/// The qualified name (e.g., `module.EnumName.MemberName`) is built automatically
/// via AST parent traversal since `enum_declaration` is registered as a scope pattern.
pub(crate) const ENUM_MEMBER_QUERY: &str = r#"
[
  ;; Simple enum member: enum Color { Red, Green }
  ;; property_identifier is a direct child of enum_body
  (enum_body
    (property_identifier) @name) @enum_member

  ;; Enum member with value: enum Status { Active = 1 }
  (enum_body
    (enum_assignment
      name: (property_identifier) @name)) @enum_member
]
"#;

/// Query for namespace declarations (internal modules)
///
/// Matches:
/// - `namespace Foo {}`
/// - `module Bar {}`
/// - `export namespace Foo {}`
pub(crate) const NAMESPACE_QUERY: &str = r#"
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

/// Query for interface property signatures
///
/// Matches property signatures inside interface bodies:
/// - `id: number`
/// - `name?: string` (optional)
/// - `readonly createdAt: Date`
pub(crate) const INTERFACE_PROPERTY_QUERY: &str = r#"
(interface_body
  (property_signature
    name: (property_identifier) @name) @interface_property)
"#;

/// Query for interface method signatures
///
/// Matches method signatures inside interface bodies:
/// - `greet(): string`
/// - `updateEmail(email: string): void`
pub(crate) const INTERFACE_METHOD_QUERY: &str = r#"
(interface_body
  (method_signature
    name: (property_identifier) @name) @interface_method)
"#;
