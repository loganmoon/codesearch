//! Handler registry for entity extraction
//!
//! This module provides the `HandlerRegistration` type which is used to register
//! entity handlers via the `inventory` crate. Handlers are registered at compile time
//! and discovered at runtime.

use crate::extract_context::ExtractContext;
use codesearch_core::entities::EntityType;
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

/// Type signature for handler functions
///
/// Handler functions receive an `ExtractContext` containing all the data needed
/// to extract an entity, and return either:
/// - `Ok(Some(entity))` - Successfully extracted an entity
/// - `Ok(None)` - Match should be skipped (e.g., semantic analysis determined invalid)
/// - `Err(...)` - Extraction failed with an error
pub type HandlerFn = for<'a> fn(&ExtractContext<'a>) -> Result<Option<CodeEntity>>;

/// Registration entry for an entity handler
///
/// This struct is submitted to `inventory` by the `#[entity_handler]` proc macro
/// to register handlers at compile time.
///
/// # Example
///
/// ```ignore
/// inventory::submit! {
///     HandlerRegistration {
///         name: "rust::free_function",
///         language: "rust",
///         entity_type: EntityType::Function,
///         primary_capture: "func",
///         handler: free_function_handler,
///     }
/// }
/// ```
pub struct HandlerRegistration {
    /// Handler name for debugging and dispatch (e.g., "rust::free_function")
    pub name: &'static str,

    /// Language this handler applies to (e.g., "rust", "javascript", "typescript")
    pub language: &'static str,

    /// Entity type produced by this handler
    pub entity_type: EntityType,

    /// Primary capture name from the associated query
    ///
    /// This is the capture that identifies the main AST node being extracted.
    pub primary_capture: &'static str,

    /// The handler function
    pub handler: HandlerFn,
}

// Register the collection for inventory
inventory::collect!(HandlerRegistration);

/// Find a handler registration by name
pub fn find_handler(name: &str) -> Option<&'static HandlerRegistration> {
    inventory::iter::<HandlerRegistration>().find(|h| h.name == name)
}

/// Get all registered handlers for a language
pub fn handlers_for_language(language: &str) -> Vec<&'static HandlerRegistration> {
    inventory::iter::<HandlerRegistration>()
        .filter(|h| h.language == language)
        .collect()
}

/// Get all registered handlers
pub fn all_handlers() -> impl Iterator<Item = &'static HandlerRegistration> {
    inventory::iter::<HandlerRegistration>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::import_map::ImportMap;
    use codesearch_core::entities::Language;
    use std::path::Path;

    // Test handler function
    fn test_handler(ctx: &ExtractContext) -> Result<Option<CodeEntity>> {
        // Simple test implementation - just return None
        let _ = ctx.node(); // Use the context to avoid unused warning
        Ok(None)
    }

    // Submit a test handler for testing
    inventory::submit! {
        HandlerRegistration {
            name: "test::test_handler",
            language: "rust",
            entity_type: EntityType::Function,
            primary_capture: "func",
            handler: test_handler,
        }
    }

    #[test]
    fn test_find_handler() {
        let handler = find_handler("test::test_handler");
        assert!(handler.is_some());
        let h = handler.unwrap();
        assert_eq!(h.name, "test::test_handler");
        assert_eq!(h.language, "rust");
        assert_eq!(h.entity_type, EntityType::Function);
        assert_eq!(h.primary_capture, "func");
    }

    #[test]
    fn test_find_handler_not_found() {
        let handler = find_handler("nonexistent::handler");
        assert!(handler.is_none());
    }

    #[test]
    fn test_handlers_for_language() {
        let rust_handlers = handlers_for_language("rust");
        // At minimum, our test handler should be found
        assert!(rust_handlers.iter().any(|h| h.name == "test::test_handler"));
    }

    #[test]
    fn test_handler_invocation() {
        let source = "fn test() {}";
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let import_map = ImportMap::default();

        let ctx = ExtractContext::builder()
            .node(tree.root_node())
            .source(source)
            .file_path(Path::new("/test/file.rs"))
            .import_map(&import_map)
            .language(Language::Rust)
            .language_str("rust")
            .repository_id("test-repo")
            .repo_root(Path::new("/test"))
            .build()
            .unwrap();

        let handler = find_handler("test::test_handler").unwrap();
        let result = (handler.handler)(&ctx);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    // Test using the entity_handler macro
    mod macro_tests {
        use super::*;
        use crate::entity_handler;
        use crate::extract_context::{CaptureData, ExtractContext};
        // These imports are required by the macro-generated code
        use crate::handler_registry::HandlerRegistration;
        use codesearch_core::entities::{EntityType, Language};
        use codesearch_core::error::Result;
        use codesearch_core::CodeEntity;
        use tree_sitter::Node;

        // Handler using the macro with capture injection
        #[entity_handler(entity_type = Function, capture = "func", language = "rust")]
        fn macro_test_handler(
            #[capture] name: &str,
            #[capture] params: Option<Node>,
            ctx: &ExtractContext,
        ) -> Result<Option<CodeEntity>> {
            // Verify captures were extracted correctly
            let _ = ctx.node(); // Access ctx to confirm it's passed
            let _ = name; // Use the captured name
            let _ = params; // Use the optional params
            Ok(None)
        }

        #[test]
        fn test_macro_handler_registration() {
            // The handler should be registered via inventory
            let handler = find_handler("rust::macro_test_handler");
            assert!(handler.is_some(), "macro handler should be registered");

            let h = handler.unwrap();
            assert_eq!(h.name, "rust::macro_test_handler");
            assert_eq!(h.language, "rust");
            assert_eq!(h.entity_type, EntityType::Function);
            assert_eq!(h.primary_capture, "func");
        }

        #[test]
        fn test_macro_handler_invocation() {
            let source = "fn my_func(x: i32) {}";
            let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&language).unwrap();
            let tree = parser.parse(source, None).unwrap();
            let import_map = ImportMap::default();

            // Get the function node
            let func_node = tree.root_node().child(0).unwrap();
            let name_node = func_node.child_by_field_name("name").unwrap();
            let params_node = func_node.child_by_field_name("parameters").unwrap();

            // Create captures map
            let mut captures = std::collections::HashMap::new();
            captures.insert(
                "name",
                CaptureData {
                    node: name_node,
                    text: "my_func",
                },
            );
            captures.insert(
                "params",
                CaptureData {
                    node: params_node,
                    text: "(x: i32)",
                },
            );

            let ctx = ExtractContext::builder()
                .node(func_node)
                .source(source)
                .captures(captures)
                .file_path(Path::new("/test/file.rs"))
                .import_map(&import_map)
                .language(Language::Rust)
                .language_str("rust")
                .repository_id("test-repo")
                .repo_root(Path::new("/test"))
                .build()
                .unwrap();

            let handler = find_handler("rust::macro_test_handler").unwrap();
            let result = (handler.handler)(&ctx);
            assert!(result.is_ok(), "handler should succeed");
        }
    }
}
