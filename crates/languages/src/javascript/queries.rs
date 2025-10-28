//! Tree-sitter queries for JavaScript entity extraction

/// Query for regular function declarations
pub const FUNCTION_QUERY: &str = r#"
(function_declaration
  name: (identifier) @name
  parameters: (formal_parameters) @params
  body: (statement_block) @body
) @function
"#;

/// Query for arrow functions assigned to variables
pub const ARROW_FUNCTION_QUERY: &str = r#"
(arrow_function) @arrow_function
"#;

/// Query for class declarations
pub const CLASS_QUERY: &str = r#"
(class_declaration
  name: (identifier) @name
  (class_heritage)? @extends
  body: (class_body) @class_body
) @class
"#;

/// Query for class methods
pub const METHOD_QUERY: &str = r#"
(method_definition
  name: (property_identifier) @name
  parameters: (formal_parameters) @params
  body: (statement_block) @body
) @method
"#;
