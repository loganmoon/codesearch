//! Class-related queries for JavaScript and TypeScript

/// Query for class declarations
///
/// Matches:
/// - `class Foo {}`
/// - `class Foo extends Bar {}`
/// - `export class Foo {}`
pub const CLASS_DECLARATION_QUERY: &str = r#"
[
  (class_declaration
    name: (identifier) @name
    (class_heritage
      (extends_clause
        value: (_) @extends))?
    body: (class_body) @body) @class

  (export_statement
    declaration: (class_declaration
      name: (identifier) @name
      (class_heritage
        (extends_clause
          value: (_) @extends))?
      body: (class_body) @body)) @class
]
"#;

/// Query for class expressions assigned to variables
///
/// Matches:
/// - `const Foo = class {}`
/// - `const Foo = class Bar {}`
/// - `let Foo = class extends Base {}`
pub const CLASS_EXPRESSION_QUERY: &str = r#"
(lexical_declaration
  (variable_declarator
    name: (identifier) @name
    value: (class
      name: (identifier)? @class_name
      (class_heritage
        (extends_clause
          value: (_) @extends))?
      body: (class_body) @body))) @class

(variable_declaration
  (variable_declarator
    name: (identifier) @name
    value: (class
      name: (identifier)? @class_name
      (class_heritage
        (extends_clause
          value: (_) @extends))?
      body: (class_body) @body))) @class
"#;

/// Query for default exported classes
///
/// Matches:
/// - `export default class {}`
/// - `export default class Foo {}`
pub const DEFAULT_EXPORT_CLASS_QUERY: &str = r#"
(export_statement
  (class_declaration
    name: (identifier)? @name
    (class_heritage
      (extends_clause
        value: (_) @extends))?
    body: (class_body) @body) @value
  "default" @default) @class
"#;

/// Query for class methods
///
/// Matches:
/// - `method() {}`
/// - `static method() {}`
/// - `async method() {}`
/// - `*generatorMethod() {}`
/// - `get prop() {}`
/// - `set prop(v) {}`
/// - `#privateMethod() {}`
pub const METHOD_QUERY: &str = r#"
(class_body
  (method_definition
    name: [
      (property_identifier) @name
      (private_property_identifier) @name
    ]
    parameters: (formal_parameters) @params
    body: (statement_block) @body)) @method
"#;

/// Query for class fields/properties
///
/// Matches:
/// - `field = value`
/// - `static field = value`
/// - `#privateField = value`
/// - `field` (no initializer)
pub const PROPERTY_QUERY: &str = r#"
(class_body
  (field_definition
    property: [
      (property_identifier) @name
      (private_property_identifier) @name
    ]
    value: (_)? @value)) @property

(class_body
  (public_field_definition
    name: [
      (property_identifier) @name
      (private_property_identifier) @name
    ]
    value: (_)? @value)) @property
"#;

/// Query for static blocks (ES2022)
///
/// Matches:
/// - `static { ... }`
pub const STATIC_BLOCK_QUERY: &str = r#"
(class_body
  (static_block) @static_block) @method
"#;
