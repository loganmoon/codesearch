//! Rust-specific import resolution and path normalization
//!
//! This module provides Rust-specific logic for:
//! - Normalizing relative paths (crate::, self::, super::)
//! - Resolving references through import maps
//! - Parsing use declarations
//! - Parsing trait impl method names

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::import_map::ImportMap;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

/// Normalize Rust-relative paths (crate::, self::, super::) to absolute qualified names
///
/// # Arguments
/// * `path` - The path to normalize (e.g., "crate::foo::Bar", "self::utils::Helper",
///   "super::super::other")
/// * `package_name` - The current crate name (e.g., "codesearch_core"). If None or empty,
///   the package prefix is omitted from the result.
/// * `current_module` - The current module path (e.g., "entities::error"). Required for
///   accurate self:: and super:: resolution; if None, those prefixes resolve at package root.
///
/// # Returns
/// The normalized absolute path, or the original path if not a relative path.
///
/// # Notes
/// - Supports chained super:: prefixes (e.g., `super::super::foo` navigates up two levels)
/// - When context is missing, gracefully degrades to partial resolution
pub fn normalize_rust_path(
    path: &str,
    package_name: Option<&str>,
    current_module: Option<&str>,
) -> String {
    if let Some(rest) = path.strip_prefix("crate::") {
        // crate:: -> package_name::rest
        match package_name {
            Some(pkg) if !pkg.is_empty() => format!("{pkg}::{rest}"),
            _ => rest.to_string(),
        }
    } else if let Some(rest) = path.strip_prefix("self::") {
        // self:: -> package_name::current_module::rest
        match (package_name, current_module) {
            (Some(pkg), Some(module)) if !pkg.is_empty() && !module.is_empty() => {
                format!("{pkg}::{module}::{rest}")
            }
            (Some(pkg), _) if !pkg.is_empty() => format!("{pkg}::{rest}"),
            (_, Some(module)) if !module.is_empty() => format!("{module}::{rest}"),
            _ => rest.to_string(),
        }
    } else if path.starts_with("super::") {
        // super:: -> navigate up from current_module (supports chained super::super::)
        let mut remaining = path;
        let mut levels_up = 0;

        // Count how many super:: prefixes we have
        while let Some(rest) = remaining.strip_prefix("super::") {
            levels_up += 1;
            remaining = rest;
        }

        if let Some(module) = current_module {
            let parts: Vec<&str> = module.split("::").collect();
            if parts.len() > levels_up {
                // Navigate up by levels_up
                let parent = parts[..parts.len() - levels_up].join("::");
                match package_name {
                    Some(pkg) if !pkg.is_empty() => format!("{pkg}::{parent}::{remaining}"),
                    _ => format!("{parent}::{remaining}"),
                }
            } else {
                // At or beyond root level, super:: goes to package root
                match package_name {
                    Some(pkg) if !pkg.is_empty() => format!("{pkg}::{remaining}"),
                    _ => remaining.to_string(),
                }
            }
        } else {
            // No module context, return with package prefix if available
            match package_name {
                Some(pkg) if !pkg.is_empty() => format!("{pkg}::{remaining}"),
                _ => remaining.to_string(),
            }
        }
    } else {
        // Not a relative path, return as-is
        path.to_string()
    }
}

/// Resolve a UFCS (Universal Function Call Syntax) call pattern.
///
/// Parses patterns like `<Type as Trait>::method` and resolves both the type
/// and trait through the import map to produce the canonical qualified name
/// matching trait impl method entities.
///
/// # Arguments
/// * `name` - The UFCS call pattern (e.g., `<Data as Processor>::process`)
/// * `import_map` - Import map for resolving type and trait names
/// * `package_name` - Current crate name for path normalization
/// * `current_module` - Current module path for path normalization
///
/// # Returns
/// The resolved canonical form (e.g., `<test_crate::Data as test_crate::Processor>::process`)
/// or the original pattern if parsing fails.
fn resolve_ufcs_call(
    name: &str,
    import_map: &ImportMap,
    package_name: Option<&str>,
    current_module: Option<&str>,
) -> String {
    // Parse: <Type as Trait>::method
    // Find " as " separator
    let as_pos = match name.find(" as ") {
        Some(pos) => pos,
        None => return name.to_string(), // Can't parse, return as-is
    };

    // Find ">::" which separates the trait from the method name
    let method_sep = match name.find(">::") {
        Some(pos) => pos,
        None => return name.to_string(), // Can't parse, return as-is
    };

    // Extract components
    let type_name = &name[1..as_pos]; // Skip leading '<'
    let trait_name = &name[as_pos + 4..method_sep]; // Skip " as "
    let method_name = &name[method_sep + 3..]; // Skip ">::"

    // Resolve both type and trait through imports
    // Use None for parent_scope since we're resolving type/trait names, not methods
    let resolved_type = resolve_rust_reference(
        type_name.trim(),
        import_map,
        None,
        package_name,
        current_module,
    );
    let resolved_trait = resolve_rust_reference(
        trait_name.trim(),
        import_map,
        None,
        package_name,
        current_module,
    );

    // Return canonical form matching trait impl method qualified names
    format!("<{resolved_type} as {resolved_trait}>::{method_name}")
}

