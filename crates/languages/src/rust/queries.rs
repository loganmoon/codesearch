//! Tree-sitter query definitions for Rust pattern matching
//!
//! This module contains query patterns for extracting various
//! Rust language constructs using tree-sitter's query system.

#![allow(dead_code)] // These will be used in Phase 2 implementation

/// Query for extracting function definitions
pub const FUNCTION_QUERY: &str = r#"
(function_item
  (visibility_modifier)? @vis
  (function_modifiers)? @modifiers
  name: (identifier) @name
  type_parameters: (type_parameters)? @generics
  parameters: (parameters) @params
  return_type: (_)? @return
  body: (block) @body
) @function
"#;

/// Query for extracting struct definitions
pub const STRUCT_QUERY: &str = r#"
(struct_item
  (visibility_modifier)? @vis
  "struct"
  name: (type_identifier) @name
  type_parameters: (type_parameters)? @generics
  (where_clause)? @where
  body: [
    (field_declaration_list) @fields
    (ordered_field_declaration_list) @fields
  ]?
) @struct
"#;

/// Query for extracting trait definitions
pub const TRAIT_QUERY: &str = r#"
(trait_item
  (visibility_modifier)? @vis
  "unsafe"? @unsafe
  "trait"
  name: (type_identifier) @name
  type_parameters: (type_parameters)? @generics
  bounds: (trait_bounds)? @bounds
  body: (declaration_list) @trait_body
) @trait
"#;

/// Query for extracting enum definitions
pub const ENUM_QUERY: &str = r#"
(enum_item
  (visibility_modifier)? @vis
  "enum"
  name: (type_identifier) @name
  type_parameters: (type_parameters)? @generics
  body: (enum_variant_list) @enum_body
) @enum
"#;

/// Query for extracting impl blocks (inherent impls)
pub const IMPL_QUERY: &str = r#"
(impl_item
  type_parameters: (type_parameters)? @generics
  type: (_) @type
  body: (declaration_list) @impl_body
) @impl
"#;

/// Query for extracting trait implementation blocks
pub const IMPL_TRAIT_QUERY: &str = r#"
(impl_item
  type_parameters: (type_parameters)? @generics
  trait: (_) @trait
  "for"
  type: (_) @type
  body: (declaration_list) @impl_body
) @impl_trait
"#;

/// Query for extracting module definitions
pub const MODULE_QUERY: &str = r#"
(mod_item
  (visibility_modifier)? @vis
  "mod"
  name: (identifier) @name
  body: (declaration_list)? @mod_body
) @module
"#;

/// Query for extracting constant and static items
pub const CONSTANT_QUERY: &str = r#"
[
  (const_item
    (visibility_modifier)? @vis
    "const" @const
    name: (identifier) @name
    type: (_) @type
    value: (_) @value
  ) @constant
  (static_item
    (visibility_modifier)? @vis
    "static" @static
    (mutable_specifier)? @mut
    name: (identifier) @name
    type: (_) @type
    value: (_) @value
  ) @constant
]
"#;

/// Query for extracting type aliases
pub const TYPE_ALIAS_QUERY: &str = r#"
(type_item
  (visibility_modifier)? @vis
  "type"
  name: (type_identifier) @name
  type_parameters: (type_parameters)? @generics
  "="
  type: (_) @type
) @type_alias
"#;

/// Query for extracting macro definitions
pub const MACRO_QUERY: &str = r#"
(macro_definition
  name: (identifier) @name
  (macro_rule
    left: (token_tree_pattern)? @pattern
    right: (token_tree)? @expansion
  )? @rule
) @macro
"#;

/// Query for extracting function calls
pub const CALL_QUERY: &str = r#"
(call_expression
  function: [
    (identifier) @func_name
    (field_expression 
      field: (field_identifier) @method_name)
    (scoped_identifier 
      path: (_)? @path
      name: (identifier) @scoped_name)
  ]
  arguments: (arguments) @args
) @call
"#;

/// Query for extracting use declarations
pub const USE_QUERY: &str = r#"
(use_declaration
  (visibility_modifier)? @vis
  "use"
  [
    (use_wildcard
      (scoped_identifier) @path
    ) @wildcard
    (use_list) @list
    (use_as_clause
      (scoped_identifier) @path
      (identifier) @alias
    ) @as_clause
    (scoped_identifier) @simple_path
  ]
) @use
"#;

