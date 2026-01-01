//! Structured representation of Rust paths
//!
//! This module provides `RustPath`, an immutable type that eliminates string manipulation
//! for consumers when handling paths in import resolution. Instead of callers using
//! `split("::")`, `join("::")`, and `strip_prefix()`, paths are represented as structured
//! data with typed path kinds and accessed via methods like `segments()` and `simple_name()`.
//!
//! Note: The `parse()` constructor still uses string operations internally, but consumers
//! work with structured data thereafter.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use std::fmt;

/// Well-known external crate prefixes that indicate standard library or common external crates
const EXTERNAL_PREFIXES: &[&str] = &["std", "core", "alloc", "external"];

/// The kind of a Rust path, indicating how it should be resolved
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RustPathKind {
    /// Absolute path (e.g., `std::collections::HashMap`, `mypackage::module::Type`)
    /// Also used for simple names without prefixes (e.g., `HashMap`), which may still
    /// need resolution through import maps or scope lookup.
    #[default]
    Absolute,

    /// `crate::` prefix (e.g., `crate::module::Type`)
    /// Resolves to `package_name::rest` during normalization.
    Crate,

    /// `self::` prefix (e.g., `self::submodule::Type`)
    /// Resolves to `package_name::current_module::rest` during normalization.
    SelfRelative,

    /// `super::` prefix with level count (e.g., `super::super::sibling::Type`)
    /// The levels field indicates how many parent modules to navigate up.
    Super { levels: u32 },

    /// External/unknown origin (e.g., `external::serde`)
    /// Used for references that couldn't be resolved to a known entity.
    External,
}

/// An immutable structured representation of a Rust path.
///
/// Instead of storing paths as strings with `"::"` separators, this type stores
/// the path kind and segments separately, enabling type-safe operations without
/// string manipulation.
///
/// # Examples
///
/// ```ignore
/// let path = RustPath::parse("crate::module::Type");
/// assert_eq!(path.kind(), RustPathKind::Crate);
/// assert_eq!(path.segments(), &["module", "Type"]);
/// assert_eq!(path.simple_name(), Some("Type"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RustPath {
    kind: RustPathKind,
    segments: Vec<String>,
}

impl RustPath {
    /// Parse a string path into a structured `RustPath`.
    ///
    /// Detects and handles:
    /// - `crate::rest` → `RustPathKind::Crate` with `["rest", ...]`
    /// - `self::rest` → `RustPathKind::SelfRelative` with `["rest", ...]`
    /// - `super::...::rest` → `RustPathKind::Super { levels }` with `["rest", ...]`
    /// - `external::rest` → `RustPathKind::External` with `["rest", ...]`
    /// - Everything else → `RustPathKind::Absolute`
    pub fn parse(path: &str) -> Self {
        if path.is_empty() {
            return Self {
                kind: RustPathKind::Absolute,
                segments: Vec::new(),
            };
        }

        // Check for crate:: prefix
        if let Some(rest) = path.strip_prefix("crate::") {
            return Self {
                kind: RustPathKind::Crate,
                segments: rest.split("::").map(String::from).collect(),
            };
        }

        // Check for self:: prefix
        if let Some(rest) = path.strip_prefix("self::") {
            return Self {
                kind: RustPathKind::SelfRelative,
                segments: rest.split("::").map(String::from).collect(),
            };
        }

        // Check for super:: prefix (may be chained)
        if path.starts_with("super::") {
            let mut remaining = path;
            let mut levels: u32 = 0;

            while let Some(rest) = remaining.strip_prefix("super::") {
                levels += 1;
                remaining = rest;
            }

            return Self {
                kind: RustPathKind::Super { levels },
                segments: remaining.split("::").map(String::from).collect(),
            };
        }

        // Check for external:: prefix
        if let Some(rest) = path.strip_prefix("external::") {
            return Self {
                kind: RustPathKind::External,
                segments: rest.split("::").map(String::from).collect(),
            };
        }

        // Default: absolute path
        Self {
            kind: RustPathKind::Absolute,
            segments: path.split("::").map(String::from).collect(),
        }
    }

    /// Create a builder for constructing paths.
    pub fn builder() -> RustPathBuilder {
        RustPathBuilder::new()
    }

    /// Get the path kind.
    pub fn kind(&self) -> RustPathKind {
        self.kind
    }