/// Well-known std types that should never be prefixed with a local crate name.
/// These are foreign types from the standard library.
const STD_TYPES: &[&str] = &[
    // Primitive types (not really std but also shouldn't be prefixed)
    "bool",
    "char",
    "str",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    "f32",
    "f64",
    // Common std types
    "String",
    "Vec",
    "Box",
    "Rc",
    "Arc",
    "Cell",
    "RefCell",
    "Option",
    "Result",
    "Some",
    "None",
    "Ok",
    "Err",
    "HashMap",
    "HashSet",
    "BTreeMap",
    "BTreeSet",
    "Path",
    "PathBuf",
    "OsStr",
    "OsString",
    "Cow",
    "Mutex",
    "RwLock",
    "Pin",
    "PhantomData",
    "Ordering",
    "Duration",
    "Instant",
    "SystemTime",
];

/// Check if a type name is a well-known std type that shouldn't be prefixed
fn is_std_type(name: &str) -> bool {
    STD_TYPES.contains(&name)
}

/// Resolve a Rust reference with path normalization
///
/// This extends resolve_reference() to handle crate::, self::, super:: prefixes.
///
/// Resolution order:
/// 1. If path starts with crate::/self::/super::, normalize it and return
/// 2. If already scoped (contains ::), use as-is
/// 3. Try import map lookup
/// 4. Try parent_scope::name
/// 5. Try package_name::current_module::name (for locally-defined types)
/// 6. Mark as external::name (only if no package context available)
pub fn resolve_rust_reference(
    name: &str,
    import_map: &ImportMap,
    parent_scope: Option<&str>,
    package_name: Option<&str>,
    current_module: Option<&str>,
) -> String {
    // Handle UFCS (Universal Function Call Syntax) patterns: <Type as Trait>::method
    if name.starts_with('<') {
        return resolve_ufcs_call(name, import_map, package_name, current_module);
    }

    // Check if this is a well-known std type - don't prefix with local crate name
    if is_std_type(name) {
        return name.to_string();
    }

    // First normalize any Rust-relative paths
    if name.starts_with("crate::") || name.starts_with("self::") || name.starts_with("super::") {
        return normalize_rust_path(name, package_name, current_module);
    }

    // Already scoped paths need special handling
    if ImportMap::is_scoped(name, "::") {
        // Check if it looks like an external path
        // If it starts with known external prefixes, return as-is
        if name.starts_with("std::")
            || name.starts_with("core::")
            || name.starts_with("alloc::")
            || name.starts_with("external::")
        {
            return name.to_string();
        }
        // Try to resolve through import map first
        if let Some(resolved) = import_map.resolve(name) {
            return resolved.to_string();
        }

        // Handle Type::method patterns where the first segment is an imported type
        // For example: Widget::new where Widget is imported from types::Widget
        // Should resolve to types::Widget::new
        let segments: Vec<&str> = name.split("::").collect();
        if segments.len() >= 2 {
            let first_segment = segments[0];
            if let Some(resolved_type) = import_map.resolve(first_segment) {
                // The first segment was in the import map
                // Build the resolved path: resolved_type::remaining_segments
                let remaining = segments[1..].join("::");
                let resolved_path = format!("{resolved_type}::{remaining}");

                // If the resolved type has Rust-relative prefixes, normalize
                if resolved_type.starts_with("crate::")
                    || resolved_type.starts_with("self::")
                    || resolved_type.starts_with("super::")
                {
                    let normalized_type =
                        normalize_rust_path(resolved_type, package_name, current_module);
                    return format!("{normalized_type}::{remaining}");
                }

                // If the resolved type is a scoped path (e.g., types::Widget),
                // prepend package name to make it absolute
                if ImportMap::is_scoped(resolved_type, "::") {
                    if let Some(pkg) = package_name {
                        if !pkg.is_empty() {
                            let first = resolved_type.split("::").next().unwrap_or(resolved_type);
                            if first != pkg
                                && !resolved_type.starts_with("std::")
                                && !resolved_type.starts_with("core::")
                                && !resolved_type.starts_with("alloc::")
                            {
                                return format!("{pkg}::{resolved_path}");
                            }
                        }
                    }
                    return resolved_path;
                }

                // Otherwise prepend package if available
                if let Some(pkg) = package_name {
                    if !pkg.is_empty() {
                        return format!("{pkg}::{resolved_path}");
                    }
                }
                return resolved_path;
            }
        }

        // For relative scoped paths like `utils::helper`, prepend package name
        // to make them absolute (e.g., `test_crate::utils::helper`).
        // BUT: if the first segment is a known external crate, return as-is.
        if let Some(pkg) = package_name {
            if !pkg.is_empty() {
                // Extract the first path segment
                let first_segment = name.split("::").next().unwrap_or(name);
                // If the first segment matches the package name, it's already
                // absolute within this crate - return as-is
                if first_segment == pkg {
                    return name.to_string();
                }
                // Check if any import in the map starts with this segment.
                // If so, it's a known external crate (e.g., `serde::Deserialize`
                // when we have `use serde::Serialize;`).
                if import_map.has_crate_import(first_segment, "::") {
                    return name.to_string();
                }
                // Otherwise, assume it's a relative internal path and prepend
                // the package name (e.g., `utils::helper` -> `my_crate::utils::helper`)
                return format!("{pkg}::{name}");
            }
        }
        return name.to_string();
    }

    // Try import map
    if let Some(resolved) = import_map.resolve(name) {
        // Normalize result if it contains Rust-relative prefixes
        if resolved.starts_with("crate::")
            || resolved.starts_with("self::")
            || resolved.starts_with("super::")
        {
            return normalize_rust_path(resolved, package_name, current_module);
        }

        // If resolved is a scoped path (contains ::), it may be a relative internal path
        // that needs package name prepended.
        // Note: Unlike the scoped path handling above (for direct input), we don't use
        // has_crate_import here because the path already came from the import map.
        // Import map paths are typically internal module paths that need the package prefix.
        if ImportMap::is_scoped(resolved, "::") {
            // Check if it looks like an external path (known std lib prefixes)
            if resolved.starts_with("std::")
                || resolved.starts_with("core::")
                || resolved.starts_with("alloc::")
                || resolved.starts_with("external::")
            {
                return resolved.to_string();
            }
            // For relative scoped paths, prepend package name
            if let Some(pkg) = package_name {
                if !pkg.is_empty() {
                    let first_segment = resolved.split("::").next().unwrap_or(resolved);
                    // If first segment matches package name, it's already absolute
                    if first_segment == pkg {
                        return resolved.to_string();
                    }
                    // Prepend package name for relative internal paths
                    return format!("{pkg}::{resolved}");
                }
            }
        }

        return resolved.to_string();
    }

    // Try glob imports as fallback
    // For `use helpers::*`, when encountering bare `helper_a`, we try glob imports.
    // Since the Rust compiler would error on ambiguous symbols from multiple glob imports,
    // we can assume only one glob import would provide any given symbol.
    // We use the first glob import as a best-effort resolution since we cannot verify
    // at extraction time which module actually exports the symbol.
    if let Some(glob_path) = import_map.glob_imports().first() {
        // Build the potential qualified path from glob import
        let candidate = format!("{glob_path}::{name}");

        // Normalize with package name if available
        if let Some(pkg) = package_name {
            if !pkg.is_empty() {
                // Check if glob_path is already absolute
                if glob_path.starts_with(pkg) {
                    return format!("{glob_path}::{name}");
                }
                // Otherwise prepend package name
                return format!("{pkg}::{candidate}");
            }
        }
        return candidate;
    }

    // Try parent scope
    if let Some(scope) = parent_scope {
        if !scope.is_empty() {
            return format!("{scope}::{name}");
        }
    }

    // Try package_name::current_module::name for locally-defined types
    // This handles types defined in the same module that aren't imported
    match (package_name, current_module) {
        (Some(pkg), Some(module)) if !pkg.is_empty() && !module.is_empty() => {
            format!("{pkg}::{module}::{name}")
        }
        (Some(pkg), _) if !pkg.is_empty() => {
            // At crate root (no module path)
            format!("{pkg}::{name}")
        }
        _ => {
            // No package context available, mark as external
            format!("external::{name}")
        }
    }
}

