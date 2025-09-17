//! Shared constants for Rust AST handler modules
//!
//! This module defines all string constants used by the type and function handlers
//! for tree-sitter AST traversal and entity extraction.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

// ============================================================================
// Node Kind Constants
// ============================================================================

/// Node kind constants for tree-sitter AST traversal
#[allow(dead_code)]
pub(crate) mod node_kinds {
    // Type definitions
    pub const STRUCT: &str = "struct";
    pub const ENUM: &str = "enum";
    pub const TRAIT: &str = "trait";

    // Function related
    pub const FUNCTION_ITEM: &str = "function_item";
    pub const FUNCTION_SIGNATURE_ITEM: &str = "function_signature_item";
    pub const PARAMETER: &str = "parameter";
    pub const SELF_PARAMETER: &str = "self_parameter";
    pub const VARIADIC_PARAMETER: &str = "variadic_parameter";

    // Attributes and modifiers
    pub const ATTRIBUTE_ITEM: &str = "attribute_item";
    pub const ATTRIBUTE: &str = "attribute";
    pub const VISIBILITY_MODIFIER: &str = "visibility_modifier";
    pub const MUTABLE_SPECIFIER: &str = "mutable_specifier";
    pub const MUT_PATTERN: &str = "mut_pattern";

    // Identifiers
    pub const IDENTIFIER: &str = "identifier";
    pub const TYPE_IDENTIFIER: &str = "type_identifier";
    pub const FIELD_IDENTIFIER: &str = "field_identifier";
    pub const SCOPED_IDENTIFIER: &str = "scoped_identifier";
    pub const SCOPED_TYPE_IDENTIFIER: &str = "scoped_type_identifier";

    // Field and variant related
    pub const FIELD_DECLARATION: &str = "field_declaration";
    pub const FIELD_DECLARATION_LIST: &str = "field_declaration_list";
    pub const ORDERED_FIELD_DECLARATION_LIST: &str = "ordered_field_declaration_list";
    pub const ENUM_VARIANT: &str = "enum_variant";

    // Type parameters and generics
    pub const TYPE_PARAMETER: &str = "type_parameter";
    pub const LIFETIME_PARAMETER: &str = "lifetime_parameter";
    pub const CONST_PARAMETER: &str = "const_parameter";
    pub const LIFETIME: &str = "lifetime";
    pub const CONSTRAINED_TYPE_PARAMETER: &str = "constrained_type_parameter";
    pub const OPTIONAL_TYPE_PARAMETER: &str = "optional_type_parameter";

    // Trait members
    pub const ASSOCIATED_TYPE: &str = "associated_type";

    // Meta and token trees
    pub const TOKEN_TREE: &str = "token_tree";
    pub const META_ARGUMENTS: &str = "meta_arguments";
    pub const META_ITEM: &str = "meta_item";

    // Comments
    pub const LINE_COMMENT: &str = "line_comment";
    pub const BLOCK_COMMENT: &str = "block_comment";
}

// ============================================================================
// Capture Name Constants
// ============================================================================

/// Capture name constants for tree-sitter queries
#[allow(dead_code)]
pub(crate) mod capture_names {
    pub const NAME: &str = "name";
    pub const STRUCT: &str = "struct";
    pub const ENUM: &str = "enum";
    pub const TRAIT: &str = "trait";
    pub const FUNCTION: &str = "function";
    pub const VIS: &str = "vis";
    pub const GENERICS: &str = "generics";
    pub const FIELDS: &str = "fields";
    pub const ENUM_BODY: &str = "enum_body";
    pub const BOUNDS: &str = "bounds";
    pub const TRAIT_BODY: &str = "trait_body";
    pub const PARAMS: &str = "params";
    pub const RETURN: &str = "return";
    pub const MODIFIERS: &str = "modifiers";
}

// ============================================================================
// Keywords
// ============================================================================

/// Visibility keywords
#[allow(dead_code)]
pub(crate) mod visibility_keywords {
    pub const PUB: &str = "pub";
    pub const CRATE: &str = "crate";
    pub const SUPER: &str = "super";
    pub const SELF: &str = "self";
    pub const IN: &str = "in";
}

/// Function modifier keywords
#[allow(dead_code)]
pub(crate) mod function_modifiers {
    pub const ASYNC: &str = "async";
    pub const UNSAFE: &str = "unsafe";
    pub const CONST: &str = "const";
}

/// Other language keywords
#[allow(dead_code)]
pub(crate) mod keywords {
    pub const FN: &str = "fn";
    pub const SELF: &str = "self";
}

// ============================================================================
// Special Identifiers
// ============================================================================

/// Special identifiers and names
#[allow(dead_code)]
pub(crate) mod special_idents {
    pub const DERIVE: &str = "derive";
    pub const ANONYMOUS: &str = "anonymous";
    pub const VARIADIC: &str = "...";
}

// ============================================================================
// Documentation Comment Prefixes
// ============================================================================

/// Documentation comment prefixes
#[allow(dead_code)]
pub(crate) mod doc_prefixes {
    pub const LINE_OUTER: &str = "///";
    pub const LINE_INNER: &str = "//!";
    pub const BLOCK_OUTER_START: &str = "/**";
    pub const BLOCK_INNER_START: &str = "/*!";
    pub const BLOCK_END: &str = "*/";
}

// ============================================================================
// Punctuation Tokens
// ============================================================================

/// Punctuation tokens
#[allow(dead_code)]
pub(crate) mod punctuation {
    pub const OPEN_PAREN: &str = "(";
    pub const CLOSE_PAREN: &str = ")";
    pub const OPEN_BRACKET: &str = "[";
    pub const CLOSE_BRACKET: &str = "]";
    pub const OPEN_ANGLE: &str = "<";
    pub const CLOSE_ANGLE: &str = ">";
    pub const COMMA: &str = ",";
    pub const COLON: &str = ":";
    pub const EQUALS: &str = "=";
    pub const PLUS: &str = "+";
}
