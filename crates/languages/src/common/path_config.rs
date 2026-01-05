//! Configuration for language-specific path parsing
//!
//! This module provides declarative configuration for how paths are parsed
//! and resolved in different programming languages.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

/// Semantic meaning of a relative prefix
///
/// Different languages use different syntax for relative paths:
/// - Rust: `crate::`, `self::`, `super::`
/// - Python: `.`, `..`
/// - JavaScript: `./`, `../`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelativeSemantics {
    /// Package/crate/module root
    /// - Rust: `crate::`
    Root,

    /// Current scope/module
    /// - Rust: `self::`
    /// - Python: `.` (single dot in relative import)
    Current,

    /// Parent scope with level count
    /// - Rust: `super::` (levels = 1), `super::super::` (levels = 2)
    /// - Python: `..` (levels = 1), `...` (levels = 2)
    Parent { levels: u32 },
}

/// A relative prefix pattern and its semantics
#[derive(Debug)]
pub struct RelativePrefix {
    /// The prefix string to match (e.g., "crate::", "self::", "./")
    pub prefix: &'static str,

    /// The semantic meaning when this prefix is matched
    pub semantics: RelativeSemantics,

    /// Whether this prefix can be chained (e.g., "super::super::")
    /// When true, the parser will count consecutive occurrences
    pub chainable: bool,
}

/// Configuration for parsing paths in a specific language
///
/// This struct is designed to be created as a static constant in each
/// language module, providing all the information needed to parse and
/// resolve paths without language-specific code.
///
/// # Field Ordering and Consistency
///
/// - **`relative_prefixes`**: Order matters! The parser tries prefixes in order and uses
///   the first match. Put longer/more-specific prefixes before shorter ones to avoid
///   incorrect matches. For example, if you have both `"self::"` and `"self::super::"`,
///   list `"self::super::"` first.
///
/// - **`separator` consistency**: All prefixes in `relative_prefixes` should end with
///   the `separator` (e.g., `"crate::"` not `"crate"` for Rust). The `external_prefixes`
///   should NOT include the separator (e.g., `"std"` not `"std::"`).
///
/// - **`external_prefixes`**: These are matched against the first segment of a path,
///   so they should be crate/package names without separators.
#[derive(Debug)]
pub struct PathConfig {
    /// Path separator ("::" for Rust, "." for Python/JS)
    pub separator: &'static str,

    /// Relative prefix mappings, ordered by match priority
    ///
    /// The parser tries prefixes in order and uses the first match.
    /// **Order matters**: Put longer/more-specific prefixes before shorter ones.
    /// All prefixes should include the trailing separator (e.g., `"crate::"` not `"crate"`).
    pub relative_prefixes: &'static [RelativePrefix],

    /// Known external/third-party prefixes (without separator)
    ///
    /// These are matched against the first segment of a path to determine
    /// if a reference is external (stdlib, third-party crate, etc.).
    /// - Rust: `["std", "core", "alloc", "external"]`
    /// - Python: `[]` (external detection uses different heuristics)
    pub external_prefixes: &'static [&'static str],
}

/// Pre-defined path configuration for Rust
pub const RUST_PATH_CONFIG: PathConfig = PathConfig {
    separator: "::",
    relative_prefixes: &[
        RelativePrefix {
            prefix: "crate::",
            semantics: RelativeSemantics::Root,
            chainable: false,
        },
        RelativePrefix {
            prefix: "self::",
            semantics: RelativeSemantics::Current,
            chainable: false,
        },
        RelativePrefix {
            prefix: "super::",
            semantics: RelativeSemantics::Parent { levels: 1 },
            chainable: true,
        },
    ],
    external_prefixes: &["std", "core", "alloc", "external"],
};

/// Pre-defined path configuration for Python
pub const PYTHON_PATH_CONFIG: PathConfig = PathConfig {
    separator: ".",
    relative_prefixes: &[
        // Python uses dots for relative imports, but these are typically
        // handled at the import statement level, not in type references.
        // For now, Python paths are treated as absolute.
    ],
    external_prefixes: &[],
};

/// Pre-defined path configuration for JavaScript/TypeScript
pub const JAVASCRIPT_PATH_CONFIG: PathConfig = PathConfig {
    separator: ".",
    relative_prefixes: &[
        // JavaScript uses "./" and "../" for relative imports, but these
        // are typically handled at the import statement level.
        // For now, JS/TS paths are treated as absolute.
    ],
    external_prefixes: &[],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_config_has_all_prefixes() {
        assert_eq!(RUST_PATH_CONFIG.separator, "::");
        assert_eq!(RUST_PATH_CONFIG.relative_prefixes.len(), 3);
        assert_eq!(RUST_PATH_CONFIG.external_prefixes.len(), 4);
    }

    #[test]
    fn test_rust_crate_prefix() {
        let prefix = &RUST_PATH_CONFIG.relative_prefixes[0];
        assert_eq!(prefix.prefix, "crate::");
        assert_eq!(prefix.semantics, RelativeSemantics::Root);
        assert!(!prefix.chainable);
    }

    #[test]
    fn test_rust_super_prefix_is_chainable() {
        let prefix = &RUST_PATH_CONFIG.relative_prefixes[2];
        assert_eq!(prefix.prefix, "super::");
        assert!(matches!(
            prefix.semantics,
            RelativeSemantics::Parent { levels: 1 }
        ));
        assert!(prefix.chainable);
    }

    #[test]
    fn test_python_config() {
        assert_eq!(PYTHON_PATH_CONFIG.separator, ".");
        assert!(PYTHON_PATH_CONFIG.relative_prefixes.is_empty());
    }

    #[test]
    fn test_javascript_config() {
        assert_eq!(JAVASCRIPT_PATH_CONFIG.separator, ".");
        assert!(JAVASCRIPT_PATH_CONFIG.relative_prefixes.is_empty());
    }
}
