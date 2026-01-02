//! Tree-sitter queries for TypeScript entity extraction

// Reuse JavaScript query for arrow functions (similar syntax)
pub use crate::javascript::queries::ARROW_FUNCTION_QUERY;

/// Query for regular function declarations (TypeScript-specific to handle type annotations)
pub const FUNCTION_QUERY: &str = r#"
(function_declaration
  name: (identifier) @name
  parameters: (formal_parameters) @params
) @function
"#;

/// Query for class declarations (TypeScript-specific)
pub const CLASS_QUERY: &str = r#"
(class_declaration
  name: (type_identifier) @name
  body: (class_body) @class_body
) @class
"#;

/// Query for class methods (TypeScript-specific)
pub const METHOD_QUERY: &str = r#"
(method_definition
  name: (property_identifier) @name
  parameters: (formal_parameters) @params
) @method
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