/// Query for extracting trait bounds and where clauses
pub const WHERE_CLAUSE_QUERY: &str = r#"
(where_clause
  "where"
  (where_predicate
    left: (type_identifier) @type
    bounds: (trait_bounds
      (type_identifier) @bound
      ("+" (type_identifier))* @additional_bounds
    )
  )* @predicates
) @where
"#;

/// Query for extracting generic parameters
pub const GENERICS_QUERY: &str = r#"
(type_parameters
  "<"
  (
    (type_parameter
      name: (type_identifier) @param_name
      bounds: (trait_bounds)? @param_bounds
    ) |
    (lifetime
      "'" @lifetime_tick
      (identifier) @lifetime_name
    ) |
    (const_parameter
      "const"
      name: (identifier) @const_name
      type: (_) @const_type
    )
  )* @params
  ">"
) @generics
"#;

/// Query for extracting method calls
pub const METHOD_CALL_QUERY: &str = r#"
(call_expression
  function: (field_expression
    value: (_) @receiver
    field: (field_identifier) @method
  )
  arguments: (arguments) @args
) @method_call
"#;

/// Query for extracting field access
pub const FIELD_ACCESS_QUERY: &str = r#"
(field_expression
  value: (_) @object
  "."
  field: (field_identifier) @field
) @field_access
"#;

/// Query for extracting match expressions (useful for finding enum usage)
pub const MATCH_QUERY: &str = r#"
(match_expression
  value: (_) @matched_value
  body: (match_block
    (match_arm
      pattern: (_) @pattern
      value: (_) @arm_value
    )* @arms
  )
) @match
"#;

/// Query for extracting derive attributes  
pub const DERIVE_QUERY: &str = r##"
(attribute_item
  (attribute
    (identifier) @attr_name
    (token_tree
      (identifier) @derive_trait
    )?
  )
) @attribute
"##;

/// Query for extracting doc comments
pub const DOC_COMMENT_QUERY: &str = r#"
[
  (line_comment) @doc_line
  (block_comment) @doc_block
]
"#;

/// Query for extracting async blocks
pub const ASYNC_BLOCK_QUERY: &str = r#"
(async_block
  "async"
  "move"? @move
  (block) @body
) @async_block
"#;

/// Query for extracting closure expressions
pub const CLOSURE_QUERY: &str = r#"
(closure_expression
  "move"? @move
  parameters: (closure_parameters)? @params
  return_type: (_)? @return_type
  body: (_) @body
) @closure
"#;

/// Query for extracting unsafe blocks
pub const UNSAFE_BLOCK_QUERY: &str = r#"
(unsafe_block
  "unsafe"
  (block) @body
) @unsafe_block
"#;

/// Query for extracting extern blocks (FFI)
pub const EXTERN_BLOCK_QUERY: &str = r#"
(foreign_mod_item
  "extern"
  (string_literal)? @abi
  body: (declaration_list
    (function_signature_item)* @extern_functions
  )
) @extern_block
"#;

/// Query for extracting macro invocations
pub const MACRO_INVOCATION_QUERY: &str = r#"
(macro_invocation
  macro: [
    (identifier) @macro_name
    (scoped_identifier
      path: (_)? @macro_path
      name: (identifier) @macro_name
    )
  ]
  "!"
  (token_tree) @macro_args
) @macro_call
"#;

/// Query for extracting attribute macros
pub const ATTRIBUTE_MACRO_QUERY: &str = r##"
(attribute_item
  (attribute
    [
      (identifier) @attr_name
      (scoped_identifier
        path: (_)? @attr_path
        name: (identifier) @attr_name
      )
    ]
    (token_tree)? @attr_args
  )
) @attribute_macro
"##;

/// Query for extracting tests
pub const TEST_QUERY: &str = r##"
(
  (attribute_item
    "#"
    "["
    (meta_item
      name: (identifier) @attr
    )
    "]"
  ) @test_attr
  .
  (function_item
    name: (identifier) @test_name
  ) @test_fn
  (#eq? @attr "test")
)
"##;

/// Query for extracting benchmark functions
pub const BENCH_QUERY: &str = r##"
(
  (attribute_item
    "#"
    "["
    (meta_item
      name: (identifier) @attr
    )
    "]"
  ) @bench_attr
  .
  (function_item
    name: (identifier) @bench_name
  ) @bench_fn
  (#eq? @attr "bench")
)
"##;