    /// Get the segments as a slice.
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Get the simple name (last segment).
    ///
    /// Returns `None` if the path has no segments.
    pub fn simple_name(&self) -> Option<&str> {
        self.segments.last().map(String::as_str)
    }

    /// Get the first segment.
    ///
    /// Returns `None` if the path has no segments.
    pub fn first_segment(&self) -> Option<&str> {
        self.segments.first().map(String::as_str)
    }

    /// Check if this is a relative path (crate/self/super).
    ///
    /// Relative paths need context (package name, current module) to resolve.
    pub fn is_relative(&self) -> bool {
        matches!(
            self.kind,
            RustPathKind::Crate | RustPathKind::SelfRelative | RustPathKind::Super { .. }
        )
    }

    /// Check if this path is qualified (has multiple segments).
    ///
    /// A qualified path contains `::` in its string form (e.g., `std::io::Read`).
    /// A simple name has only one segment (e.g., `Read`).
    pub fn is_qualified(&self) -> bool {
        self.segments.len() > 1
    }

    /// Check if this path refers to a known external crate.
    ///
    /// Returns true if:
    /// - The kind is `RustPathKind::External`
    /// - The first segment is a known external prefix (`std`, `core`, `alloc`, `external`)
    pub fn is_external(&self) -> bool {
        if matches!(self.kind, RustPathKind::External) {
            return true;
        }

        // Check if first segment is a known external crate
        if let Some(first) = self.first_segment() {
            return EXTERNAL_PREFIXES.contains(&first);
        }

        false
    }

    /// Convert to a qualified name string.
    ///
    /// This is the only method that produces a string from the segments.
    /// It reconstructs the path with the appropriate prefix based on kind.
    pub fn to_qualified_name(&self) -> String {
        let segments_str = self.segments.join("::");

        match self.kind {
            RustPathKind::Absolute => segments_str,
            RustPathKind::Crate => {
                if segments_str.is_empty() {
                    "crate".to_string()
                } else {
                    format!("crate::{segments_str}")
                }
            }
            RustPathKind::SelfRelative => {
                if segments_str.is_empty() {
                    "self".to_string()
                } else {
                    format!("self::{segments_str}")
                }
            }
            RustPathKind::Super { levels } => {
                let super_prefix = "super::".repeat(levels as usize);
                if segments_str.is_empty() {
                    // Remove trailing ::
                    super_prefix.trim_end_matches("::").to_string()
                } else {
                    format!("{super_prefix}{segments_str}")
                }
            }
            RustPathKind::External => {
                if segments_str.is_empty() {
                    "external".to_string()
                } else {
                    format!("external::{segments_str}")
                }
            }
        }
    }
}

impl fmt::Display for RustPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_qualified_name())
    }
}

impl From<&str> for RustPath {
    fn from(s: &str) -> Self {
        Self::parse(s)
    }
}

/// Builder for constructing `RustPath` instances.
///
/// Provides a fluent API for building paths without string manipulation.
///
/// # Examples
///
/// ```ignore
/// // Build an absolute path: mypackage::module::Type
/// let path = RustPath::builder()
///     .segment("mypackage")
///     .segment("module")
///     .segment("Type")
///     .build();
///
/// // Build with package prefix
/// let path = RustPath::builder()
///     .with_package("mypackage")
///     .segment("module")
///     .segment("Type")
///     .build();
/// ```
#[derive(Debug, Default)]
pub struct RustPathBuilder {
    kind: RustPathKind,
    segments: Vec<String>,
}

impl RustPathBuilder {
    /// Create a new builder with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the path kind.
    pub fn kind(mut self, kind: RustPathKind) -> Self {
        self.kind = kind;
        self
    }

    /// Add a single segment.
    pub fn segment(mut self, segment: impl Into<String>) -> Self {
        self.segments.push(segment.into());
        self
    }

    /// Add multiple segments.
    pub fn segments<I, S>(mut self, segments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.segments.extend(segments.into_iter().map(Into::into));
        self
    }

    /// Copy segments from an existing path.
    pub fn from_path(mut self, path: &RustPath) -> Self {
        self.segments.extend(path.segments.iter().cloned());
        self
    }

