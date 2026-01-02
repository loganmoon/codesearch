//! Tree-sitter queries for TypeScript entity extraction

// Reuse JavaScript query for arrow functions (similar syntax)
pub use crate::javascript::queries::ARROW_FUNCTION_QUERY;

/// Query for regular function declarations (TypeScript-specific to handle type annotations)
/// Also matches generator function declarations (function*) and ambient function declarations (declare function)
pub const FUNCTION_QUERY: &str = r#"
[
  (function_declaration
    name: (identifier) @name
    parameters: (formal_parameters) @params
  ) @function

  (generator_function_declaration
    name: (identifier) @name
    parameters: (formal_parameters) @params
  ) @function

  (ambient_declaration
    (function_signature
      name: (identifier) @name
      parameters: (formal_parameters) @params
    ) @function
  )
]
"#;

/// Query for function expressions (named and anonymous)
/// Used to extract functions from: `const fn = function name() {}` or `const fn = function() {}`
/// Matches function expressions directly (the handler traverses up to find the variable name if anonymous)
/// NOTE: Currently disabled - causes timeout issues
#[allow(dead_code)]
pub const FUNCTION_EXPRESSION_QUERY: &str = r#"
(function_expression
  name: (identifier)? @func_name
  parameters: (formal_parameters) @params
) @func_expr
"#;

/// Query for class declarations (TypeScript-specific)
/// Captures class_heritage for extends/implements clause handling
/// Matches both regular and abstract class declarations
pub const CLASS_QUERY: &str = r#"
[
  (class_declaration
    name: (type_identifier) @name
    (class_heritage)? @extends
    body: (class_body) @class_body
  ) @class

  (abstract_class_declaration
    name: (type_identifier) @name
    (class_heritage)? @extends
    body: (class_body) @class_body
  ) @class
]
"#;

/// Query for class methods (TypeScript-specific)
/// Matches both regular method definitions and abstract method signatures
pub const METHOD_QUERY: &str = r#"
[
  (method_definition
    name: (property_identifier) @name
    parameters: (formal_parameters) @params
  ) @method

  (abstract_method_signature
    name: (property_identifier) @name
    parameters: (formal_parameters) @params
  ) @method
]
"#;

/// Query for interface declarations
pub const INTERFACE_QUERY: &str = r#"
(interface_declaration
  name: (type_identifier) @name
) @interface
"#;

/// Query for type aliases
pub const TYPE_ALIAS_QUERY: &str = r#"
(type_alias_declaration
  name: (type_identifier) @name
) @type_alias
"#;

/// Query for enums
pub const ENUM_QUERY: &str = r#"
(enum_declaration
  name: (identifier) @name
) @enum
"#;

/// Query for the root program node (used for Module entity extraction)
pub const MODULE_QUERY: &str = r#"
(program) @module
"#;

/// Query for namespace declarations (TypeScript namespaces produce Module entities)
/// Matches both `namespace` and `module` keywords
/// NOTE: Currently disabled - causes timeout issues
#[allow(dead_code)]
pub const NAMESPACE_QUERY: &str = r#"
[
  (namespace_declaration
    name: (identifier) @name
    body: (statement_block) @body
  ) @namespace

  (internal_module
    name: (identifier) @name
    body: (statement_block) @body
  ) @namespace
]
"#;

/// Query for variable declarations (const, let, var)
/// Matches top-level lexical and variable declarations
/// Includes both simple identifiers and destructuring patterns
pub const VARIABLE_QUERY: &str = r#"
[
  ; Simple identifier declarations: const x = 1
  (lexical_declaration
    (variable_declarator
      name: (identifier) @name
      value: (_)? @value
    )
  ) @declaration

  (variable_declaration
    (variable_declarator
      name: (identifier) @name
      value: (_)? @value
    )
  ) @declaration

  ; Object destructuring: const { a, b } = obj
  (lexical_declaration
    (variable_declarator
      name: (object_pattern) @destructure_pattern
      value: (_)? @value
    )
  ) @declaration

  (variable_declaration
    (variable_declarator
      name: (object_pattern) @destructure_pattern
      value: (_)? @value
    )
  ) @declaration
]
"#;

/// Query for class field/property declarations
/// Matches public_field_definition nodes inside class bodies
pub const FIELD_QUERY: &str = r#"
(public_field_definition
  name: (property_identifier) @name
) @field
"#;

/// Query for private class fields (#private syntax)
/// Matches private fields like `#count: number`
pub const PRIVATE_FIELD_QUERY: &str = r#"
(public_field_definition
  name: (private_property_identifier) @name
) @field
"#;

/// Query for interface property signatures
/// Matches properties like `id: number` in interfaces
pub const INTERFACE_PROPERTY_QUERY: &str = r#"
(property_signature
  name: (property_identifier) @name
) @property
"#;

/// Query for interface method signatures
/// Matches methods like `greet(): string` in interfaces
pub const INTERFACE_METHOD_QUERY: &str = r#"
(method_signature
  name: (property_identifier) @name
) @method
"#;

/// Query for interface call signatures
/// Matches callable interfaces like `(x: number): number` in `interface Callable { (x: number): number; }`
pub const CALL_SIGNATURE_QUERY: &str = r#"
(call_signature
  parameters: (formal_parameters) @params
) @call_sig
"#;

/// Query for interface construct signatures
/// Matches newable interfaces like `new (name: string): object` in `interface Constructable { new (name: string): object; }`
pub const CONSTRUCT_SIGNATURE_QUERY: &str = r#"
(construct_signature
  parameters: (formal_parameters) @params
) @construct_sig
"#;

/// Query for interface index signatures
/// Matches indexer patterns like `[key: string]: value` in interfaces
pub const INDEX_SIGNATURE_QUERY: &str = r#"
(index_signature) @index_sig
"#;

/// Query for class expressions (named and anonymous)
/// Used to extract classes from: `const C = class Name {}` or `const C = class {}`
/// Matches class expressions directly (the handler traverses up to find the variable name if anonymous)
pub const CLASS_EXPRESSION_QUERY: &str = r#"
(class
  name: (type_identifier)? @class_name
  body: (class_body) @class_body
) @class_expr
"#;

/// Query for constructor parameter properties
/// Matches constructor methods with parameters to extract parameter properties
/// like `constructor(public x: number, private y: string)`
pub const PARAMETER_PROPERTY_QUERY: &str = r#"
(method_definition
  name: (property_identifier) @method_name
  parameters: (formal_parameters) @params
  (#eq? @method_name "constructor")
) @constructor
"#;
