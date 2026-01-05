//! Language-agnostic path representation
//!
//! This module provides `LanguagePath`, a generic type for representing
//! paths in any programming language. It replaces the Rust-specific
//! `RustPath` type with a configurable, language-agnostic alternative.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use super::path_config::{PathConfig, RelativeSemantics};
use std::fmt;
use tracing::trace;

/// The kind of a language path, indicating how it should be resolved
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum PathKind {
    /// Absolute path or simple name
    /// Used for fully qualified paths (e.g., `std::collections::HashMap`)
    /// or simple names without prefixes (e.g., `HashMap`)
    #[default]
    Absolute,

    /// Relative path with semantic meaning
    Relative {
        /// The original prefix string (e.g., "crate::", "self::", "super::")
        prefix: String,
        /// The semantic meaning of this prefix
        semantics: RelativeSemantics,
    },

    /// External/third-party reference
    /// Used for references that are known to be outside the current package
    External,
}

/// A language-agnostic path representation
///
/// Instead of storing paths as strings with separators, this type stores
/// the path kind and segments separately, enabling type-safe operations
/// without string manipulation.
///
/// # Examples
///
/// ```ignore
/// use crate::common::path_config::RUST_PATH_CONFIG;
///
/// let path = LanguagePath::parse("crate::module::Type", &RUST_PATH_CONFIG);
/// assert!(matches!(path.kind(), PathKind::Relative { .. }));
/// assert_eq!(path.segments(), &["module", "Type"]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LanguagePath {
    kind: PathKind,
    segments: Vec<String>,
    separator: &'static str,
}

impl LanguagePath {
    /// Parse a string path into a structured `LanguagePath` using the given configuration.
    ///
    /// The parser:
    /// 1. Checks for relative prefixes in order of configuration
    /// 2. Handles chainable prefixes (e.g., `super::super::`)
    /// 3. Checks for external prefixes in the first segment
    /// 4. Falls back to absolute path
    pub fn parse(path: &str, config: &PathConfig) -> Self {
        if path.is_empty() {
            return Self {
                kind: PathKind::Absolute,
                segments: Vec::new(),
                separator: config.separator,
            };
        }

        // Check for relative prefixes
        for rel_prefix in config.relative_prefixes {
            if let Some(stripped) = path.strip_prefix(rel_prefix.prefix) {
                if rel_prefix.chainable {
                    // Count consecutive occurrences (already counted first one)
                    let mut remaining = stripped;
                    let mut count: u32 = 1;

                    while let Some(rest) = remaining.strip_prefix(rel_prefix.prefix) {
                        count += 1;
                        remaining = rest;
                    }

                    // Update semantics with actual level count for Parent
                    let semantics = match rel_prefix.semantics {
                        RelativeSemantics::Parent { .. } => {
                            RelativeSemantics::Parent { levels: count }
                        }
                        other => other,
                    };

                    return Self {
                        kind: PathKind::Relative {
                            prefix: rel_prefix.prefix.repeat(count as usize),
                            semantics,
                        },
                        segments: Self::split_segments(remaining, config.separator),
                        separator: config.separator,
                    };
                } else {
                    // Non-chainable prefix
                    return Self {
                        kind: PathKind::Relative {
                            prefix: rel_prefix.prefix.to_string(),
                            semantics: rel_prefix.semantics,
                        },
                        segments: Self::split_segments(stripped, config.separator),
                        separator: config.separator,
                    };
                }
            }
        }

        // Check for "external" prefix (project convention for marking external dependencies)
        // Uses language-specific separator: "external::foo" for Rust, "external.foo" for Python
        let external_prefix = format!("external{}", config.separator);
        if let Some(rest) = path.strip_prefix(&external_prefix) {
            return Self {
                kind: PathKind::External,
                segments: Self::split_segments(rest, config.separator),
                separator: config.separator,
            };
        }

        // Check if first segment is a known external prefix
        let segments = Self::split_segments(path, config.separator);
        if let Some(first) = segments.first() {
            if config.external_prefixes.contains(&first.as_str()) {
                return Self {
                    kind: PathKind::External,
                    segments,
                    separator: config.separator,
                };
            }
        }

        // Default: absolute path
        Self {
            kind: PathKind::Absolute,
            segments,
            separator: config.separator,
        }
    }

