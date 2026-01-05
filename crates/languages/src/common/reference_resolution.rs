//! Language-agnostic reference resolution
//!
//! This module provides generic reference resolution that works across
//! all programming languages using configuration from `PathConfig`.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use super::edge_case_handlers::{EdgeCaseContext, EdgeCaseRegistry};
use super::import_map::ImportMap;
use super::language_path::LanguagePath;
use super::path_config::PathConfig;

/// Result of resolving a reference
///
/// Contains the resolved qualified name and metadata about the reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedReference {
    /// The fully resolved qualified name (e.g., "std::collections::HashMap")
    pub target: String,
    /// The simple/unqualified name as it appeared in the source (e.g., "HashMap")
    pub simple_name: String,
    /// Whether this reference is to an external dependency (not in this repository)
    pub is_external: bool,
}

/// Context for reference resolution
///
/// Contains all the information needed to resolve references in a specific
/// file/module context.
pub struct ResolutionContext<'a> {
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
    /// Optional edge case handlers for language-specific patterns
    pub edge_case_handlers: Option<&'a EdgeCaseRegistry>,
}

impl ResolvedReference {
    /// Create a resolved reference with automatic external detection
    pub fn new(target: String, simple_name: String, config: &PathConfig) -> Self {
        let is_external = is_external_path(&target, config);
        Self {
            target,
            simple_name,
            is_external,
        }
    }

    /// Create an internal reference
    pub fn internal(target: String, simple_name: String) -> Self {
        Self {
            target,
            simple_name,
            is_external: false,
        }
    }

    /// Create an external reference
    pub fn external(target: String, simple_name: String) -> Self {
        Self {
            target,
            simple_name,
            is_external: true,
        }
    }
}

/// Check if a path is external based on the configuration
fn is_external_path(path: &str, config: &PathConfig) -> bool {
    let parsed = LanguagePath::parse(path, config);
    parsed.is_external()
}

