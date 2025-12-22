//! Tree-sitter queries for Python entity extraction

/// Query for function definitions (module-level and nested)
pub const FUNCTION_QUERY: &str = r#"
(function_definition
  name: (identifier) @name
  parameters: (parameters) @params
  return_type: (type)? @return_type
  body: (block) @body
) @function
"#;

/// Query for class definitions
pub const CLASS_QUERY: &str = r#"
(class_definition
  name: (identifier) @name
  superclasses: (argument_list)? @bases
  body: (block) @class_body
) @class
"#;

/// Query for method definitions (functions inside class body)
/// Matches both plain and decorated methods
pub const METHOD_QUERY: &str = r#"
(class_definition
  body: (block
    [
      (function_definition
        name: (identifier) @name
        parameters: (parameters) @params
        return_type: (type)? @return_type
        body: (block) @body
      ) @method
      (decorated_definition
        (function_definition
          name: (identifier) @name
          parameters: (parameters) @params
          return_type: (type)? @return_type
          body: (block) @body
        ) @method
      )
    ]
  )
) @class
"#;

/// Query for the root module node (used for Module entity extraction)
/// Python's tree-sitter uses "module" as the root node type
pub const MODULE_QUERY: &str = r#"
(module) @module
"#;