    /// Split a path string into segments using the separator
    fn split_segments(path: &str, separator: &str) -> Vec<String> {
        if path.is_empty() {
            Vec::new()
        } else {
            path.split(separator).map(String::from).collect()
        }
    }

    /// Create a builder for constructing paths
    pub fn builder(config: &'static PathConfig) -> LanguagePathBuilder {
        LanguagePathBuilder::new(config)
    }

    /// Get the path kind
    pub fn kind(&self) -> &PathKind {
        &self.kind
    }

    /// Get the segments as a slice
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Get the separator used by this path
    pub fn separator(&self) -> &'static str {
        self.separator
    }

    /// Get the simple name (last segment)
    ///
    /// Returns `None` if the path has no segments.
    pub fn simple_name(&self) -> Option<&str> {
        self.segments.last().map(String::as_str)
    }

    /// Get the first segment
    ///
    /// Returns `None` if the path has no segments.
    pub fn first_segment(&self) -> Option<&str> {
        self.segments.first().map(String::as_str)
    }

    /// Check if this is a relative path
    ///
    /// Relative paths need context (package name, current module) to resolve.
    pub fn is_relative(&self) -> bool {
        matches!(self.kind, PathKind::Relative { .. })
    }

    /// Check if this path is qualified (has multiple segments)
    ///
    /// A qualified path contains the separator in its string form.
    pub fn is_qualified(&self) -> bool {
        self.segments.len() > 1
    }

    /// Check if this path refers to an external reference
    pub fn is_external(&self) -> bool {
        matches!(self.kind, PathKind::External)
    }

    /// Convert to a qualified name string
    ///
    /// This reconstructs the path with the appropriate prefix based on kind.
    pub fn to_qualified_name(&self) -> String {
        let segments_str = self.segments.join(self.separator);

        match &self.kind {
            PathKind::Absolute => segments_str,
            PathKind::Relative { prefix, .. } => {
                if segments_str.is_empty() {
                    // Remove trailing separator from prefix
                    prefix.trim_end_matches(self.separator).to_string()
                } else {
                    format!("{prefix}{segments_str}")
                }
            }
            PathKind::External => {
                if segments_str.is_empty() {
                    "external".to_string()
                } else {
                    format!("external{}{segments_str}", self.separator)
                }
            }
        }
    }

    /// Resolve this path to an absolute path given context
    ///
    /// # Arguments
    /// * `package` - The current package name (e.g., "codesearch_core")
    /// * `module` - The current module path (e.g., "entities::error")
    /// * `config` - Path configuration for the target language
    ///
    /// # Returns
    /// A new `LanguagePath` with `Absolute` kind (or `External` if the input was external).
    pub fn resolve(
        &self,
        package: Option<&str>,
        module: Option<&LanguagePath>,
        config: &'static PathConfig,
    ) -> LanguagePath {
        match &self.kind {
            PathKind::Absolute | PathKind::External => self.clone(),

            PathKind::Relative { semantics, .. } => match semantics {
                RelativeSemantics::Root => {
                    // crate:: -> package::rest
                    let mut builder = LanguagePath::builder(config).kind(PathKind::Absolute);

                    if let Some(pkg) = package {
                        if !pkg.is_empty() {
                            builder = builder.segment(pkg);
                        }
                    }

                    builder.segments(self.segments.iter().cloned()).build()
                }

                RelativeSemantics::Current => {
                    // self:: -> package::module::rest
                    let mut builder = LanguagePath::builder(config).kind(PathKind::Absolute);

                    if let Some(pkg) = package {
                        if !pkg.is_empty() {
                            builder = builder.segment(pkg);
                        }
                    }

                    if let Some(mod_path) = module {
                        builder = builder.from_path(mod_path);
                    }

                    builder.segments(self.segments.iter().cloned()).build()
                }

                RelativeSemantics::Parent { levels } => {
                    // super:: -> navigate up from module, then append rest
                    let mut builder = LanguagePath::builder(config).kind(PathKind::Absolute);

                    if let Some(pkg) = package {
                        if !pkg.is_empty() {
                            builder = builder.segment(pkg);
                        }
                    }

                    if let Some(mod_path) = module {
                        builder = builder.navigate_up_from(mod_path, *levels as usize);
                    }

                    builder.segments(self.segments.iter().cloned()).build()
                }
            },
        }
    }
}