/// Resolve a reference to its fully qualified name
///
/// Resolution order:
/// 0. Check edge case handlers (UFCS, well-known types)
/// 1. Handle relative paths (crate::, self::, super::) via LanguagePath::resolve()
/// 2. Check if already qualified and external
/// 3. Try import map lookup
/// 4. Handle Type::method patterns
/// 5. Try parent scope
/// 6. Try module-local with package prefix
///
/// # Arguments
/// * `name` - The name as it appears in source code (may be simple or qualified)
/// * `simple_name` - The simple/unqualified name extracted from the AST
/// * `ctx` - Resolution context with import map, package name, etc.
pub fn resolve_reference(
    name: &str,
    simple_name: &str,
    ctx: &ResolutionContext,
) -> ResolvedReference {
    let simple = simple_name.to_string();
    let config = ctx.path_config;

    // 0. Check edge case handlers first (e.g., UFCS, well-known stdlib types)
    if let Some(registry) = ctx.edge_case_handlers {
        let edge_ctx = EdgeCaseContext {
            import_map: ctx.import_map,
            parent_scope: ctx.parent_scope,
            package_name: ctx.package_name,
            current_module: ctx.current_module,
            path_config: ctx.path_config,
        };
        if let Some(resolved) = registry.try_resolve(name, simple_name, &edge_ctx) {
            return resolved;
        }
    }

    // Parse the name into a structured path
    let name_path = LanguagePath::parse(name, config);

    // 1. Handle relative paths (crate::, self::, super::)
    if name_path.is_relative() {
        let module = ctx
            .current_module
            .filter(|m| !m.is_empty())
            .map(|m| LanguagePath::parse(m, config));

        let resolved = name_path.resolve(
            ctx.package_name.filter(|p| !p.is_empty()),
            module.as_ref(),
            config,
        );

        return ResolvedReference::internal(resolved.to_qualified_name(), simple);
    }

    // 2. Already qualified paths need special handling
    if name_path.is_qualified() {
        // Check if it's an external path
        if name_path.is_external() {
            return ResolvedReference::external(name.to_string(), simple);
        }

        // Try to resolve through import map
        if let Some(resolved_path) = ctx.import_map.resolve(name) {
            return ResolvedReference::new(resolved_path.to_string(), simple, config);
        }

        // Handle Type::method patterns where first segment is imported
        let segments = name_path.segments();
        if segments.len() >= 2 {
            if let Some(first_segment) = name_path.first_segment() {
                if let Some(resolved_type) = ctx.import_map.resolve(first_segment) {
                    let resolved_type_path = LanguagePath::parse(resolved_type, config);

                    // If the resolved type has relative prefixes, normalize
                    if resolved_type_path.is_relative() {
                        let module = ctx
                            .current_module
                            .filter(|m| !m.is_empty())
                            .map(|m| LanguagePath::parse(m, config));

                        let normalized_type = resolved_type_path.resolve(
                            ctx.package_name.filter(|p| !p.is_empty()),
                            module.as_ref(),
                            config,
                        );

                        let result = LanguagePath::builder(config)
                            .segments(normalized_type.segments().iter().cloned())
                            .segments(segments[1..].iter().cloned())
                            .build();

                        return ResolvedReference::internal(result.to_qualified_name(), simple);
                    }

                    // Build the resolved path: resolved_type::remaining_segments
                    let resolved_path = LanguagePath::builder(config)
                        .segments(resolved_type_path.segments().iter().cloned())
                        .segments(segments[1..].iter().cloned())
                        .build()
                        .to_qualified_name();

                    // If the resolved type is scoped, prepend package name
                    if resolved_type_path.is_qualified() {
                        if let Some(pkg) = ctx.package_name {
                            if !pkg.is_empty() {
                                if let Some(first) = resolved_type_path.first_segment() {
                                    if first != pkg && !resolved_type_path.is_external() {
                                        let result = LanguagePath::builder(config)
                                            .segment(pkg)
                                            .segments(resolved_type_path.segments().iter().cloned())
                                            .segments(segments[1..].iter().cloned())
                                            .build();
                                        return ResolvedReference::internal(
                                            result.to_qualified_name(),
                                            simple,
                                        );
                                    }
                                }
                            }
                        }
                        return ResolvedReference::new(resolved_path, simple, config);
                    }

                    // Otherwise prepend package if available
                    if let Some(pkg) = ctx.package_name {
                        if !pkg.is_empty() {
                            let result = LanguagePath::builder(config)
                                .segment(pkg)
                                .segments(resolved_type_path.segments().iter().cloned())
                                .segments(segments[1..].iter().cloned())
                                .build();
                            return ResolvedReference::internal(result.to_qualified_name(), simple);
                        }
                    }
                    return ResolvedReference::new(resolved_path, simple, config);
                }
            }
        }

        // For relative scoped paths, prepend package name
        if let Some(pkg) = ctx.package_name {
            if !pkg.is_empty() {
                if let Some(first_segment) = name_path.first_segment() {
                    // If first segment matches package, already absolute
                    if first_segment == pkg {
                        return ResolvedReference::internal(name.to_string(), simple);
                    }
                    // Check if known external crate
                    if ctx
                        .import_map
                        .has_crate_import(first_segment, config.separator)
                    {
                        return ResolvedReference::external(name.to_string(), simple);
                    }
                }
                // Prepend package name
                let result = LanguagePath::builder(config)
                    .segment(pkg)
                    .segments(name_path.segments().iter().cloned())
                    .build();
                return ResolvedReference::internal(result.to_qualified_name(), simple);
            }
        }

        // Return as-is
        return ResolvedReference::new(name.to_string(), simple, config);
    }

    // 3. Simple name - try import map lookup
    if let Some(resolved_path) = ctx.import_map.resolve(name) {
        let resolved = LanguagePath::parse(resolved_path, config);

        // Handle relative imports
        if resolved.is_relative() {
            let module = ctx
                .current_module
                .filter(|m| !m.is_empty())
                .map(|m| LanguagePath::parse(m, config));

            let normalized = resolved.resolve(
                ctx.package_name.filter(|p| !p.is_empty()),
                module.as_ref(),
                config,
            );

            // Prepend package if needed
            if let Some(pkg) = ctx.package_name.filter(|p| !p.is_empty()) {
                if normalized.first_segment() != Some(pkg) {
                    let result = LanguagePath::builder(config)
                        .segment(pkg)
                        .segments(normalized.segments().iter().cloned())
                        .build();
                    return ResolvedReference::internal(result.to_qualified_name(), simple);
                }
            }

            return ResolvedReference::internal(normalized.to_qualified_name(), simple);
        }

        // Prepend package for internal scoped imports
        if resolved.is_qualified() && !resolved.is_external() {
            if let Some(pkg) = ctx.package_name.filter(|p| !p.is_empty()) {
                if resolved.first_segment() != Some(pkg) {
                    let result = LanguagePath::builder(config)
                        .segment(pkg)
                        .segments(resolved.segments().iter().cloned())
                        .build();
                    return ResolvedReference::internal(result.to_qualified_name(), simple);
                }
            }
        }

        return ResolvedReference::new(resolved_path.to_string(), simple, config);
    }

    // 4. Try glob imports (use first glob import as best-effort resolution)
    if let Some(glob_path) = ctx.import_map.glob_imports().first() {
        let result = format!("{}{}{}", glob_path, config.separator, name);
        return ResolvedReference::new(result, simple, config);
    }

    // 5. Try parent scope
    if let Some(scope) = ctx.parent_scope {
        if !scope.is_empty() {
            return ResolvedReference::internal(
                format!("{}{}{}", scope, config.separator, name),
                simple,
            );
        }
    }

    // 6. Fallback to module-local (package::module::name)
    let mut parts = Vec::new();

    if let Some(pkg) = ctx.package_name {
        if !pkg.is_empty() {
            parts.push(pkg.to_string());
        }
    }

    if let Some(module) = ctx.current_module {
        if !module.is_empty() {
            parts.push(module.to_string());
        }
    }

    parts.push(name.to_string());

    let target = parts.join(config.separator);
    ResolvedReference::internal(target, simple)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::common::path_config::RUST_PATH_CONFIG;

    fn make_context<'a>(
        import_map: &'a ImportMap,
        parent_scope: Option<&'a str>,
        package_name: Option<&'a str>,
        current_module: Option<&'a str>,
    ) -> ResolutionContext<'a> {
        ResolutionContext {
            import_map,
            parent_scope,
            package_name,
            current_module,
            path_config: &RUST_PATH_CONFIG,
            edge_case_handlers: None,
        }
    }

    #[test]
    fn test_resolve_simple_name_via_import() {
        let mut import_map = ImportMap::new("::");
        import_map.add("HashMap", "std::collections::HashMap");

        let ctx = make_context(&import_map, None, Some("mypackage"), None);
        let result = resolve_reference("HashMap", "HashMap", &ctx);

        assert_eq!(result.target, "std::collections::HashMap");
        assert!(result.is_external);
    }

    #[test]
    fn test_resolve_crate_relative() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map, None, Some("mypackage"), Some("utils"));

        let result = resolve_reference("crate::module::Type", "Type", &ctx);

        assert_eq!(result.target, "mypackage::module::Type");
        assert!(!result.is_external);
    }

    #[test]
    fn test_resolve_self_relative() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map, None, Some("mypackage"), Some("utils::network"));

        let result = resolve_reference("self::helper", "helper", &ctx);

        assert_eq!(result.target, "mypackage::utils::network::helper");
        assert!(!result.is_external);
    }

    #[test]
    fn test_resolve_super_relative() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map, None, Some("mypackage"), Some("utils::network"));

        let result = resolve_reference("super::sibling", "sibling", &ctx);

        assert_eq!(result.target, "mypackage::utils::sibling");
        assert!(!result.is_external);
    }

    #[test]
    fn test_resolve_external_std_path() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map, None, Some("mypackage"), None);

        let result = resolve_reference("std::io::Read", "Read", &ctx);

        assert_eq!(result.target, "std::io::Read");
        assert!(result.is_external);
    }

    #[test]
    fn test_resolve_with_parent_scope() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(
            &import_map,
            Some("mypackage::MyStruct"),
            Some("mypackage"),
            None,
        );

        let result = resolve_reference("helper_method", "helper_method", &ctx);

        assert_eq!(result.target, "mypackage::MyStruct::helper_method");
        assert!(!result.is_external);
    }

    #[test]
    fn test_resolve_unimported_local() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map, None, Some("mypackage"), Some("utils"));

        let result = resolve_reference("LocalType", "LocalType", &ctx);

        assert_eq!(result.target, "mypackage::utils::LocalType");
        assert!(!result.is_external);
    }

    #[test]
    fn test_resolve_type_method_pattern() {
        let mut import_map = ImportMap::new("::");
        import_map.add("Widget", "types::Widget");

        let ctx = make_context(&import_map, None, Some("mypackage"), None);
        let result = resolve_reference("Widget::new", "new", &ctx);

        assert_eq!(result.target, "mypackage::types::Widget::new");
        assert!(!result.is_external);
    }

    #[test]
    fn test_resolve_glob_import_fallback() {
        let mut import_map = ImportMap::new("::");
        import_map.add_glob("prelude");

        let ctx = make_context(&import_map, None, Some("mypackage"), None);
        let result = resolve_reference("SomeType", "SomeType", &ctx);

        assert_eq!(result.target, "prelude::SomeType");
    }

    #[test]
    fn test_resolve_chained_super() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map, None, Some("pkg"), Some("a::b::c"));

        let result = resolve_reference("super::super::ancestor", "ancestor", &ctx);

        assert_eq!(result.target, "pkg::a::ancestor");
        assert!(!result.is_external);
    }
}
