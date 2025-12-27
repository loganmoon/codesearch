//! Tree-sitter query definitions for Rust pattern matching
//!
//! This module contains query patterns for extracting various
//! Rust language constructs using tree-sitter's query system.

/// Query for extracting function definitions
pub const FUNCTION_QUERY: &str = r#"
(function_item
  (visibility_modifier)? @vis
  (function_modifiers)? @modifiers
  name: (identifier) @name
  type_parameters: (type_parameters)? @generics
  parameters: (parameters) @params
  return_type: (_)? @return
  (where_clause)? @where
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
  (where_clause)? @where
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
  (where_clause)? @where
  body: (enum_variant_list) @enum_body
) @enum
"#;

/// Query for extracting impl blocks (inherent impls)
pub const IMPL_QUERY: &str = r#"
(impl_item
  type_parameters: (type_parameters)? @generics
  type: (_) @type
  (where_clause)? @where
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
  (where_clause)? @where
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
) @macro
"#;

/// Query for extracting the crate root module
///
/// This matches the entire source_file node, which exists once per file.
/// The handler uses file path detection to only create an entity when
/// processing lib.rs or main.rs (crate root files).
pub const CRATE_ROOT_QUERY: &str = r#"
(source_file) @crate_root
"#;