impl fmt::Display for LanguagePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_qualified_name())
    }
}

/// Builder for constructing `LanguagePath` instances
///
/// Provides a fluent API for building paths without string manipulation.
#[derive(Debug)]
pub struct LanguagePathBuilder {
    kind: PathKind,
    segments: Vec<String>,
    separator: &'static str,
}

impl LanguagePathBuilder {
    /// Create a new builder with the given configuration
    pub fn new(config: &'static PathConfig) -> Self {
        Self {
            kind: PathKind::Absolute,
            segments: Vec::new(),
            separator: config.separator,
        }
    }

    /// Set the path kind
    pub fn kind(mut self, kind: PathKind) -> Self {
        self.kind = kind;
        self
    }

    /// Add a single segment
    pub fn segment(mut self, segment: impl Into<String>) -> Self {
        self.segments.push(segment.into());
        self
    }

    /// Add multiple segments
    pub fn segments<I, S>(mut self, segments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.segments.extend(segments.into_iter().map(Into::into));
        self
    }

    /// Copy segments from an existing path
    pub fn from_path(mut self, path: &LanguagePath) -> Self {
        self.segments.extend(path.segments.iter().cloned());
        self
    }

    /// Prepend a package name as the first segment
    ///
    /// This also sets the kind to `Absolute`.
    pub fn with_package(mut self, package: &str) -> Self {
        if !package.is_empty() {
            self.segments.insert(0, package.to_string());
            self.kind = PathKind::Absolute;
        }
        self
    }

    /// Navigate up N levels from a module context
    ///
    /// Takes segments from `module`, removes the last `levels` segments,
    /// and adds the remaining to this builder.
    ///
    /// If `levels` exceeds or equals the module depth, no segments are added
    /// and a trace log is emitted (this may indicate an issue with super:: chains).
    pub fn navigate_up_from(mut self, module: &LanguagePath, levels: usize) -> Self {
        let module_segments = module.segments();
        if module_segments.len() > levels {
            let keep = module_segments.len() - levels;
            self.segments
                .extend(module_segments[..keep].iter().cloned());
        } else {
            trace!(
                module = module.to_qualified_name(),
                levels = levels,
                module_depth = module_segments.len(),
                "super:: chain exceeds module depth, no parent segments added"
            );
        }
        self
    }