    /// Prepend a package name as the first segment.
    ///
    /// **Important:** This method also sets the kind to `Absolute`, overriding any
    /// previously set kind. This is intentional since prepending a package name
    /// produces a fully qualified path.
    pub fn with_package(mut self, package: &str) -> Self {
        if !package.is_empty() {
            self.segments.insert(0, package.to_string());
            self.kind = RustPathKind::Absolute;
        }
        self
    }

    /// Navigate up N levels from a module context.
    ///
    /// This takes the segments from `module`, removes the last `levels` segments,
    /// and adds them to this builder. Used for resolving `super::` paths.
    ///
    /// # Example
    ///
    /// If module is `["a", "b", "c"]` and levels is 2, this adds `["a"]`
    /// to the builder's segments (keeping `module.len() - levels` segments).
    pub fn navigate_up_from(mut self, module: &RustPath, levels: usize) -> Self {
        let module_segments = module.segments();
        if module_segments.len() > levels {
            let keep = module_segments.len() - levels;
            self.segments
                .extend(module_segments[..keep].iter().cloned());
        }
        // If levels >= module_segments.len(), we're at or beyond root,
        // so we add nothing from module
        self
    }

    /// Build the immutable `RustPath`.
    pub fn build(self) -> RustPath {
        RustPath {
            kind: self.kind,
            segments: self.segments,
        }
    }
}

