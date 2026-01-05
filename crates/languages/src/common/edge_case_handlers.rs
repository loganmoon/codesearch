//! Pluggable edge case handlers for language-specific resolution quirks
//!
//! This module provides a trait-based system for handling language-specific
//! edge cases in reference resolution without polluting the generic resolver.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use super::import_map::ImportMap;
use super::path_config::PathConfig;
use super::reference_resolution::ResolvedReference;

/// Context for edge case resolution
///
/// Provides access to all the information an edge case handler might need.
pub struct EdgeCaseContext<'a> {
    /// Import map for looking up imported names
    pub import_map: &'a ImportMap,
    /// Parent scope for method/field resolution
    pub parent_scope: Option<&'a str>,
    /// Current package/crate name
    pub package_name: Option<&'a str>,
    /// Current module path
    pub current_module: Option<&'a str>,
    /// Path configuration for this language
    pub path_config: &'static PathConfig,
}

/// Trait for language-specific edge case handlers
///
/// Edge case handlers intercept reference resolution for specific patterns
/// that require special handling (e.g., UFCS in Rust, well-known stdlib types).
pub trait EdgeCaseHandler: Send + Sync {
    /// Name of this handler for debugging/logging
    fn name(&self) -> &'static str;

    /// Check if this handler should process the given reference
    ///
    /// Returns true if the handler can handle this pattern.
    fn applies(&self, name: &str, ctx: &EdgeCaseContext) -> bool;

    /// Handle the edge case and return the resolved reference
    ///
    /// This is only called if `applies()` returned true.
    fn resolve(&self, name: &str, simple_name: &str, ctx: &EdgeCaseContext) -> ResolvedReference;
}

/// Registry of edge case handlers for a language
///
/// Handlers are tried in order until one matches. The first matching
/// handler's result is used.
#[derive(Default)]
pub struct EdgeCaseRegistry {
    handlers: Vec<&'static dyn EdgeCaseHandler>,
}

impl EdgeCaseRegistry {
    /// Create a new empty registry
    pub const fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Create a registry from a static slice of handlers
    pub fn from_handlers(handlers: &'static [&'static dyn EdgeCaseHandler]) -> Self {
        Self {
            handlers: handlers.to_vec(),
        }
    }

    /// Try to resolve using edge case handlers
    ///
    /// Returns Some(resolved) if a handler matched, None otherwise.
    pub fn try_resolve(
        &self,
        name: &str,
        simple_name: &str,
        ctx: &EdgeCaseContext,
    ) -> Option<ResolvedReference> {
        for handler in &self.handlers {
            if handler.applies(name, ctx) {
                return Some(handler.resolve(name, simple_name, ctx));
            }
        }
        None
    }

    /// Check if the registry has any handlers
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Get the number of handlers
    pub fn len(&self) -> usize {
        self.handlers.len()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::common::path_config::RUST_PATH_CONFIG;

    /// Test handler that matches names starting with "test_"
    struct TestPrefixHandler;

    impl EdgeCaseHandler for TestPrefixHandler {
        fn name(&self) -> &'static str {
            "test_prefix"
        }

        fn applies(&self, name: &str, _ctx: &EdgeCaseContext) -> bool {
            name.starts_with("test_")
        }

        fn resolve(
            &self,
            name: &str,
            simple_name: &str,
            _ctx: &EdgeCaseContext,
        ) -> ResolvedReference {
            ResolvedReference::external(format!("tests::{name}"), simple_name.to_string())
        }
    }

    static TEST_HANDLER: TestPrefixHandler = TestPrefixHandler;

    fn make_context(import_map: &ImportMap) -> EdgeCaseContext<'_> {
        EdgeCaseContext {
            import_map,
            parent_scope: None,
            package_name: None,
            current_module: None,
            path_config: &RUST_PATH_CONFIG,
        }
    }

    #[test]
    fn test_registry_empty() {
        let registry = EdgeCaseRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_from_handlers() {
        static HANDLERS: &[&dyn EdgeCaseHandler] = &[&TEST_HANDLER];
        let registry = EdgeCaseRegistry::from_handlers(HANDLERS);
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_handler_applies() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map);

        assert!(TEST_HANDLER.applies("test_something", &ctx));
        assert!(!TEST_HANDLER.applies("other_name", &ctx));
    }

    #[test]
    fn test_registry_try_resolve_match() {
        static HANDLERS: &[&dyn EdgeCaseHandler] = &[&TEST_HANDLER];
        let registry = EdgeCaseRegistry::from_handlers(HANDLERS);

        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map);

        let result = registry.try_resolve("test_func", "test_func", &ctx);
        assert!(result.is_some());
        let resolved = result.unwrap();
        assert_eq!(resolved.target, "tests::test_func");
        assert!(resolved.is_external);
    }

    #[test]
    fn test_registry_try_resolve_no_match() {
        static HANDLERS: &[&dyn EdgeCaseHandler] = &[&TEST_HANDLER];
        let registry = EdgeCaseRegistry::from_handlers(HANDLERS);

        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map);

        let result = registry.try_resolve("other_func", "other_func", &ctx);
        assert!(result.is_none());
    }
}
