//! Query definitions for entity extraction
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
              (visibility_modifier)? @visibility
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
                  (visibility_modifier)? @visibility
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
              (visibility_modifier)? @visibility
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
              (visibility_modifier)? @visibility
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
              (visibility_modifier)? @visibility
              name: (type_identifier) @name
            ) @trait
        "#,
    };

    /// Associated functions in inherent impl (no self parameter)
    pub const ASSOCIATED_FUNCTION_IN_INHERENT_IMPL: QueryDef = QueryDef {
        handler: "rust::associated_function_in_inherent_impl",
        entity_type: EntityType::Function,
        capture: "function",
        query: r#"
            ((impl_item
              type: [(type_identifier) (generic_type type: (type_identifier))] @impl_type
              body: (declaration_list
                (function_item
                  (visibility_modifier)? @visibility
                  name: (identifier) @name
                  parameters: (parameters) @params
                ) @function
              )
            ) @impl
            (#not-has-child? @impl trait)
            (#not-has-child? @params self_parameter))
        "#,
    };

    /// Module declarations
    pub const MODULE: QueryDef = QueryDef {
        handler: "rust::module_declaration",
        entity_type: EntityType::Module,
        capture: "module",
        query: r#"
            (mod_item
              (visibility_modifier)? @visibility
              name: (identifier) @name
            ) @module
        "#,
    };

    /// Struct fields (named fields in struct body)
    pub const STRUCT_FIELD: QueryDef = QueryDef {
        handler: "rust::struct_field",
        entity_type: EntityType::Property,
        capture: "field",
        query: r#"
            (struct_item
              name: (type_identifier) @struct_name
              body: (field_declaration_list
                (field_declaration
                  (visibility_modifier)? @visibility
                  name: (field_identifier) @name
                  type: (_) @field_type
                ) @field
              )
            )
        "#,
    };

    /// Enum variants
    pub const ENUM_VARIANT: QueryDef = QueryDef {
        handler: "rust::enum_variant",
        entity_type: EntityType::EnumVariant,
        capture: "variant",
        query: r#"
            (enum_item
              name: (type_identifier) @enum_name
              body: (enum_variant_list
                (enum_variant
                  name: (identifier) @name
                ) @variant
              )
            )
        "#,
    };

    /// Constants at module level
    pub const CONSTANT: QueryDef = QueryDef {
        handler: "rust::constant",
        entity_type: EntityType::Constant,
        capture: "const",
        query: r#"
            (const_item
              (visibility_modifier)? @visibility
              name: (identifier) @name
              type: (_) @const_type
            ) @const
        "#,
    };

    /// Statics at module level
    pub const STATIC: QueryDef = QueryDef {
        handler: "rust::static_item",
        entity_type: EntityType::Static,
        capture: "static",
        query: r#"
            (static_item
              (visibility_modifier)? @visibility
              name: (identifier) @name
              type: (_) @static_type
            ) @static
        "#,
    };

    /// Type aliases
    pub const TYPE_ALIAS: QueryDef = QueryDef {
        handler: "rust::type_alias",
        entity_type: EntityType::TypeAlias,
        capture: "type_alias",
        query: r#"
            (type_item
              (visibility_modifier)? @visibility
              name: (type_identifier) @name
              type: (_) @aliased_type
            ) @type_alias
        "#,
    };

    /// Union definitions
    pub const UNION: QueryDef = QueryDef {
        handler: "rust::union_definition",
        entity_type: EntityType::Union,
        capture: "union",
        query: r#"
            (union_item
              (visibility_modifier)? @visibility
              name: (type_identifier) @name
            ) @union
        "#,
    };

    /// Macro definitions (macro_rules!)
    pub const MACRO_DEFINITION: QueryDef = QueryDef {
        handler: "rust::macro_definition",
        entity_type: EntityType::Macro,
        capture: "macro",
        query: r#"
            (macro_definition
              name: (identifier) @name
            ) @macro
        "#,
    };

    /// Method signatures in trait definitions
    pub const METHOD_IN_TRAIT_DEF: QueryDef = QueryDef {
        handler: "rust::method_in_trait_def",
        entity_type: EntityType::Method,
        capture: "method",
        query: r#"
            (trait_item
              name: (type_identifier) @trait_name
              body: (declaration_list
                (function_signature_item
                  name: (identifier) @name
                ) @method
              )
            )
        "#,
    };

    /// All Rust query definitions
    pub const ALL: &[&QueryDef] = &[
        &FREE_FUNCTION,
        &INHERENT_IMPL,
        &TRAIT_IMPL,
        &METHOD_IN_INHERENT_IMPL,
        &METHOD_IN_TRAIT_IMPL,
        &ASSOCIATED_FUNCTION_IN_INHERENT_IMPL,
        &STRUCT,
        &ENUM,
        &TRAIT,
        &MODULE,
        &STRUCT_FIELD,
        &ENUM_VARIANT,
        &CONSTANT,
        &STATIC,
        &TYPE_ALIAS,
        &UNION,
        &MACRO_DEFINITION,
        &METHOD_IN_TRAIT_DEF,
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
        assert_eq!(rust::ALL.len(), 18);
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
