//! Rust-specific edge case handlers for FQN resolution
//!
//! This module provides handlers for Rust-specific patterns that require
//! special resolution logic:
//! - UFCS (Universal Function Call Syntax): `<Type as Trait>::method`
//! - Well-known std types: `Vec`, `String`, `HashMap`, etc.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::edge_case_handlers::{EdgeCaseContext, EdgeCaseHandler};
use crate::common::reference_resolution::{
    resolve_reference, ResolutionContext, ResolvedReference,
};

/// Well-known std types that should never be prefixed with a local crate name.
/// These are foreign types from the standard library.
///
/// NOTE: Option and Result are intentionally excluded because they are prelude types
/// that can be legitimately shadowed by user-defined types. When shadowed, references
/// should resolve to the local definition, not the std types. Excluding them allows
/// normal resolution to handle them (prepending the package name).
///
/// Some, None, Ok, Err are also excluded because they are enum variants (constructors),
/// not types, and would not be matched by the type_identifier query anyway.
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
    // Common std types (excluding Option/Result which can be shadowed)
    "String",
    "Vec",
    "Box",
    "Rc",
    "Arc",
    "Cell",
    "RefCell",
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

/// Handler for UFCS (Universal Function Call Syntax) patterns
///
/// Parses patterns like `<Type as Trait>::method` and resolves both the type
/// and trait through the import map to produce the canonical qualified name.
pub struct UfcsHandler;

impl EdgeCaseHandler for UfcsHandler {
    fn name(&self) -> &'static str {
        "rust_ufcs"
    }

    fn applies(&self, name: &str, _ctx: &EdgeCaseContext) -> bool {
        name.starts_with('<') && name.contains(" as ") && name.contains(">::")
    }

    fn resolve(&self, name: &str, simple_name: &str, ctx: &EdgeCaseContext) -> ResolvedReference {
        let simple = simple_name.to_string();

        // Parse: <Type as Trait>::method
        let as_pos = match name.find(" as ") {
            Some(pos) => pos,
            None => return ResolvedReference::new(name.to_string(), simple, ctx.path_config),
        };

        let method_sep = match name.find(">::") {
            Some(pos) => pos,
            None => return ResolvedReference::new(name.to_string(), simple, ctx.path_config),
        };

        // Extract components
        let type_name = &name[1..as_pos]; // Skip leading '<'
        let trait_name = &name[as_pos + 4..method_sep]; // Skip " as "
        let method_name = &name[method_sep + 3..]; // Skip ">::"

        // Create resolution context for nested resolution
        // Note: edge_case_handlers is None to avoid infinite recursion
        // Note: current_module is None because types/traits are defined at module level,
        // not inside functions. When called from a function body, ctx.current_module
        // would be the function's FQN (e.g., "test_crate::use_ufcs"), but type names
        // should resolve to module-level scope (e.g., "test_crate::Data").
        let resolution_ctx = ResolutionContext {
            import_map: ctx.import_map,
            parent_scope: None, // Type/trait names, not methods
            package_name: ctx.package_name,
            current_module: None,
            path_config: ctx.path_config,
            edge_case_handlers: None,
        };

        // Resolve both type and trait through imports
        let resolved_type = resolve_reference(type_name.trim(), type_name.trim(), &resolution_ctx);
        let resolved_trait =
            resolve_reference(trait_name.trim(), trait_name.trim(), &resolution_ctx);

        // If either type or trait is external, the whole UFCS call is external
        let is_external = resolved_type.is_external || resolved_trait.is_external;
        ResolvedReference {
            target: format!(
                "<{} as {}>::{method_name}",
                resolved_type.target, resolved_trait.target
            ),
            simple_name: simple,
            is_external,
        }
    }
}

/// Handler for well-known std types
///
/// These types should never be prefixed with a local crate name as they
/// are external references from the standard library.
pub struct StdTypeHandler;

