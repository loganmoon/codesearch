//! Configuration for language-specific path parsing
//!
//! This module provides declarative configuration for how paths are parsed
//! and resolved in different programming languages.
//!
//! # Language Families
//!
//! Languages are grouped into families that share resolution semantics:
//!
//! - **CrateBased** (Rust): Uses `crate::`, `self::`, `super::` for relative paths.
//!   Unprefixed paths are resolved within the current crate.
//!
//! - **ModuleBased** (JavaScript, TypeScript, Python): Uses `./` and `../` for relative
//!   imports. Unprefixed paths are external (npm packages, stdlib).
//!
//! - **PackageBased** (Java, Go, C#): Uses absolute package paths. Reserved for future use.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

/// Language family for shared FQN resolution semantics
///
/// Languages within the same family share the same relative path resolution rules,
/// differing only in syntax details like file extensions or separator characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageFamily {
    /// Rust-style: `crate::`, `self::`, `super::` prefixes
    ///
    /// - Unprefixed paths are resolved within the current crate/package
    /// - External references use explicit crate names (e.g., `std::`, `serde::`)
    CrateBased,

    /// JavaScript/TypeScript/Python-style: `./`, `../` prefixes
    ///
    /// - Unprefixed paths are treated as external (npm packages, stdlib)
    /// - Relative imports use `./` (current) and `../` (parent)
    ModuleBased,

    /// Java/Go/C#-style: absolute package paths (future use)
    ///
    /// - All imports use absolute package paths
    /// - No relative import syntax
    PackageBased,
}

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

    /// Whether unprefixed imports are treated as external
    ///
    /// - `true` (ModuleBased): `import foo from 'lodash'` -> external
    /// - `false` (CrateBased): `use foo::bar` -> local unless in external_prefixes
    ///
    /// This affects how the parser determines if a path refers to an external
    /// dependency versus a local module.
    pub unprefixed_is_external: bool,
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
    unprefixed_is_external: false,
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
    unprefixed_is_external: true, // Python imports without dots are external (stdlib, pip)
};

/// Pre-defined path configuration for JavaScript/TypeScript
///
/// Note: This is the legacy config. New code should use `MODULE_BASED_PATH_CONFIG`
/// via the language family system.
pub const JAVASCRIPT_PATH_CONFIG: PathConfig = PathConfig {
    separator: ".",
    relative_prefixes: &[
        // JavaScript uses "./" and "../" for relative imports, but these
        // are typically handled at the import statement level.
        // For now, JS/TS paths are treated as absolute.
    ],
    external_prefixes: &[],
    unprefixed_is_external: true, // JS imports without ./ are external (npm packages)
};

// =============================================================================
// Language Family Configurations
// =============================================================================

/// Path configuration for the Crate-Based family (Rust)
///
/// This is the canonical configuration for languages using crate-style
/// module resolution. Individual language configs can override fields.
pub const CRATE_BASED_PATH_CONFIG: PathConfig = PathConfig {
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
    unprefixed_is_external: false,
};

/// Path configuration for the Module-Based family (JavaScript, TypeScript, Python)
///
/// This is the canonical configuration for languages using file-based
/// module resolution with `./` and `../` for relative imports.
pub const MODULE_BASED_PATH_CONFIG: PathConfig = PathConfig {
    separator: ".",
    relative_prefixes: &[
        RelativePrefix {
            prefix: "./",
            semantics: RelativeSemantics::Current,
            chainable: false,
        },
        RelativePrefix {
            prefix: "../",
            semantics: RelativeSemantics::Parent { levels: 1 },
            chainable: true,
        },
    ],
    external_prefixes: &[], // External detection via unprefixed_is_external
    unprefixed_is_external: true,
};

/// Path configuration for the Package-Based family (Java, Go, C#) - future use
///
/// This is reserved for languages using absolute package paths without
/// relative import syntax.
pub const PACKAGE_BASED_PATH_CONFIG: PathConfig = PathConfig {
    separator: ".",
    relative_prefixes: &[], // No relative imports
    external_prefixes: &[],
    unprefixed_is_external: false, // All paths are absolute
};