/// Parse Rust use declarations
///
/// Handles:
/// - `use std::io::Read;` -> ("Read", "std::io::Read")
/// - `use std::io::{Read, Write};` -> [("Read", "std::io::Read"), ("Write", "std::io::Write")]
/// - `use std::io::Read as MyRead;` -> ("MyRead", "std::io::Read")
/// - `use helpers::*;` -> stores "helpers" as glob import for fallback resolution
pub fn parse_rust_imports(root: Node, source: &str) -> ImportMap {
    let mut import_map = ImportMap::new("::");

    let query_source = r#"
        (use_declaration
          argument: (use_as_clause
            path: (_) @path
            alias: (identifier) @alias))

        (use_declaration
          argument: (scoped_identifier) @scoped_path)

        (use_declaration
          argument: (scoped_use_list
            path: (_) @base_path
            list: (use_list) @use_list))

        (use_declaration
          argument: (identifier) @simple_import)

        (use_declaration
          argument: (use_wildcard
            (scoped_identifier) @wildcard_scope))

        (use_declaration
          argument: (use_wildcard
            (identifier) @wildcard_simple))
    "#;

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return import_map,
    };

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, source.as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            let capture_name = query
                .capture_names()
                .get(capture.index as usize)
                .copied()
                .unwrap_or("");

            match capture_name {
                "alias" => {
                    // use X as Y - the alias name
                    if let (Some(path_cap), Ok(alias_text)) = (
                        query_match
                            .captures
                            .iter()
                            .find(|c| {
                                query.capture_names().get(c.index as usize).copied() == Some("path")
                            })
                            .map(|c| c.node),
                        capture.node.utf8_text(source.as_bytes()),
                    ) {
                        if let Ok(path_text) = path_cap.utf8_text(source.as_bytes()) {
                            import_map.add(alias_text, path_text);
                        }
                    }
                }
                "scoped_path" => {
                    // use std::io::Read - extract the last segment as simple name
                    if let Ok(full_path) = capture.node.utf8_text(source.as_bytes()) {
                        if let Some(simple_name) = full_path.rsplit("::").next() {
                            if simple_name == "*" {
                                // Store glob import base path for fallback resolution
                                if let Some(base) = full_path.strip_suffix("::*") {
                                    import_map.add_glob(base);
                                }
                            } else {
                                import_map.add(simple_name, full_path);
                            }
                        }
                    }
                }
                "use_list" => {
                    // use std::io::{Read, Write} - process the list
                    if let Some(base_path_cap) = query_match.captures.iter().find(|c| {
                        query.capture_names().get(c.index as usize).copied() == Some("base_path")
                    }) {
                        if let Ok(base_path) = base_path_cap.node.utf8_text(source.as_bytes()) {
                            parse_rust_use_list(capture.node, source, base_path, &mut import_map);
                        }
                    }
                }
                "simple_import" => {
                    // use identifier - rare but valid
                    if let Ok(name) = capture.node.utf8_text(source.as_bytes()) {
                        import_map.add(name, name);
                    }
                }
                "wildcard_scope" => {
                    // use helpers::* - scoped wildcard import
                    if let Ok(base_path) = capture.node.utf8_text(source.as_bytes()) {
                        import_map.add_glob(base_path);
                    }
                }
                "wildcard_simple" => {
                    // use ident::* - simple identifier wildcard
                    if let Ok(base_path) = capture.node.utf8_text(source.as_bytes()) {
                        import_map.add_glob(base_path);
                    }
                }
                _ => {}
            }
        }
    }

    import_map
}