/// Resolve a relative `RustPath` to an absolute path given context.
///
/// This is a standalone function that returns a new `RustPath` without
/// modifying the input. It handles all path kinds:
///
/// - `Absolute` / `External` → returned as-is (cloned)
/// - `Crate` → `package::segments`
/// - `SelfRelative` → `package::module::segments`
/// - `Super { levels }` → navigate up from module, then append segments
///
/// # Arguments
///
/// * `path` - The path to resolve
/// * `package` - The current package name (e.g., `"codesearch_core"`)
/// * `module` - The current module path as a `RustPath` (e.g., `"entities::error"`)
///
/// # Returns
///
/// A new `RustPath` with `Absolute` kind (or `External` if the input was external).
pub fn resolve_rust_path(
    path: &RustPath,
    package: Option<&str>,
    module: Option<&RustPath>,
) -> RustPath {
    match path.kind() {
        RustPathKind::Absolute | RustPathKind::External => path.clone(),

        RustPathKind::Crate => {
            // crate:: -> package::rest
            let mut builder = RustPath::builder().kind(RustPathKind::Absolute);

            if let Some(pkg) = package {
                if !pkg.is_empty() {
                    builder = builder.segment(pkg);
                }
            }

            builder.segments(path.segments().iter().cloned()).build()
        }

        RustPathKind::SelfRelative => {
            // self:: -> package::module::rest
            let mut builder = RustPath::builder().kind(RustPathKind::Absolute);

            if let Some(pkg) = package {
                if !pkg.is_empty() {
                    builder = builder.segment(pkg);
                }
            }

            if let Some(mod_path) = module {
                builder = builder.from_path(mod_path);
            }

            builder.segments(path.segments().iter().cloned()).build()
        }

        RustPathKind::Super { levels } => {
            // super:: -> navigate up from module, then append rest
            let mut builder = RustPath::builder().kind(RustPathKind::Absolute);

            if let Some(pkg) = package {
                if !pkg.is_empty() {
                    builder = builder.segment(pkg);
                }
            }

            if let Some(mod_path) = module {
                builder = builder.navigate_up_from(mod_path, levels as usize);
            }

            builder.segments(path.segments().iter().cloned()).build()
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ========================================================================
    // Tests for RustPath parsing
    // ========================================================================

    #[test]
    fn test_parse_absolute_path() {
        let path = RustPath::parse("std::collections::HashMap");
        assert_eq!(path.kind(), RustPathKind::Absolute);
        assert_eq!(path.segments(), &["std", "collections", "HashMap"]);
    }

    #[test]
    fn test_parse_simple_name() {
        let path = RustPath::parse("HashMap");
        assert_eq!(path.kind(), RustPathKind::Absolute);
        assert_eq!(path.segments(), &["HashMap"]);
    }

    #[test]
    fn test_parse_crate_path() {
        let path = RustPath::parse("crate::module::Type");
        assert_eq!(path.kind(), RustPathKind::Crate);
        assert_eq!(path.segments(), &["module", "Type"]);
    }

    #[test]
    fn test_parse_self_path() {
        let path = RustPath::parse("self::submodule::Helper");
        assert_eq!(path.kind(), RustPathKind::SelfRelative);
        assert_eq!(path.segments(), &["submodule", "Helper"]);
    }

    #[test]
    fn test_parse_super_path() {
        let path = RustPath::parse("super::sibling::Type");
        assert_eq!(path.kind(), RustPathKind::Super { levels: 1 });
        assert_eq!(path.segments(), &["sibling", "Type"]);
    }

    #[test]
    fn test_parse_chained_super_path() {
        let path = RustPath::parse("super::super::parent::Type");
        assert_eq!(path.kind(), RustPathKind::Super { levels: 2 });
        assert_eq!(path.segments(), &["parent", "Type"]);
    }

    #[test]
    fn test_parse_triple_super_path() {
        let path = RustPath::parse("super::super::super::ancestor::Type");
        assert_eq!(path.kind(), RustPathKind::Super { levels: 3 });
        assert_eq!(path.segments(), &["ancestor", "Type"]);
    }

    #[test]
    fn test_parse_external_path() {
        let path = RustPath::parse("external::serde::Serialize");
        assert_eq!(path.kind(), RustPathKind::External);
        assert_eq!(path.segments(), &["serde", "Serialize"]);
    }

    #[test]
    fn test_parse_empty_path() {
        let path = RustPath::parse("");
        assert_eq!(path.kind(), RustPathKind::Absolute);
        assert!(path.segments().is_empty());
    }

    // ========================================================================
    // Tests for RustPath accessors
    // ========================================================================

    #[test]
    fn test_simple_name() {
        assert_eq!(
            RustPath::parse("std::collections::HashMap").simple_name(),
            Some("HashMap")
        );
        assert_eq!(RustPath::parse("Type").simple_name(), Some("Type"));
        assert_eq!(RustPath::parse("").simple_name(), None);
    }

    #[test]
    fn test_first_segment() {
        assert_eq!(
            RustPath::parse("std::collections::HashMap").first_segment(),
            Some("std")
        );
        assert_eq!(RustPath::parse("Type").first_segment(), Some("Type"));
        assert_eq!(RustPath::parse("").first_segment(), None);
    }

    // ========================================================================
    // Tests for RustPath predicates
    // ========================================================================

    #[test]
    fn test_is_relative() {
        assert!(RustPath::parse("crate::module::Type").is_relative());
        assert!(RustPath::parse("self::helper").is_relative());
        assert!(RustPath::parse("super::sibling").is_relative());
        assert!(!RustPath::parse("std::io::Read").is_relative());
        assert!(!RustPath::parse("external::serde").is_relative());
    }

    #[test]
    fn test_is_qualified() {
        assert!(RustPath::parse("std::io::Read").is_qualified());
        assert!(RustPath::parse("a::b").is_qualified());
        assert!(!RustPath::parse("Read").is_qualified());
    }

    #[test]
    fn test_is_external() {
        assert!(RustPath::parse("std::io::Read").is_external());
        assert!(RustPath::parse("core::fmt::Display").is_external());
        assert!(RustPath::parse("alloc::vec::Vec").is_external());
        assert!(RustPath::parse("external::serde").is_external());
        assert!(!RustPath::parse("mypackage::module::Type").is_external());
        assert!(!RustPath::parse("crate::module::Type").is_external());
    }

    // ========================================================================
    // Tests for RustPath Display
    // ========================================================================

    #[test]
    fn test_display_absolute() {
        assert_eq!(
            RustPath::parse("std::collections::HashMap").to_string(),
            "std::collections::HashMap"
        );
    }

    #[test]
    fn test_display_crate() {
        assert_eq!(
            RustPath::parse("crate::module::Type").to_string(),
            "crate::module::Type"
        );
    }

    #[test]
    fn test_display_self() {
        assert_eq!(RustPath::parse("self::helper").to_string(), "self::helper");
    }

    #[test]
    fn test_display_super() {
        assert_eq!(
            RustPath::parse("super::sibling").to_string(),
            "super::sibling"
        );
        assert_eq!(
            RustPath::parse("super::super::parent").to_string(),
            "super::super::parent"
        );
    }

    #[test]
    fn test_display_external() {
        assert_eq!(
            RustPath::parse("external::serde").to_string(),
            "external::serde"
        );
    }

    #[test]
    fn test_roundtrip_parse_display() {
        let paths = [
            "std::collections::HashMap",
            "crate::module::Type",
            "self::helper::func",
            "super::sibling::Type",
            "super::super::parent::Type",
            "external::serde::Serialize",
            "SimpleType",
        ];

        for path_str in paths {
            let path = RustPath::parse(path_str);
            assert_eq!(
                path.to_string(),
                path_str,
                "Roundtrip failed for {path_str}"
            );
        }
    }

    // ========================================================================
    // Tests for RustPathBuilder
    // ========================================================================

    #[test]
    fn test_builder_basic() {
        let path = RustPath::builder()
            .segment("std")
            .segment("io")
            .segment("Read")
            .build();

        assert_eq!(path.kind(), RustPathKind::Absolute);
        assert_eq!(path.segments(), &["std", "io", "Read"]);
    }

    #[test]
    fn test_builder_with_kind() {
        let path = RustPath::builder()
            .kind(RustPathKind::Crate)
            .segment("module")
            .segment("Type")
            .build();

        assert_eq!(path.kind(), RustPathKind::Crate);
        assert_eq!(path.segments(), &["module", "Type"]);
    }

    #[test]
    fn test_builder_segments() {
        let path = RustPath::builder().segments(["a", "b", "c"]).build();

        assert_eq!(path.segments(), &["a", "b", "c"]);
    }

    #[test]
    fn test_builder_from_path() {
        let source = RustPath::parse("module::submodule");
        let path = RustPath::builder()
            .segment("package")
            .from_path(&source)
            .segment("Type")
            .build();

        assert_eq!(path.segments(), &["package", "module", "submodule", "Type"]);
    }

    #[test]
    fn test_builder_with_package() {
        let path = RustPath::builder()
            .segment("module")
            .segment("Type")
            .with_package("mypackage")
            .build();

        // with_package prepends the package and sets kind to Absolute
        assert_eq!(path.kind(), RustPathKind::Absolute);
        assert_eq!(path.segments(), &["mypackage", "module", "Type"]);
    }

    #[test]
    fn test_builder_with_empty_package() {
        let path = RustPath::builder()
            .segment("module")
            .segment("Type")
            .with_package("")
            .build();

        // Empty package should not change anything
        assert_eq!(path.segments(), &["module", "Type"]);
    }

    #[test]
    fn test_builder_navigate_up_from() {
        let module = RustPath::parse("a::b::c");

        // Navigate up 1 level: keep ["a", "b"]
        let path = RustPath::builder()
            .navigate_up_from(&module, 1)
            .segment("sibling")
            .build();

        assert_eq!(path.segments(), &["a", "b", "sibling"]);
    }

    #[test]
    fn test_builder_navigate_up_from_multiple_levels() {
        let module = RustPath::parse("a::b::c::d");

        // Navigate up 2 levels: keep ["a", "b"]
        let path = RustPath::builder()
            .navigate_up_from(&module, 2)
            .segment("cousin")
            .build();

        assert_eq!(path.segments(), &["a", "b", "cousin"]);
    }

    #[test]
    fn test_builder_navigate_up_exceeds_depth() {
        let module = RustPath::parse("a::b");

        // Navigate up 3 levels from 2-segment module: nothing kept from module
        let path = RustPath::builder()
            .segment("package")
            .navigate_up_from(&module, 3)
            .segment("root")
            .build();

        // Only package and root, nothing from module
        assert_eq!(path.segments(), &["package", "root"]);
    }

    // ========================================================================
    // Tests for resolve_rust_path
    // ========================================================================

    #[test]
    fn test_resolve_absolute_unchanged() {
        let path = RustPath::parse("std::io::Read");
        let resolved = resolve_rust_path(&path, Some("pkg"), None);

        assert_eq!(resolved.kind(), RustPathKind::Absolute);
        assert_eq!(resolved.to_string(), "std::io::Read");
    }

    #[test]
    fn test_resolve_external_unchanged() {
        let path = RustPath::parse("external::serde::Serialize");
        let resolved = resolve_rust_path(&path, Some("pkg"), None);

        assert_eq!(resolved.kind(), RustPathKind::External);
        assert_eq!(resolved.to_string(), "external::serde::Serialize");
    }

    #[test]
    fn test_resolve_crate_with_package() {
        let path = RustPath::parse("crate::module::Type");
        let resolved = resolve_rust_path(&path, Some("mypackage"), None);

        assert_eq!(resolved.kind(), RustPathKind::Absolute);
        assert_eq!(resolved.to_string(), "mypackage::module::Type");
    }

    #[test]
    fn test_resolve_crate_without_package() {
        let path = RustPath::parse("crate::module::Type");
        let resolved = resolve_rust_path(&path, None, None);

        assert_eq!(resolved.to_string(), "module::Type");
    }

    #[test]
    fn test_resolve_self_with_module() {
        let path = RustPath::parse("self::helper");
        let module = RustPath::parse("utils::network");
        let resolved = resolve_rust_path(&path, Some("mypackage"), Some(&module));

        assert_eq!(resolved.to_string(), "mypackage::utils::network::helper");
    }

    #[test]
    fn test_resolve_self_without_module() {
        let path = RustPath::parse("self::helper");
        let resolved = resolve_rust_path(&path, Some("mypackage"), None);

        assert_eq!(resolved.to_string(), "mypackage::helper");
    }

    #[test]
    fn test_resolve_super_with_module() {
        let path = RustPath::parse("super::sibling");
        let module = RustPath::parse("utils::network");
        let resolved = resolve_rust_path(&path, Some("mypackage"), Some(&module));

        // Navigate up 1 from ["utils", "network"] -> ["utils"]
        assert_eq!(resolved.to_string(), "mypackage::utils::sibling");
    }

    #[test]
    fn test_resolve_chained_super() {
        let path = RustPath::parse("super::super::ancestor");
        let module = RustPath::parse("a::b::c");
        let resolved = resolve_rust_path(&path, Some("pkg"), Some(&module));

        // Navigate up 2 from ["a", "b", "c"] -> ["a"]
        assert_eq!(resolved.to_string(), "pkg::a::ancestor");
    }

    #[test]
    fn test_resolve_super_exceeds_depth() {
        let path = RustPath::parse("super::super::super::root");
        let module = RustPath::parse("a::b");
        let resolved = resolve_rust_path(&path, Some("pkg"), Some(&module));

        // Navigate up 3 from 2-segment module -> nothing from module
        assert_eq!(resolved.to_string(), "pkg::root");
    }

    // ========================================================================
    // Edge case tests (added per PR review)
    // ========================================================================

    #[test]
    fn test_builder_navigate_up_from_zero_levels() {
        let module = RustPath::parse("a::b::c");

        // Navigate up 0 levels: keep all segments
        let path = RustPath::builder()
            .navigate_up_from(&module, 0)
            .segment("child")
            .build();

        assert_eq!(path.segments(), &["a", "b", "c", "child"]);
    }

    #[test]
    fn test_parse_bare_crate_keyword() {
        // Just "crate" without :: should parse as a single-segment Absolute path
        let path = RustPath::parse("crate");
        assert_eq!(path.kind(), RustPathKind::Absolute);
        assert_eq!(path.segments(), &["crate"]);
    }

    #[test]
    fn test_parse_bare_self_keyword() {
        // Just "self" without :: should parse as a single-segment Absolute path
        let path = RustPath::parse("self");
        assert_eq!(path.kind(), RustPathKind::Absolute);
        assert_eq!(path.segments(), &["self"]);
    }

    #[test]
    fn test_parse_bare_super_keyword() {
        // Just "super" without :: should parse as a single-segment Absolute path
        let path = RustPath::parse("super");
        assert_eq!(path.kind(), RustPathKind::Absolute);
        assert_eq!(path.segments(), &["super"]);
    }

    #[test]
    fn test_to_qualified_name_empty_segments() {
        // Builder can produce empty segments - test to_qualified_name handles it
        let path = RustPath::builder().kind(RustPathKind::Crate).build();
        assert_eq!(path.to_qualified_name(), "crate");

        let path = RustPath::builder().kind(RustPathKind::SelfRelative).build();
        assert_eq!(path.to_qualified_name(), "self");

        let path = RustPath::builder()
            .kind(RustPathKind::Super { levels: 2 })
            .build();
        assert_eq!(path.to_qualified_name(), "super::super");

        let path = RustPath::builder().kind(RustPathKind::External).build();
        assert_eq!(path.to_qualified_name(), "external");

        let path = RustPath::builder().kind(RustPathKind::Absolute).build();
        assert_eq!(path.to_qualified_name(), "");
    }
}