    /// Build the immutable `LanguagePath`
    pub fn build(self) -> LanguagePath {
        LanguagePath {
            kind: self.kind,
            segments: self.segments,
            separator: self.separator,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::common::path_config::RUST_PATH_CONFIG;

    // ========================================================================
    // Tests for LanguagePath parsing with Rust config
    // ========================================================================

    #[test]
    fn test_parse_absolute_path() {
        let path = LanguagePath::parse("std::collections::HashMap", &RUST_PATH_CONFIG);
        // std:: is in external_prefixes, so it should be External
        assert!(matches!(path.kind(), PathKind::External));
        assert_eq!(path.segments(), &["std", "collections", "HashMap"]);
    }

    #[test]
    fn test_parse_simple_name() {
        let path = LanguagePath::parse("HashMap", &RUST_PATH_CONFIG);
        assert!(matches!(path.kind(), PathKind::Absolute));
        assert_eq!(path.segments(), &["HashMap"]);
    }

    #[test]
    fn test_parse_crate_path() {
        let path = LanguagePath::parse("crate::module::Type", &RUST_PATH_CONFIG);
        assert!(matches!(
            path.kind(),
            PathKind::Relative {
                semantics: RelativeSemantics::Root,
                ..
            }
        ));
        assert_eq!(path.segments(), &["module", "Type"]);
    }

    #[test]
    fn test_parse_self_path() {
        let path = LanguagePath::parse("self::submodule::Helper", &RUST_PATH_CONFIG);
        assert!(matches!(
            path.kind(),
            PathKind::Relative {
                semantics: RelativeSemantics::Current,
                ..
            }
        ));
        assert_eq!(path.segments(), &["submodule", "Helper"]);
    }

    #[test]
    fn test_parse_super_path() {
        let path = LanguagePath::parse("super::sibling::Type", &RUST_PATH_CONFIG);
        assert!(matches!(
            path.kind(),
            PathKind::Relative {
                semantics: RelativeSemantics::Parent { levels: 1 },
                ..
            }
        ));
        assert_eq!(path.segments(), &["sibling", "Type"]);
    }

    #[test]
    fn test_parse_chained_super_path() {
        let path = LanguagePath::parse("super::super::parent::Type", &RUST_PATH_CONFIG);
        assert!(matches!(
            path.kind(),
            PathKind::Relative {
                semantics: RelativeSemantics::Parent { levels: 2 },
                ..
            }
        ));
        assert_eq!(path.segments(), &["parent", "Type"]);
    }

    #[test]
    fn test_parse_triple_super_path() {
        let path = LanguagePath::parse("super::super::super::ancestor::Type", &RUST_PATH_CONFIG);
        assert!(matches!(
            path.kind(),
            PathKind::Relative {
                semantics: RelativeSemantics::Parent { levels: 3 },
                ..
            }
        ));
        assert_eq!(path.segments(), &["ancestor", "Type"]);
    }

    #[test]
    fn test_parse_external_path() {
        let path = LanguagePath::parse("external::serde::Serialize", &RUST_PATH_CONFIG);
        assert!(matches!(path.kind(), PathKind::External));
        assert_eq!(path.segments(), &["serde", "Serialize"]);
    }

    #[test]
    fn test_parse_empty_path() {
        let path = LanguagePath::parse("", &RUST_PATH_CONFIG);
        assert!(matches!(path.kind(), PathKind::Absolute));
        assert!(path.segments().is_empty());
    }

    // ========================================================================
    // Tests for LanguagePath accessors
    // ========================================================================

    #[test]
    fn test_simple_name() {
        // std is in external_prefixes
        let path = LanguagePath::parse("std::collections::HashMap", &RUST_PATH_CONFIG);
        assert_eq!(path.simple_name(), Some("HashMap"));

        let path = LanguagePath::parse("Type", &RUST_PATH_CONFIG);
        assert_eq!(path.simple_name(), Some("Type"));

        let path = LanguagePath::parse("", &RUST_PATH_CONFIG);
        assert_eq!(path.simple_name(), None);
    }

    #[test]
    fn test_first_segment() {
        let path = LanguagePath::parse("std::collections::HashMap", &RUST_PATH_CONFIG);
        assert_eq!(path.first_segment(), Some("std"));

        let path = LanguagePath::parse("Type", &RUST_PATH_CONFIG);
        assert_eq!(path.first_segment(), Some("Type"));

        let path = LanguagePath::parse("", &RUST_PATH_CONFIG);
        assert_eq!(path.first_segment(), None);
    }

    // ========================================================================
    // Tests for LanguagePath predicates
    // ========================================================================

    #[test]
    fn test_is_relative() {
        assert!(LanguagePath::parse("crate::module::Type", &RUST_PATH_CONFIG).is_relative());
        assert!(LanguagePath::parse("self::helper", &RUST_PATH_CONFIG).is_relative());
        assert!(LanguagePath::parse("super::sibling", &RUST_PATH_CONFIG).is_relative());
        assert!(!LanguagePath::parse("std::io::Read", &RUST_PATH_CONFIG).is_relative());
        assert!(!LanguagePath::parse("external::serde", &RUST_PATH_CONFIG).is_relative());
    }

    #[test]
    fn test_is_qualified() {
        assert!(LanguagePath::parse("std::io::Read", &RUST_PATH_CONFIG).is_qualified());
        assert!(LanguagePath::parse("a::b", &RUST_PATH_CONFIG).is_qualified());
        assert!(!LanguagePath::parse("Read", &RUST_PATH_CONFIG).is_qualified());
    }

    #[test]
    fn test_is_external() {
        assert!(LanguagePath::parse("std::io::Read", &RUST_PATH_CONFIG).is_external());
        assert!(LanguagePath::parse("core::fmt::Display", &RUST_PATH_CONFIG).is_external());
        assert!(LanguagePath::parse("alloc::vec::Vec", &RUST_PATH_CONFIG).is_external());
        assert!(LanguagePath::parse("external::serde", &RUST_PATH_CONFIG).is_external());
        assert!(!LanguagePath::parse("mypackage::module::Type", &RUST_PATH_CONFIG).is_external());
        assert!(!LanguagePath::parse("crate::module::Type", &RUST_PATH_CONFIG).is_external());
    }

    // ========================================================================
    // Tests for LanguagePath Display
    // ========================================================================

    #[test]
    fn test_display_absolute() {
        let path = LanguagePath::parse("mypackage::module::Type", &RUST_PATH_CONFIG);
        assert_eq!(path.to_string(), "mypackage::module::Type");
    }

    #[test]
    fn test_display_crate() {
        let path = LanguagePath::parse("crate::module::Type", &RUST_PATH_CONFIG);
        assert_eq!(path.to_string(), "crate::module::Type");
    }

    #[test]
    fn test_display_self() {
        let path = LanguagePath::parse("self::helper", &RUST_PATH_CONFIG);
        assert_eq!(path.to_string(), "self::helper");
    }

    #[test]
    fn test_display_super() {
        let path = LanguagePath::parse("super::sibling", &RUST_PATH_CONFIG);
        assert_eq!(path.to_string(), "super::sibling");

        let path = LanguagePath::parse("super::super::parent", &RUST_PATH_CONFIG);
        assert_eq!(path.to_string(), "super::super::parent");
    }

    #[test]
    fn test_display_external() {
        let path = LanguagePath::parse("external::serde", &RUST_PATH_CONFIG);
        assert_eq!(path.to_string(), "external::serde");
    }

    #[test]
    fn test_roundtrip_parse_display() {
        // Note: std::collections::HashMap parses as External, so its display is unchanged
        let paths = [
            "mypackage::module::Type",
            "crate::module::Type",
            "self::helper::func",
            "super::sibling::Type",
            "super::super::parent::Type",
            "external::serde::Serialize",
            "SimpleType",
        ];

        for path_str in paths {
            let path = LanguagePath::parse(path_str, &RUST_PATH_CONFIG);
            assert_eq!(
                path.to_string(),
                path_str,
                "Roundtrip failed for {path_str}"
            );
        }
    }

    // ========================================================================
    // Tests for LanguagePathBuilder
    // ========================================================================

    #[test]
    fn test_builder_basic() {
        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .segment("mypackage")
            .segment("io")
            .segment("Read")
            .build();

        assert!(matches!(path.kind(), PathKind::Absolute));
        assert_eq!(path.segments(), &["mypackage", "io", "Read"]);
    }

    #[test]
    fn test_builder_with_kind() {
        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .kind(PathKind::External)
            .segment("serde")
            .segment("Serialize")
            .build();

        assert!(matches!(path.kind(), PathKind::External));
        assert_eq!(path.segments(), &["serde", "Serialize"]);
    }

    #[test]
    fn test_builder_segments() {
        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .segments(["a", "b", "c"])
            .build();

        assert_eq!(path.segments(), &["a", "b", "c"]);
    }

    #[test]
    fn test_builder_from_path() {
        let source = LanguagePath::parse("module::submodule", &RUST_PATH_CONFIG);
        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .segment("package")
            .from_path(&source)
            .segment("Type")
            .build();

        assert_eq!(path.segments(), &["package", "module", "submodule", "Type"]);
    }

    #[test]
    fn test_builder_with_package() {
        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .segment("module")
            .segment("Type")
            .with_package("mypackage")
            .build();

        assert!(matches!(path.kind(), PathKind::Absolute));
        assert_eq!(path.segments(), &["mypackage", "module", "Type"]);
    }

    #[test]
    fn test_builder_with_empty_package() {
        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .segment("module")
            .segment("Type")
            .with_package("")
            .build();

        assert_eq!(path.segments(), &["module", "Type"]);
    }

    #[test]
    fn test_builder_navigate_up_from() {
        let module = LanguagePath::parse("a::b::c", &RUST_PATH_CONFIG);

        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .navigate_up_from(&module, 1)
            .segment("sibling")
            .build();

        assert_eq!(path.segments(), &["a", "b", "sibling"]);
    }

    #[test]
    fn test_builder_navigate_up_from_multiple_levels() {
        let module = LanguagePath::parse("a::b::c::d", &RUST_PATH_CONFIG);

        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .navigate_up_from(&module, 2)
            .segment("cousin")
            .build();

        assert_eq!(path.segments(), &["a", "b", "cousin"]);
    }

    #[test]
    fn test_builder_navigate_up_exceeds_depth() {
        let module = LanguagePath::parse("a::b", &RUST_PATH_CONFIG);

        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .segment("package")
            .navigate_up_from(&module, 3)
            .segment("root")
            .build();

        assert_eq!(path.segments(), &["package", "root"]);
    }

    // ========================================================================
    // Tests for resolve
    // ========================================================================

    #[test]
    fn test_resolve_absolute_unchanged() {
        let path = LanguagePath::parse("mypackage::io::Read", &RUST_PATH_CONFIG);
        let resolved = path.resolve(Some("pkg"), None, &RUST_PATH_CONFIG);

        assert!(matches!(resolved.kind(), PathKind::Absolute));
        assert_eq!(resolved.to_string(), "mypackage::io::Read");
    }

    #[test]
    fn test_resolve_external_unchanged() {
        let path = LanguagePath::parse("external::serde::Serialize", &RUST_PATH_CONFIG);
        let resolved = path.resolve(Some("pkg"), None, &RUST_PATH_CONFIG);

        assert!(matches!(resolved.kind(), PathKind::External));
        assert_eq!(resolved.to_string(), "external::serde::Serialize");
    }

    #[test]
    fn test_resolve_crate_with_package() {
        let path = LanguagePath::parse("crate::module::Type", &RUST_PATH_CONFIG);
        let resolved = path.resolve(Some("mypackage"), None, &RUST_PATH_CONFIG);

        assert!(matches!(resolved.kind(), PathKind::Absolute));
        assert_eq!(resolved.to_string(), "mypackage::module::Type");
    }

    #[test]
    fn test_resolve_crate_without_package() {
        let path = LanguagePath::parse("crate::module::Type", &RUST_PATH_CONFIG);
        let resolved = path.resolve(None, None, &RUST_PATH_CONFIG);

        assert_eq!(resolved.to_string(), "module::Type");
    }

    #[test]
    fn test_resolve_self_with_module() {
        let path = LanguagePath::parse("self::helper", &RUST_PATH_CONFIG);
        let module = LanguagePath::parse("utils::network", &RUST_PATH_CONFIG);
        let resolved = path.resolve(Some("mypackage"), Some(&module), &RUST_PATH_CONFIG);

        assert_eq!(resolved.to_string(), "mypackage::utils::network::helper");
    }

    #[test]
    fn test_resolve_self_without_module() {
        let path = LanguagePath::parse("self::helper", &RUST_PATH_CONFIG);
        let resolved = path.resolve(Some("mypackage"), None, &RUST_PATH_CONFIG);

        assert_eq!(resolved.to_string(), "mypackage::helper");
    }

    #[test]
    fn test_resolve_super_with_module() {
        let path = LanguagePath::parse("super::sibling", &RUST_PATH_CONFIG);
        let module = LanguagePath::parse("utils::network", &RUST_PATH_CONFIG);
        let resolved = path.resolve(Some("mypackage"), Some(&module), &RUST_PATH_CONFIG);

        assert_eq!(resolved.to_string(), "mypackage::utils::sibling");
    }

    #[test]
    fn test_resolve_chained_super() {
        let path = LanguagePath::parse("super::super::ancestor", &RUST_PATH_CONFIG);
        let module = LanguagePath::parse("a::b::c", &RUST_PATH_CONFIG);
        let resolved = path.resolve(Some("pkg"), Some(&module), &RUST_PATH_CONFIG);

        assert_eq!(resolved.to_string(), "pkg::a::ancestor");
    }

    #[test]
    fn test_resolve_super_exceeds_depth() {
        let path = LanguagePath::parse("super::super::super::root", &RUST_PATH_CONFIG);
        let module = LanguagePath::parse("a::b", &RUST_PATH_CONFIG);
        let resolved = path.resolve(Some("pkg"), Some(&module), &RUST_PATH_CONFIG);

        assert_eq!(resolved.to_string(), "pkg::root");
    }

    // ========================================================================
    // Edge case tests
    // ========================================================================

    #[test]
    fn test_builder_navigate_up_from_zero_levels() {
        let module = LanguagePath::parse("a::b::c", &RUST_PATH_CONFIG);

        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .navigate_up_from(&module, 0)
            .segment("child")
            .build();

        assert_eq!(path.segments(), &["a", "b", "c", "child"]);
    }

    #[test]
    fn test_parse_bare_crate_keyword() {
        // Just "crate" without :: should parse as a single-segment Absolute path
        let path = LanguagePath::parse("crate", &RUST_PATH_CONFIG);
        assert!(matches!(path.kind(), PathKind::Absolute));
        assert_eq!(path.segments(), &["crate"]);
    }

    #[test]
    fn test_parse_bare_self_keyword() {
        // Just "self" without :: should parse as a single-segment Absolute path
        let path = LanguagePath::parse("self", &RUST_PATH_CONFIG);
        assert!(matches!(path.kind(), PathKind::Absolute));
        assert_eq!(path.segments(), &["self"]);
    }

    #[test]
    fn test_parse_bare_super_keyword() {
        // Just "super" without :: should parse as a single-segment Absolute path
        let path = LanguagePath::parse("super", &RUST_PATH_CONFIG);
        assert!(matches!(path.kind(), PathKind::Absolute));
        assert_eq!(path.segments(), &["super"]);
    }

    #[test]
    fn test_to_qualified_name_empty_segments() {
        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .kind(PathKind::Relative {
                prefix: "crate::".to_string(),
                semantics: RelativeSemantics::Root,
            })
            .build();
        assert_eq!(path.to_qualified_name(), "crate");

        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .kind(PathKind::Relative {
                prefix: "self::".to_string(),
                semantics: RelativeSemantics::Current,
            })
            .build();
        assert_eq!(path.to_qualified_name(), "self");

        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .kind(PathKind::Relative {
                prefix: "super::super::".to_string(),
                semantics: RelativeSemantics::Parent { levels: 2 },
            })
            .build();
        assert_eq!(path.to_qualified_name(), "super::super");

        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .kind(PathKind::External)
            .build();
        assert_eq!(path.to_qualified_name(), "external");

        let path = LanguagePath::builder(&RUST_PATH_CONFIG)
            .kind(PathKind::Absolute)
            .build();
        assert_eq!(path.to_qualified_name(), "");
    }
}