impl EdgeCaseHandler for StdTypeHandler {
    fn name(&self) -> &'static str {
        "rust_std_types"
    }

    fn applies(&self, name: &str, _ctx: &EdgeCaseContext) -> bool {
        // Only apply to simple names (not qualified paths)
        !name.contains("::") && STD_TYPES.contains(&name)
    }

    fn resolve(&self, name: &str, simple_name: &str, _ctx: &EdgeCaseContext) -> ResolvedReference {
        ResolvedReference::external(name.to_string(), simple_name.to_string())
    }
}

/// Parse a trait impl method qualified name to extract the short form.
///
/// For example:
/// - `<test_crate::IntProducer as test_crate::Producer>::produce` -> `test_crate::IntProducer::produce`
/// - `<pkg::Type as pkg::Trait>::method` -> `pkg::Type::method`
///
/// Returns `None` if the qualified name is not a trait impl method.
pub fn parse_trait_impl_short_form(qualified_name: &str, separator: &str) -> Option<String> {
    // Check if it starts with '<' (trait impl syntax)
    if !qualified_name.starts_with('<') {
        return None;
    }

    // Find " as " separator
    let as_pos = qualified_name.find(" as ")?;

    // Extract the type FQN (between '<' and ' as ')
    let type_fqn = &qualified_name[1..as_pos];

    // Find ">::" which separates the trait from the method name
    let method_sep_pattern = format!(">{separator}");
    let method_sep = qualified_name.find(&method_sep_pattern)?;

    // Extract the method name (after >::)
    let method_name = &qualified_name[method_sep + method_sep_pattern.len()..];

    // Build the short form: TypeFQN::method
    Some(format!("{type_fqn}{separator}{method_name}"))
}

/// Check if a type name is a well-known std type that shouldn't be prefixed
pub fn is_std_type(name: &str) -> bool {
    STD_TYPES.contains(&name)
}

/// Static registry of Rust edge case handlers
pub static RUST_EDGE_CASE_HANDLERS: &[&dyn EdgeCaseHandler] = &[&UFCS_HANDLER, &STD_TYPE_HANDLER];

