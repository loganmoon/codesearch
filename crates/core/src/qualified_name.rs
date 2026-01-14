//! Structured qualified name representation for code entities.
//!
//! This module provides a type-safe representation of qualified names that
//! captures their semantic structure, enabling proper containment checking
//! without brittle string manipulation.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{self, Display};

use crate::error::{Error, Result};

/// Path separator style for qualified names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PathSeparator {
    /// `::` separator (Rust)
    #[default]
    DoubleColon,
    /// `.` separator (TypeScript, Python, JavaScript)
    Dot,
}

impl PathSeparator {
    /// Get the string representation of this separator.
    pub fn as_str(&self) -> &'static str {
        match self {
            PathSeparator::DoubleColon => "::",
            PathSeparator::Dot => ".",
        }
    }
}

/// Structured qualified name representation.
///
/// Captures the semantic structure of qualified names to enable proper
/// containment checking without substring matching.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum QualifiedName {
    /// Regular path: `crate::module::Type::method` or `package.module.Class.method`
    SimplePath {
        /// Path segments (e.g., `["crate", "module", "Type", "method"]`)
        segments: Vec<String>,
        /// Separator style
        separator: PathSeparator,
    },

    /// Inherent impl block: `crate::impl crate::Type`
    InherentImpl {
        /// Module scope containing the impl (e.g., `["crate", "module"]`)
        scope: Vec<String>,
        /// Type being implemented (e.g., `["crate", "module", "Type"]`)
        type_path: Vec<String>,
    },

    /// Trait impl block: `<crate::Type as crate::Trait>`
    TraitImpl {
        /// Module scope containing the impl (not serialized, used for containment)
        scope: Vec<String>,
        /// Type being implemented (e.g., `["crate", "Type"]`)
        type_path: Vec<String>,
        /// Trait being implemented (e.g., `["crate", "Trait"]`)
        trait_path: Vec<String>,
    },

    /// Trait impl item: `<crate::Type as crate::Trait>::method`
    TraitImplItem {
        /// Type being implemented
        type_path: Vec<String>,
        /// Trait being implemented
        trait_path: Vec<String>,
        /// Item name (method, associated type, or constant)
        item_name: String,
    },

    /// Extern block: `crate::extern "C"`
    ExternBlock {
        /// Module scope containing the extern block
        scope: Vec<String>,
        /// Linkage specification (e.g., "C", "Rust")
        linkage: String,
    },
}

impl QualifiedName {
    /// Parse a qualified name string into structured form.
    ///
    /// # Errors
    ///
    /// Returns an error if the string is empty or malformed.
    pub fn parse(s: &str) -> Result<Self> {
        if s.is_empty() {
            return Err(Error::invalid_input("qualified name cannot be empty"));
        }

        if s.starts_with('<') {
            Self::parse_trait_impl(s)
        } else if s.contains("::impl ") {
            Self::parse_inherent_impl(s)
        } else if s.contains("::extern ") {
            Self::parse_extern_block(s)
        } else {
            Self::parse_simple_path(s)
        }
    }

    fn parse_simple_path(s: &str) -> Result<Self> {
        let (segments, separator) = if s.contains("::") {
            (
                s.split("::").map(String::from).collect::<Vec<_>>(),
                PathSeparator::DoubleColon,
            )
        } else if s.contains('.') {
            (
                s.split('.').map(String::from).collect::<Vec<_>>(),
                PathSeparator::Dot,
            )
        } else {
            // Single segment
            (vec![s.to_string()], PathSeparator::DoubleColon)
        };

        // Validate no empty segments
        if segments.iter().any(|s| s.is_empty()) {
            return Err(Error::invalid_input(format!(
                "qualified name contains empty segment: {s}"
            )));
        }

        Ok(QualifiedName::SimplePath {
            segments,
            separator,
        })
    }