/// Get the canonical path configuration for a language family
///
/// This function provides the base configuration that languages in a family
/// should inherit. Individual languages can override specific fields.
///
/// # Example
///
/// ```ignore
/// // In a language extractor, use family defaults:
/// let config = get_family_config(LanguageFamily::ModuleBased);
/// ```
pub const fn get_family_config(family: LanguageFamily) -> &'static PathConfig {
    match family {
        LanguageFamily::CrateBased => &CRATE_BASED_PATH_CONFIG,
        LanguageFamily::ModuleBased => &MODULE_BASED_PATH_CONFIG,
        LanguageFamily::PackageBased => &PACKAGE_BASED_PATH_CONFIG,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_config_has_all_prefixes() {
        let config = &RUST_PATH_CONFIG;
        assert_eq!(config.separator, "::");
        assert_eq!(config.relative_prefixes.len(), 3);
        assert_eq!(config.external_prefixes.len(), 4);
        assert!(!config.unprefixed_is_external);
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
        let config = &PYTHON_PATH_CONFIG;
        assert_eq!(config.separator, ".");
        assert!(config.relative_prefixes.is_empty());
        assert!(config.unprefixed_is_external);
    }

    #[test]
    fn test_javascript_config() {
        let config = &JAVASCRIPT_PATH_CONFIG;
        assert_eq!(config.separator, ".");
        assert!(config.relative_prefixes.is_empty());
        assert!(config.unprefixed_is_external);
    }

    // Language Family Tests

    #[test]
    fn test_language_family_enum() {
        // Verify all variants exist
        let _crate_based = LanguageFamily::CrateBased;
        let _module_based = LanguageFamily::ModuleBased;
        let _package_based = LanguageFamily::PackageBased;
    }

    #[test]
    fn test_crate_based_family_config() {
        let config = get_family_config(LanguageFamily::CrateBased);
        assert_eq!(config.separator, "::");
        assert_eq!(config.relative_prefixes.len(), 3);
        assert!(!config.unprefixed_is_external);

        // Verify prefixes
        assert_eq!(config.relative_prefixes[0].prefix, "crate::");
        assert_eq!(config.relative_prefixes[1].prefix, "self::");
        assert_eq!(config.relative_prefixes[2].prefix, "super::");
    }

    #[test]
    fn test_module_based_family_config() {
        let config = get_family_config(LanguageFamily::ModuleBased);
        assert_eq!(config.separator, ".");
        assert_eq!(config.relative_prefixes.len(), 2);
        assert!(config.unprefixed_is_external);

        // Verify prefixes
        let current = &config.relative_prefixes[0];
        assert_eq!(current.prefix, "./");
        assert_eq!(current.semantics, RelativeSemantics::Current);
        assert!(!current.chainable);

        let parent = &config.relative_prefixes[1];
        assert_eq!(parent.prefix, "../");
        assert!(matches!(
            parent.semantics,
            RelativeSemantics::Parent { levels: 1 }
        ));
        assert!(parent.chainable);
    }

    #[test]
    fn test_package_based_family_config() {
        let config = get_family_config(LanguageFamily::PackageBased);
        assert_eq!(config.separator, ".");
        assert!(config.relative_prefixes.is_empty());
        assert!(!config.unprefixed_is_external);
    }

    #[test]
    fn test_get_family_config_matches_constants() {
        // Verify that the function returns configs matching the expected constants
        // Note: We compare by value since constants are inlined (not statics)
        let crate_config = get_family_config(LanguageFamily::CrateBased);
        assert_eq!(crate_config.separator, CRATE_BASED_PATH_CONFIG.separator);
        assert_eq!(
            crate_config.relative_prefixes.len(),
            CRATE_BASED_PATH_CONFIG.relative_prefixes.len()
        );
        assert_eq!(
            crate_config.unprefixed_is_external,
            CRATE_BASED_PATH_CONFIG.unprefixed_is_external
        );

        let module_config = get_family_config(LanguageFamily::ModuleBased);
        assert_eq!(module_config.separator, MODULE_BASED_PATH_CONFIG.separator);
        assert_eq!(
            module_config.relative_prefixes.len(),
            MODULE_BASED_PATH_CONFIG.relative_prefixes.len()
        );
        assert_eq!(
            module_config.unprefixed_is_external,
            MODULE_BASED_PATH_CONFIG.unprefixed_is_external
        );

        let package_config = get_family_config(LanguageFamily::PackageBased);
        assert_eq!(
            package_config.separator,
            PACKAGE_BASED_PATH_CONFIG.separator
        );
        assert_eq!(
            package_config.relative_prefixes.len(),
            PACKAGE_BASED_PATH_CONFIG.relative_prefixes.len()
        );
        assert_eq!(
            package_config.unprefixed_is_external,
            PACKAGE_BASED_PATH_CONFIG.unprefixed_is_external
        );
    }
}