/// Parse items in a Rust use list (e.g., `{Read, Write, BufReader as BR}`)
fn parse_rust_use_list(list_node: Node, source: &str, base_path: &str, import_map: &mut ImportMap) {
    let mut cursor = list_node.walk();

    for child in list_node.children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                if let Ok(name) = child.utf8_text(source.as_bytes()) {
                    let full_path = format!("{base_path}::{name}");
                    import_map.add(name, &full_path);
                }
            }
            "use_as_clause" => {
                // Handle `Read as R` or `tcp::connect as tcp_connect` within a use list
                // use_as_clause has named fields: path (identifier or scoped_identifier) and alias (identifier)
                let path_node = child.child_by_field_name("path");
                let alias_node = child.child_by_field_name("alias");

                if let (Some(p_node), Some(a_node)) = (path_node, alias_node) {
                    if let (Ok(path_text), Ok(alias_text)) = (
                        p_node.utf8_text(source.as_bytes()),
                        a_node.utf8_text(source.as_bytes()),
                    ) {
                        let full_path = format!("{base_path}::{path_text}");
                        import_map.add(alias_text, &full_path);
                    }
                }
            }
            "scoped_identifier" => {
                // Handle nested paths like `io::Read` within a use list
                if let Ok(scoped_path) = child.utf8_text(source.as_bytes()) {
                    let full_path = format!("{base_path}::{scoped_path}");
                    if let Some(simple_name) = scoped_path.rsplit("::").next() {
                        import_map.add(simple_name, &full_path);
                    }
                }
            }
            "self" => {
                // Handle `use foo::{self}` - imports the base path itself
                if let Some(simple_name) = base_path.rsplit("::").next() {
                    import_map.add(simple_name, base_path);
                }
            }
            "scoped_use_list" => {
                // Handle nested use groups like `http::{get as http_get, post}`
                // The child has structure: path: (identifier/scoped_identifier), list: (use_list)
                if let (Some(path_node), Some(nested_list_node)) = (
                    child.child_by_field_name("path"),
                    child.child_by_field_name("list"),
                ) {
                    if let Ok(path_text) = path_node.utf8_text(source.as_bytes()) {
                        let nested_base = format!("{base_path}::{path_text}");
                        parse_rust_use_list(nested_list_node, source, &nested_base, import_map);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Parse a trait impl method qualified name to extract the short form.
///
/// For example:
/// - `<test_crate::IntProducer as test_crate::Producer>::produce` -> `test_crate::IntProducer::produce`
/// - `<pkg::Type as pkg::Trait>::method` -> `pkg::Type::method`
///
/// Returns `None` if the qualified name is not a trait impl method.
pub fn parse_trait_impl_short_form(qualified_name: &str) -> Option<String> {
    // Check if it starts with '<' (trait impl syntax)
    if !qualified_name.starts_with('<') {
        return None;
    }

    // Find the pattern: <TypeFQN as TraitFQN>::method
    // We need to extract TypeFQN and method

    // Find the " as " separator
    let as_pos = qualified_name.find(" as ")?;

    // Extract the type FQN (between '<' and ' as ')
    let type_fqn = &qualified_name[1..as_pos];

    // Find ">::" which separates the trait from the method name
    let method_sep = qualified_name.find(">::")?;

    // Extract the method name (after >::)
    let method_name = &qualified_name[method_sep + 3..];

    // Build the short form: TypeFQN::method
    Some(format!("{type_fqn}::{method_name}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ========================================================================
    // Tests for Rust path normalization (crate::, self::, super::)
    // ========================================================================

    #[test]
    fn test_normalize_rust_path_crate_with_package() {
        // crate::foo::Bar with package "mypackage" -> mypackage::foo::Bar
        assert_eq!(
            normalize_rust_path("crate::foo::Bar", Some("mypackage"), Some("utils")),
            "mypackage::foo::Bar"
        );
    }

    #[test]
    fn test_normalize_rust_path_crate_without_package() {
        // crate::foo::Bar without package -> foo::Bar
        assert_eq!(
            normalize_rust_path("crate::foo::Bar", None, Some("utils")),
            "foo::Bar"
        );
    }

    #[test]
    fn test_normalize_rust_path_self_with_module() {
        // self::helper in mypackage::utils::network -> mypackage::utils::network::helper
        assert_eq!(
            normalize_rust_path("self::helper", Some("mypackage"), Some("utils::network")),
            "mypackage::utils::network::helper"
        );
    }

    #[test]
    fn test_normalize_rust_path_self_without_module() {
        // self::helper with package but no module -> mypackage::helper
        assert_eq!(
            normalize_rust_path("self::helper", Some("mypackage"), None),
            "mypackage::helper"
        );
    }

    #[test]
    fn test_normalize_rust_path_super_with_parent() {
        // super::other in mypackage::utils::network -> mypackage::utils::other
        assert_eq!(
            normalize_rust_path("super::other", Some("mypackage"), Some("utils::network")),
            "mypackage::utils::other"
        );
    }

    #[test]
    fn test_normalize_rust_path_super_at_root() {
        // super::other in mypackage::utils (single-level module) -> mypackage::other
        assert_eq!(
            normalize_rust_path("super::other", Some("mypackage"), Some("utils")),
            "mypackage::other"
        );
    }

    #[test]
    fn test_normalize_rust_path_not_relative() {
        // std::io::Read is not a relative path, should be returned as-is
        assert_eq!(
            normalize_rust_path("std::io::Read", Some("mypackage"), Some("utils")),
            "std::io::Read"
        );
    }

    // ========================================================================
    // Tests for chained super:: paths (super::super::foo)
    // ========================================================================

    #[test]
    fn test_normalize_rust_path_double_super() {
        // super::super::thing in mypackage::a::b::c should resolve to mypackage::a::thing
        assert_eq!(
            normalize_rust_path("super::super::thing", Some("mypackage"), Some("a::b::c")),
            "mypackage::a::thing"
        );
    }

    #[test]
    fn test_normalize_rust_path_triple_super() {
        // super::super::super::thing in mypackage::a::b::c::d should resolve to mypackage::a::thing
        assert_eq!(
            normalize_rust_path(
                "super::super::super::thing",
                Some("mypackage"),
                Some("a::b::c::d")
            ),
            "mypackage::a::thing"
        );
    }

    #[test]
    fn test_normalize_rust_path_super_exceeds_depth() {
        // super::super::super in mypackage::a::b (only 2 levels) should go to package root
        assert_eq!(
            normalize_rust_path(
                "super::super::super::thing",
                Some("mypackage"),
                Some("a::b")
            ),
            "mypackage::thing"
        );
    }

    // ========================================================================
    // Tests for resolve_ufcs_call
    // ========================================================================

    #[test]
    fn test_resolve_ufcs_call_basic() {
        // UFCS call with bare type and trait names in same module
        let import_map = ImportMap::new("::");
        let result = resolve_ufcs_call(
            "<Data as Processor>::process",
            &import_map,
            Some("test_crate"),
            None,
        );
        assert_eq!(
            result,
            "<test_crate::Data as test_crate::Processor>::process"
        );
    }

    #[test]
    fn test_resolve_ufcs_call_with_imports() {
        // UFCS call where type and trait are imported
        let mut import_map = ImportMap::new("::");
        import_map.add("Widget", "types::Widget");
        import_map.add("Drawable", "traits::Drawable");

        let result = resolve_ufcs_call(
            "<Widget as Drawable>::draw",
            &import_map,
            Some("my_crate"),
            None,
        );
        assert_eq!(
            result,
            "<my_crate::types::Widget as my_crate::traits::Drawable>::draw"
        );
    }

    #[test]
    fn test_resolve_ufcs_call_already_qualified() {
        // UFCS call with already-qualified paths
        let import_map = ImportMap::new("::");
        let result = resolve_ufcs_call(
            "<std::vec::Vec as std::iter::Iterator>::next",
            &import_map,
            Some("test_crate"),
            None,
        );
        // std:: paths should remain as-is
        assert_eq!(result, "<std::vec::Vec as std::iter::Iterator>::next");
    }

    #[test]
    fn test_resolve_ufcs_call_invalid_pattern() {
        // Invalid UFCS patterns should be returned as-is
        let import_map = ImportMap::new("::");

        // Missing " as "
        let result = resolve_ufcs_call("<Data>::process", &import_map, Some("test"), None);
        assert_eq!(result, "<Data>::process");

        // Missing ">::"
        let result = resolve_ufcs_call("<Data as Trait>", &import_map, Some("test"), None);
        assert_eq!(result, "<Data as Trait>");
    }

    #[test]
    fn test_resolve_ufcs_call_with_whitespace() {
        // UFCS with extra whitespace should still parse correctly
        let import_map = ImportMap::new("::");
        let result = resolve_ufcs_call(
            "< Data as Processor >::process",
            &import_map,
            Some("test_crate"),
            None,
        );
        assert_eq!(
            result,
            "<test_crate::Data as test_crate::Processor>::process"
        );
    }

    // ========================================================================
    // Tests for parse_trait_impl_short_form
    // ========================================================================

    #[test]
    fn test_parse_trait_impl_short_form_basic() {
        let result = parse_trait_impl_short_form(
            "<test_crate::IntProducer as test_crate::Producer>::produce",
        );
        assert_eq!(result, Some("test_crate::IntProducer::produce".to_string()));
    }

    #[test]
    fn test_parse_trait_impl_short_form_nested() {
        let result = parse_trait_impl_short_form("<pkg::sub::Type as pkg::Trait>::method");
        assert_eq!(result, Some("pkg::sub::Type::method".to_string()));
    }

    #[test]
    fn test_parse_trait_impl_short_form_not_trait_impl() {
        // Regular method paths should return None
        assert_eq!(parse_trait_impl_short_form("pkg::Type::method"), None);
        assert_eq!(parse_trait_impl_short_form("method"), None);
    }

    // ========================================================================
    // Tests for parse_rust_imports
    // ========================================================================

    #[test]
    fn test_parse_rust_simple_import() {
        let source = "use std::io::Read;";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert_eq!(import_map.resolve("Read"), Some("std::io::Read"));
    }

    #[test]
    fn test_parse_rust_use_list() {
        let source = "use std::io::{Read, Write};";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert_eq!(import_map.resolve("Read"), Some("std::io::Read"));
        assert_eq!(import_map.resolve("Write"), Some("std::io::Write"));
    }

    #[test]
    fn test_parse_rust_alias() {
        let source = "use std::io::Read as MyRead;";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert_eq!(import_map.resolve("MyRead"), Some("std::io::Read"));
        assert_eq!(import_map.resolve("Read"), None);
    }

    #[test]
    fn test_parse_rust_glob_import() {
        let source = "use helpers::*;";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert!(!import_map.glob_imports().is_empty());
        assert_eq!(import_map.glob_imports()[0], "helpers");
    }

    #[test]
    fn test_parse_rust_nested_glob_import() {
        let source = "use std::collections::*;";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert!(!import_map.glob_imports().is_empty());
        assert_eq!(import_map.glob_imports()[0], "std::collections");
    }

    #[test]
    fn test_parse_rust_nested_use_with_renaming() {
        let source = r#"
use network::{
    http::{get as http_get, post as http_post},
    tcp::connect as tcp_connect,
};
"#;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).unwrap();

        let import_map = parse_rust_imports(tree.root_node(), source);

        assert_eq!(
            import_map.resolve("http_get"),
            Some("network::http::get"),
            "http_get should resolve to network::http::get"
        );
        assert_eq!(
            import_map.resolve("http_post"),
            Some("network::http::post"),
            "http_post should resolve to network::http::post"
        );
        assert_eq!(
            import_map.resolve("tcp_connect"),
            Some("network::tcp::connect"),
            "tcp_connect should resolve to network::tcp::connect"
        );
    }

    // ========================================================================
    // Tests for resolve_rust_reference with glob imports
    // ========================================================================

    #[test]
    fn test_resolve_rust_reference_glob_fallback() {
        let mut import_map = ImportMap::new("::");
        import_map.add_glob("helpers");

        // Bare identifier should resolve through glob import
        let result = resolve_rust_reference("helper_a", &import_map, None, Some("pkg"), None);
        assert_eq!(result, "pkg::helpers::helper_a");
    }

    #[test]
    fn test_resolve_rust_reference_multiple_globs_tries_first() {
        let mut import_map = ImportMap::new("::");
        import_map.add_glob("helpers");
        import_map.add_glob("utils");

        // Should use first glob import
        let result = resolve_rust_reference("some_func", &import_map, None, Some("pkg"), None);
        assert_eq!(result, "pkg::helpers::some_func");
    }
}