    fn parse_inherent_impl(s: &str) -> Result<Self> {
        // Pattern: scope::impl type_path
        let parts: Vec<&str> = s.split("::impl ").collect();
        if parts.len() != 2 {
            return Err(Error::invalid_input(format!(
                "invalid inherent impl format: {s}"
            )));
        }

        let scope: Vec<String> = parts[0].split("::").map(String::from).collect();
        let type_path: Vec<String> = parts[1].split("::").map(String::from).collect();

        if scope.iter().any(|s| s.is_empty()) || type_path.iter().any(|s| s.is_empty()) {
            return Err(Error::invalid_input(format!(
                "inherent impl contains empty segment: {s}"
            )));
        }

        Ok(QualifiedName::InherentImpl { scope, type_path })
    }

    fn parse_trait_impl(s: &str) -> Result<Self> {
        // Pattern: <type_path as trait_path> or <type_path as trait_path>::item
        let s = s
            .strip_prefix('<')
            .ok_or_else(|| Error::invalid_input("trait impl must start with '<'"))?;

        if let Some((impl_part, item)) = s.rsplit_once(">::") {
            // Has item: <Type as Trait>::method
            let (type_str, trait_str) = impl_part.split_once(" as ").ok_or_else(|| {
                Error::invalid_input(format!("trait impl item missing ' as ': {s}"))
            })?;

            let type_path: Vec<String> = type_str.split("::").map(String::from).collect();
            let trait_path: Vec<String> = trait_str.split("::").map(String::from).collect();

            if type_path.iter().any(|s| s.is_empty())
                || trait_path.iter().any(|s| s.is_empty())
                || item.is_empty()
            {
                return Err(Error::invalid_input(format!(
                    "trait impl item contains empty segment: {s}"
                )));
            }

            Ok(QualifiedName::TraitImplItem {
                type_path,
                trait_path,
                item_name: item.to_string(),
            })
        } else {
            // Just impl block: <Type as Trait>
            let inner = s
                .strip_suffix('>')
                .ok_or_else(|| Error::invalid_input("trait impl must end with '>'"))?;

            let (type_str, trait_str) = inner
                .split_once(" as ")
                .ok_or_else(|| Error::invalid_input(format!("trait impl missing ' as ': {s}")))?;

            let type_path: Vec<String> = type_str.split("::").map(String::from).collect();
            let trait_path: Vec<String> = trait_str.split("::").map(String::from).collect();

            if type_path.iter().any(|s| s.is_empty()) || trait_path.iter().any(|s| s.is_empty()) {
                return Err(Error::invalid_input(format!(
                    "trait impl contains empty segment: {s}"
                )));
            }

            Ok(QualifiedName::TraitImpl {
                scope: vec![], // Unknown from string alone
                type_path,
                trait_path,
            })
        }
    }

    fn parse_extern_block(s: &str) -> Result<Self> {
        // Pattern: scope::extern "linkage" (e.g., "crate::extern \"C\"")
        let parts: Vec<&str> = s.split("::extern ").collect();
        if parts.len() != 2 {
            return Err(Error::invalid_input(format!(
                "invalid extern block format: {s}"
            )));
        }

        let scope: Vec<String> = parts[0].split("::").map(String::from).collect();
        let linkage = parts[1].trim_matches('"').to_string();

        if scope.iter().any(|s| s.is_empty()) || linkage.is_empty() {
            return Err(Error::invalid_input(format!(
                "extern block contains empty segment: {s}"
            )));
        }

        Ok(QualifiedName::ExternBlock { scope, linkage })
    }