static UFCS_HANDLER: UfcsHandler = UfcsHandler;
static STD_TYPE_HANDLER: StdTypeHandler = StdTypeHandler;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::common::edge_case_handlers::EdgeCaseRegistry;
    use crate::common::import_map::ImportMap;
    use crate::common::path_config::RUST_PATH_CONFIG;

    fn make_context(import_map: &ImportMap) -> EdgeCaseContext<'_> {
        EdgeCaseContext {
            import_map,
            parent_scope: None,
            package_name: Some("test_crate"),
            current_module: None,
            path_config: &RUST_PATH_CONFIG,
        }
    }

    // ========================================================================
    // UFCS Handler Tests
    // ========================================================================

    #[test]
    fn test_ufcs_handler_applies() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map);

        assert!(UFCS_HANDLER.applies("<Data as Processor>::process", &ctx));
        assert!(UFCS_HANDLER.applies("<Widget as Drawable>::draw", &ctx));
        assert!(!UFCS_HANDLER.applies("Data::process", &ctx));
        assert!(!UFCS_HANDLER.applies("<Data>::process", &ctx)); // Missing " as "
    }

    #[test]
    fn test_ufcs_handler_resolve_basic() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map);

        let result = UFCS_HANDLER.resolve("<Data as Processor>::process", "process", &ctx);

        assert_eq!(
            result.target,
            "<test_crate::Data as test_crate::Processor>::process"
        );
        assert_eq!(result.simple_name, "process");
        assert!(!result.is_external);
    }

    #[test]
    fn test_ufcs_handler_resolve_with_imports() {
        let mut import_map = ImportMap::new("::");
        import_map.add("Widget", "types::Widget");
        import_map.add("Drawable", "traits::Drawable");

        let ctx = EdgeCaseContext {
            import_map: &import_map,
            parent_scope: None,
            package_name: Some("my_crate"),
            current_module: None,
            path_config: &RUST_PATH_CONFIG,
        };

        let result = UFCS_HANDLER.resolve("<Widget as Drawable>::draw", "draw", &ctx);

        assert_eq!(
            result.target,
            "<my_crate::types::Widget as my_crate::traits::Drawable>::draw"
        );
    }

    #[test]
    fn test_ufcs_handler_resolve_external() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map);

        let result =
            UFCS_HANDLER.resolve("<std::vec::Vec as std::iter::Iterator>::next", "next", &ctx);

        assert_eq!(
            result.target,
            "<std::vec::Vec as std::iter::Iterator>::next"
        );
        assert!(result.is_external);
    }

    // ========================================================================
    // Std Type Handler Tests
    // ========================================================================

    #[test]
    fn test_std_type_handler_applies() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map);

        assert!(STD_TYPE_HANDLER.applies("Vec", &ctx));
        assert!(STD_TYPE_HANDLER.applies("String", &ctx));
        assert!(STD_TYPE_HANDLER.applies("HashMap", &ctx));
        assert!(STD_TYPE_HANDLER.applies("i32", &ctx));

        // Option and Result are intentionally excluded to allow prelude shadowing
        assert!(!STD_TYPE_HANDLER.applies("Option", &ctx));
        assert!(!STD_TYPE_HANDLER.applies("Result", &ctx));

        assert!(!STD_TYPE_HANDLER.applies("MyStruct", &ctx));
        assert!(!STD_TYPE_HANDLER.applies("std::vec::Vec", &ctx)); // Qualified path
    }

    #[test]
    fn test_std_type_handler_resolve() {
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map);

        let result = STD_TYPE_HANDLER.resolve("Vec", "Vec", &ctx);

        assert_eq!(result.target, "Vec");
        assert_eq!(result.simple_name, "Vec");
        assert!(result.is_external);
    }

    // ========================================================================
    // Registry Integration Tests
    // ========================================================================

    #[test]
    fn test_rust_edge_case_registry() {
        let registry = EdgeCaseRegistry::from_handlers(RUST_EDGE_CASE_HANDLERS);
        let import_map = ImportMap::new("::");
        let ctx = make_context(&import_map);

        // UFCS should match
        let result = registry.try_resolve("<Data as Trait>::method", "method", &ctx);
        assert!(result.is_some());

        // Std type should match
        let result = registry.try_resolve("Vec", "Vec", &ctx);
        assert!(result.is_some());
        assert!(result.unwrap().is_external);

        // Regular identifier should not match
        let result = registry.try_resolve("MyType", "MyType", &ctx);
        assert!(result.is_none());
    }

    // ========================================================================
    // parse_trait_impl_short_form Tests
    // ========================================================================

    #[test]
    fn test_parse_trait_impl_short_form_basic() {
        let result = parse_trait_impl_short_form(
            "<test_crate::IntProducer as test_crate::Producer>::produce",
            "::",
        );
        assert_eq!(result, Some("test_crate::IntProducer::produce".to_string()));
    }

    #[test]
    fn test_parse_trait_impl_short_form_nested() {
        let result = parse_trait_impl_short_form("<pkg::sub::Type as pkg::Trait>::method", "::");
        assert_eq!(result, Some("pkg::sub::Type::method".to_string()));
    }

    #[test]
    fn test_parse_trait_impl_short_form_not_trait_impl() {
        assert_eq!(parse_trait_impl_short_form("pkg::Type::method", "::"), None);
        assert_eq!(parse_trait_impl_short_form("method", "::"), None);
    }

    // ========================================================================
    // is_std_type Tests
    // ========================================================================

    #[test]
    fn test_is_std_type() {
        assert!(is_std_type("Vec"));
        assert!(is_std_type("String"));
        assert!(is_std_type("i32"));
        assert!(is_std_type("HashMap"));

        assert!(!is_std_type("MyType"));
        assert!(!is_std_type("CustomStruct"));
    }
}
