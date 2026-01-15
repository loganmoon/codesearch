//! Query definitions for entity extraction (V2 Architecture)
//!
//! This module defines tree-sitter queries as Rust constants, providing
//! type-safe query definitions with associated metadata.

use codesearch_core::entities::EntityType;

/// Definition of a tree-sitter query with associated handler metadata
#[derive(Debug, Clone, Copy)]
pub struct QueryDef {
    /// Handler name (e.g., "rust::free_function")
    pub handler: &'static str,
    /// Entity type this query produces
    pub entity_type: EntityType,
    /// Primary capture name in the query
    pub capture: &'static str,
    /// The tree-sitter query string
    pub query: &'static str,
}

/// Rust entity extraction queries
pub mod rust {
    use super::*;

    /// Free functions at module level (not inside impl blocks)
    pub const FREE_FUNCTION: QueryDef = QueryDef {
        handler: "rust::free_function",
        entity_type: EntityType::Function,
        capture: "func",
        query: r#"
            ((function_item
              name: (identifier) @name
            ) @func
            (#not-has-ancestor? @func impl_item))
        "#,
    };

    /// Inherent impl blocks (no trait)
    pub const INHERENT_IMPL: QueryDef = QueryDef {
        handler: "rust::inherent_impl",
        entity_type: EntityType::Impl,
        capture: "impl",
        query: r#"
            ((impl_item
              type: (type_identifier) @impl_type
              body: (declaration_list) @body
            ) @impl
            (#not-has-child? @impl trait))
        "#,
    };

    /// Trait impl blocks (impl Trait for Type)
    pub const TRAIT_IMPL: QueryDef = QueryDef {
        handler: "rust::trait_impl",
        entity_type: EntityType::Impl,
        capture: "impl",
        query: r#"
            (impl_item
              trait: (type_identifier) @trait_name
              type: (type_identifier) @impl_type
              body: (declaration_list) @body
            ) @impl
        "#,
    };

    /// Methods with self parameter in inherent impl blocks
    pub const METHOD_IN_INHERENT_IMPL: QueryDef = QueryDef {
        handler: "rust::method_in_inherent_impl",
        entity_type: EntityType::Method,
        capture: "method",
        query: r#"
            ((impl_item
              type: (type_identifier) @impl_type
              body: (declaration_list
                (function_item
                  name: (identifier) @name
                  parameters: (parameters
                    . (self_parameter) @self_param
                  )
                ) @method
              )
            ) @impl
            (#not-has-child? @impl trait))
        "#,
    };

    /// Methods in trait impl blocks
    pub const METHOD_IN_TRAIT_IMPL: QueryDef = QueryDef {
        handler: "rust::method_in_trait_impl",
        entity_type: EntityType::Method,
        capture: "method",
        query: r#"
            (impl_item
              trait: (type_identifier) @trait_name
              type: (type_identifier) @impl_type
              body: (declaration_list
                (function_item
                  name: (identifier) @name
                  parameters: (parameters) @params
                ) @method
              )
            ) @impl
        "#,
    };

    /// Struct definitions
    pub const STRUCT: QueryDef = QueryDef {
        handler: "rust::struct_definition",
        entity_type: EntityType::Struct,
        capture: "struct",
        query: r#"
            (struct_item
              name: (type_identifier) @name
            ) @struct
        "#,
    };

    /// Enum definitions
    pub const ENUM: QueryDef = QueryDef {
        handler: "rust::enum_definition",
        entity_type: EntityType::Enum,
        capture: "enum",
        query: r#"
            (enum_item
              name: (type_identifier) @name
            ) @enum
        "#,
    };

    /// Trait definitions
    pub const TRAIT: QueryDef = QueryDef {
        handler: "rust::trait_definition",
        entity_type: EntityType::Trait,
        capture: "trait",
        query: r#"
            (trait_item
              name: (type_identifier) @name
            ) @trait
        "#,
    };

    /// All Rust query definitions
    pub const ALL: &[&QueryDef] = &[
        &FREE_FUNCTION,
        &INHERENT_IMPL,
        &TRAIT_IMPL,
        &METHOD_IN_INHERENT_IMPL,
        &METHOD_IN_TRAIT_IMPL,
        &STRUCT,
        &ENUM,
        &TRAIT,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_def_structure() {
        assert_eq!(rust::FREE_FUNCTION.handler, "rust::free_function");
        assert_eq!(rust::FREE_FUNCTION.entity_type, EntityType::Function);
        assert_eq!(rust::FREE_FUNCTION.capture, "func");
        assert!(!rust::FREE_FUNCTION.query.is_empty());
    }

    #[test]
    fn test_all_queries_list() {
        assert_eq!(rust::ALL.len(), 8);
        for query in rust::ALL {
            assert!(!query.handler.is_empty());
            assert!(!query.capture.is_empty());
            assert!(!query.query.is_empty());
        }
    }

    #[test]
    fn test_queries_are_valid_tree_sitter() {
        // Verify queries can be parsed by tree-sitter
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();

        for query_def in rust::ALL {
            let result = tree_sitter::Query::new(&language, query_def.query);
            assert!(
                result.is_ok(),
                "Query '{}' failed to parse: {:?}",
                query_def.handler,
                result.err()
            );
        }
    }
}
