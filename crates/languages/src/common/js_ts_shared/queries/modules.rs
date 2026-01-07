//! Module-related queries for JavaScript and TypeScript
//!
//! ES Modules use import/export statements for module management.
//! Each file is treated as its own module.

/// Query for the root program node (used for Module entity extraction)
/// JavaScript/TypeScript's tree-sitter uses "program" as the root node type
pub const MODULE_QUERY: &str = r#"
(program) @program
"#;