    /// Check if this qualified name represents a child of the given parent.
    ///
    /// This method handles the semantic containment relationships:
    /// - SimplePath children have parent segments as a prefix
    /// - Impl blocks are children of their scope module
    /// - Trait impl items are children of their trait impl block
    pub fn is_child_of(&self, parent: &QualifiedName) -> bool {
        match (self, parent) {
            // Simple path: child segments start with parent segments
            (
                QualifiedName::SimplePath {
                    segments: child_segs,
                    ..
                },
                QualifiedName::SimplePath {
                    segments: parent_segs,
                    ..
                },
            ) => child_segs.len() > parent_segs.len() && child_segs.starts_with(parent_segs),

            // Inherent impl is child of its scope module
            (
                QualifiedName::InherentImpl { scope, .. },
                QualifiedName::SimplePath { segments, .. },
            ) => scope == segments,

            // Trait impl is child of its scope module
            (
                QualifiedName::TraitImpl {
                    scope,
                    type_path,
                    trait_path,
                },
                QualifiedName::SimplePath { segments, .. },
            ) => {
                if !scope.is_empty() {
                    // Scope is known, use it directly
                    scope == segments
                } else {
                    // Scope was not set (parsed from string); infer from type_path or trait_path.
                    // A trait impl `<A::B::C as D::E::F>` is a child of module `A::B`
                    // if `A::B` is a proper prefix of the type_path `A::B::C`.
                    // For extension traits like `<String as crate::MyTrait>`, check trait_path too.
                    let type_matches =
                        type_path.len() > segments.len() && type_path.starts_with(segments);
                    let trait_matches =
                        trait_path.len() > segments.len() && trait_path.starts_with(segments);
                    type_matches || trait_matches
                }
            }

            // Trait impl item is child of its trait impl block
            (
                QualifiedName::TraitImplItem {
                    type_path: t1,
                    trait_path: tr1,
                    ..
                },
                QualifiedName::TraitImpl {
                    type_path: t2,
                    trait_path: tr2,
                    ..
                },
            ) => t1 == t2 && tr1 == tr2,

            // Extern block is child of its scope module
            (
                QualifiedName::ExternBlock { scope, .. },
                QualifiedName::SimplePath { segments, .. },
            ) => scope == segments,

            // SimplePath can be child of ExternBlock if it's in the same scope
            // (extern block items are siblings in the module scope, not nested under extern block)
            (
                QualifiedName::SimplePath {
                    segments: child_segs,
                    ..
                },
                QualifiedName::ExternBlock { scope, .. },
            ) => {
                // Child must be in the extern block's scope (e.g., crate::fn is child of crate::extern "C")
                child_segs.len() > scope.len() && child_segs.starts_with(scope)
            }

            _ => false,
        }
    }

    /// Set the scope for a TraitImpl variant.
    ///
    /// Used when the scope wasn't known at parse time but is available
    /// from context (e.g., during extraction from AST).
    #[must_use]
    pub fn with_scope(self, scope: Vec<String>) -> Self {
        match self {
            QualifiedName::TraitImpl {
                type_path,
                trait_path,
                ..
            } => QualifiedName::TraitImpl {
                scope,
                type_path,
                trait_path,
            },
            other => other,
        }
    }

    /// Get the simple name (last segment) of this qualified name.
    pub fn simple_name(&self) -> &str {
        match self {
            QualifiedName::SimplePath { segments, .. } => {
                segments.last().map(|s| s.as_str()).unwrap_or("")
            }
            QualifiedName::InherentImpl { type_path, .. } => {
                type_path.last().map(|s| s.as_str()).unwrap_or("")
            }
            QualifiedName::TraitImpl {
                type_path,
                trait_path,
                ..
            } => {
                // For trait impls, we use the trait name as the "simple name"
                // since the display is "<Type as Trait>"
                trait_path
                    .last()
                    .map(|s| s.as_str())
                    .unwrap_or_else(|| type_path.last().map(|s| s.as_str()).unwrap_or(""))
            }
            QualifiedName::TraitImplItem { item_name, .. } => item_name,
            QualifiedName::ExternBlock { linkage, .. } => linkage,
        }
    }

    /// Get the segments for a SimplePath, or None for other variants.
    pub fn segments(&self) -> Option<&[String]> {
        match self {
            QualifiedName::SimplePath { segments, .. } => Some(segments),
            _ => None,
        }
    }

    /// Get the separator for this qualified name.
    pub fn separator(&self) -> PathSeparator {
        match self {
            QualifiedName::SimplePath { separator, .. } => *separator,
            // Rust-style names default to DoubleColon
            _ => PathSeparator::DoubleColon,
        }
    }
}

impl Display for QualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QualifiedName::SimplePath {
                segments,
                separator,
            } => {
                write!(f, "{}", segments.join(separator.as_str()))
            }
            QualifiedName::InherentImpl { scope, type_path } => {
                write!(f, "{}::impl {}", scope.join("::"), type_path.join("::"))
            }
            QualifiedName::TraitImpl {
                type_path,
                trait_path,
                ..
            } => {
                // Note: scope is NOT included in serialized form
                write!(f, "<{} as {}>", type_path.join("::"), trait_path.join("::"))
            }
            QualifiedName::TraitImplItem {
                type_path,
                trait_path,
                item_name,
            } => {
                write!(
                    f,
                    "<{} as {}>::{}",
                    type_path.join("::"),
                    trait_path.join("::"),
                    item_name
                )
            }
            QualifiedName::ExternBlock { scope, linkage } => {
                write!(f, "{}::extern \"{}\"", scope.join("::"), linkage)
            }
        }
    }
}

impl From<QualifiedName> for String {
    fn from(qn: QualifiedName) -> String {
        qn.to_string()
    }
}

impl TryFrom<String> for QualifiedName {
    type Error = Error;

    fn try_from(s: String) -> Result<Self> {
        QualifiedName::parse(&s)
    }
}

impl TryFrom<&str> for QualifiedName {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self> {
        QualifiedName::parse(s)
    }
}

impl Serialize for QualifiedName {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for QualifiedName {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        QualifiedName::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_path_rust() {
        let qn = QualifiedName::parse("crate::module::Type::method").unwrap();
        assert!(matches!(
            qn,
            QualifiedName::SimplePath {
                ref segments,
                separator: PathSeparator::DoubleColon
            } if segments == &["crate", "module", "Type", "method"]
        ));
    }

    #[test]
    fn test_parse_simple_path_typescript() {
        let qn = QualifiedName::parse("package.module.Class.method").unwrap();
        assert!(matches!(
            qn,
            QualifiedName::SimplePath {
                ref segments,
                separator: PathSeparator::Dot
            } if segments == &["package", "module", "Class", "method"]
        ));
    }

    #[test]
    fn test_parse_single_segment() {
        let qn = QualifiedName::parse("crate_name").unwrap();
        assert!(matches!(
            qn,
            QualifiedName::SimplePath {
                ref segments,
                separator: PathSeparator::DoubleColon
            } if segments == &["crate_name"]
        ));
    }

    #[test]
    fn test_parse_inherent_impl() {
        let qn = QualifiedName::parse("test_crate::impl test_crate::MyType").unwrap();
        assert!(matches!(
            qn,
            QualifiedName::InherentImpl {
                ref scope,
                ref type_path
            } if scope == &["test_crate"] && type_path == &["test_crate", "MyType"]
        ));
    }

    #[test]
    fn test_parse_trait_impl() {
        let qn = QualifiedName::parse("<test_crate::MyType as test_crate::Handler>").unwrap();
        assert!(matches!(
            qn,
            QualifiedName::TraitImpl {
                ref type_path,
                ref trait_path,
                ..
            } if type_path == &["test_crate", "MyType"] && trait_path == &["test_crate", "Handler"]
        ));
    }

    #[test]
    fn test_parse_trait_impl_item() {
        let qn =
            QualifiedName::parse("<test_crate::MyType as test_crate::Handler>::handle").unwrap();
        assert!(matches!(
            qn,
            QualifiedName::TraitImplItem {
                ref type_path,
                ref trait_path,
                ref item_name,
            } if type_path == &["test_crate", "MyType"]
                && trait_path == &["test_crate", "Handler"]
                && item_name == "handle"
        ));
    }

    #[test]
    fn test_parse_empty_fails() {
        assert!(QualifiedName::parse("").is_err());
    }

    #[test]
    fn test_parse_empty_segment_fails() {
        assert!(QualifiedName::parse("crate::::Type").is_err());
        assert!(QualifiedName::parse("package..Class").is_err());
    }

    #[test]
    fn test_display_roundtrip() {
        let cases = [
            "test_crate",
            "test_crate::module::Type",
            "package.module.Class",
            "test_crate::impl test_crate::MyType",
            "<test_crate::MyType as test_crate::Handler>",
            "<test_crate::MyType as test_crate::Handler>::handle",
        ];

        for case in cases {
            let parsed = QualifiedName::parse(case).unwrap();
            assert_eq!(parsed.to_string(), case, "roundtrip failed for: {case}");
        }
    }

    #[test]
    fn test_serde_roundtrip() {
        let qn = QualifiedName::parse("<test_crate::Foo as test_crate::Bar>::method").unwrap();
        let json = serde_json::to_string(&qn).unwrap();
        assert_eq!(json, "\"<test_crate::Foo as test_crate::Bar>::method\"");

        let parsed: QualifiedName = serde_json::from_str(&json).unwrap();
        assert_eq!(qn, parsed);
    }

    #[test]
    fn test_is_child_of_simple_path() {
        let parent = QualifiedName::parse("crate::module").unwrap();
        let child = QualifiedName::parse("crate::module::Type").unwrap();
        let grandchild = QualifiedName::parse("crate::module::Type::method").unwrap();
        let unrelated = QualifiedName::parse("other::module").unwrap();

        assert!(child.is_child_of(&parent));
        assert!(grandchild.is_child_of(&parent));
        assert!(grandchild.is_child_of(&child));
        assert!(!parent.is_child_of(&child));
        assert!(!unrelated.is_child_of(&parent));
        assert!(!parent.is_child_of(&parent)); // Not a child of itself
    }

    #[test]
    fn test_is_child_of_inherent_impl() {
        let module = QualifiedName::parse("test_crate").unwrap();
        let impl_block = QualifiedName::parse("test_crate::impl test_crate::MyType").unwrap();

        assert!(impl_block.is_child_of(&module));
    }

    #[test]
    fn test_is_child_of_trait_impl() {
        let module = QualifiedName::parse("test_crate").unwrap();
        let impl_block = QualifiedName::parse("<test_crate::MyType as test_crate::Handler>")
            .unwrap()
            .with_scope(vec!["test_crate".to_string()]);

        assert!(impl_block.is_child_of(&module));
    }

    #[test]
    fn test_is_child_of_trait_impl_item() {
        let impl_block = QualifiedName::parse("<test_crate::MyType as test_crate::Handler>")
            .unwrap()
            .with_scope(vec!["test_crate".to_string()]);
        let method =
            QualifiedName::parse("<test_crate::MyType as test_crate::Handler>::handle").unwrap();

        assert!(method.is_child_of(&impl_block));
    }

    #[test]
    fn test_with_scope() {
        let qn = QualifiedName::parse("<Foo as Bar>").unwrap();
        let with_scope = qn.with_scope(vec!["my_crate".to_string(), "module".to_string()]);

        if let QualifiedName::TraitImpl { scope, .. } = with_scope {
            assert_eq!(scope, vec!["my_crate", "module"]);
        } else {
            panic!("expected TraitImpl variant");
        }
    }

    #[test]
    fn test_simple_name() {
        assert_eq!(
            QualifiedName::parse("crate::module::Type")
                .unwrap()
                .simple_name(),
            "Type"
        );
        assert_eq!(
            QualifiedName::parse("test_crate::impl test_crate::MyType")
                .unwrap()
                .simple_name(),
            "MyType"
        );
        assert_eq!(
            QualifiedName::parse("<Foo as Bar>").unwrap().simple_name(),
            "Bar"
        );
        assert_eq!(
            QualifiedName::parse("<Foo as Bar>::method")
                .unwrap()
                .simple_name(),
            "method"
        );
    }
}
