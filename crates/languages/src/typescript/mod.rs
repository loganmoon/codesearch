//! TypeScript language extractor module
//!
//! This module provides entity extraction for TypeScript (.ts) and TSX (.tsx) files.
//! It uses different tree-sitter parsers for each file type:
//! - LANGUAGE_TYPESCRIPT for .ts files (does not support JSX)
//! - LANGUAGE_TSX for .tsx files (supports JSX syntax)

pub(crate) mod handler_impls;
pub(crate) mod queries;
pub(crate) mod utils;

use crate::extraction_framework::{EntityHandler, LanguageConfigurationBuilder};
use crate::qualified_name::{ScopeConfiguration, ScopePattern};
use codesearch_core::{error::Result, CodeEntity};
use std::path::{Path, PathBuf};

/// Scope patterns for TypeScript qualified name building
const TYPESCRIPT_SCOPE_PATTERNS: &[ScopePattern] = &[
    ScopePattern {
        node_kind: "class_declaration",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "abstract_class_declaration",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "function_declaration",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "interface_declaration",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "internal_module",
        field_name: "name",
    },
];

inventory::submit! {
    ScopeConfiguration {
        language: "typescript",
        separator: ".",
        patterns: TYPESCRIPT_SCOPE_PATTERNS,
    }
}

/// TypeScript/TSX language extractor
///
/// This extractor handles both .ts and .tsx files by using the appropriate
/// tree-sitter parser based on file extension:
/// - `.ts` files use LANGUAGE_TYPESCRIPT
/// - `.tsx` files use LANGUAGE_TSX (which supports JSX syntax)
pub struct TypeScriptExtractor {
    repository_id: String,
    package_name: Option<String>,
    source_root: Option<PathBuf>,
    repo_root: PathBuf,
    ts_config: crate::extraction_framework::LanguageConfiguration,
    tsx_config: crate::extraction_framework::LanguageConfiguration,
}

impl TypeScriptExtractor {
    /// Create a new TypeScript extractor
    pub fn new(
        repository_id: String,
        package_name: Option<String>,
        source_root: Option<PathBuf>,
        repo_root: PathBuf,
    ) -> Result<Self> {
        // Build configurations for both TS and TSX parsers
        let ts_config = Self::build_config(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?;
        let tsx_config = Self::build_config(tree_sitter_typescript::LANGUAGE_TSX.into())?;

        Ok(Self {
            repository_id,
            package_name,
            source_root,
            repo_root,
            ts_config,
            tsx_config,
        })
    }

    /// Build language configuration with all entity extractors
    fn build_config(
        language: tree_sitter::Language,
    ) -> Result<crate::extraction_framework::LanguageConfiguration> {
        LanguageConfigurationBuilder::new(language)
            .add_extractor(
                "function",
                queries::FUNCTION_QUERY,
                Self::wrap_handler(handler_impls::handle_function_impl),
            )
            .add_extractor(
                "arrow_function",
                queries::ARROW_FUNCTION_QUERY,
                Self::wrap_handler(handler_impls::handle_arrow_function_impl),
            )
            .add_extractor(
                "class",
                queries::CLASS_QUERY,
                Self::wrap_handler(handler_impls::handle_class_impl),
            )
            .add_extractor(
                "method",
                queries::METHOD_QUERY,
                Self::wrap_handler(handler_impls::handle_method_impl),
            )
            .add_extractor(
                "interface",
                queries::INTERFACE_QUERY,
                Self::wrap_handler(handler_impls::handle_interface_impl),
            )
            .add_extractor(
                "type_alias",
                queries::TYPE_ALIAS_QUERY,
                Self::wrap_handler(handler_impls::handle_type_alias_impl),
            )
            .add_extractor(
                "enum",
                queries::ENUM_QUERY,
                Self::wrap_handler(handler_impls::handle_enum_impl),
            )
            .add_extractor(
                "module",
                queries::MODULE_QUERY,
                Self::wrap_handler(handler_impls::handle_module_impl),
            )
            .add_extractor(
                "variable",
                queries::VARIABLE_QUERY,
                Self::wrap_handler(handler_impls::handle_variable_impl),
            )
            .add_extractor(
                "field",
                queries::FIELD_QUERY,
                Self::wrap_handler(handler_impls::handle_field_impl),
            )
            .add_extractor(
                "private_field",
                queries::PRIVATE_FIELD_QUERY,
                Self::wrap_handler(handler_impls::handle_field_impl),
            )
            .add_extractor(
                "interface_property",
                queries::INTERFACE_PROPERTY_QUERY,
                Self::wrap_handler(handler_impls::handle_interface_property_impl),
            )
            .add_extractor(
                "interface_method",
                queries::INTERFACE_METHOD_QUERY,
                Self::wrap_handler(handler_impls::handle_interface_method_impl),
            )
            .add_extractor(
                "call_signature",
                queries::CALL_SIGNATURE_QUERY,
                Self::wrap_handler(handler_impls::handle_call_signature_impl),
            )
            .add_extractor(
                "construct_signature",
                queries::CONSTRUCT_SIGNATURE_QUERY,
                Self::wrap_handler(handler_impls::handle_construct_signature_impl),
            )
            .add_extractor(
                "index_signature",
                queries::INDEX_SIGNATURE_QUERY,
                Self::wrap_handler(handler_impls::handle_index_signature_impl),
            )
            .add_extractor(
                "parameter_property",
                queries::PARAMETER_PROPERTY_QUERY,
                Self::wrap_handler(handler_impls::handle_parameter_property_impl),
            )
            .add_extractor(
                "class_expression",
                queries::CLASS_EXPRESSION_QUERY,
                Self::wrap_handler(handler_impls::handle_class_expression_impl),
            )
            .add_extractor(
                "namespace",
                queries::NAMESPACE_QUERY,
                Self::wrap_handler(handler_impls::handle_namespace_impl),
            )
            .add_extractor(
                "function_expression",
                queries::FUNCTION_EXPRESSION_QUERY,
                Self::wrap_handler(handler_impls::handle_function_expression_impl),
            )
            .build()
    }

    /// Wrap a handler function into a boxed EntityHandler
    fn wrap_handler<F>(handler: F) -> EntityHandler
    where
        F: Fn(
                &tree_sitter::QueryMatch,
                &tree_sitter::Query,
                &str,
                &Path,
                &str,
                Option<&str>,
                Option<&Path>,
                &Path,
            ) -> Result<Vec<CodeEntity>>
            + Send
            + Sync
            + 'static,
    {
        Box::new(handler)
    }

    /// Check if a file is a TSX file based on extension
    fn is_tsx_file(file_path: &Path) -> bool {
        file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("tsx"))
            .unwrap_or(false)
    }
}

impl crate::Extractor for TypeScriptExtractor {
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        // Select the appropriate configuration based on file extension
        let config = if Self::is_tsx_file(file_path) {
            &self.tsx_config
        } else {
            &self.ts_config
        };

        let mut extractor = crate::extraction_framework::GenericExtractor::new(
            config,
            self.repository_id.clone(),
            self.package_name.as_deref(),
            self.source_root.as_deref(),
            &self.repo_root,
        )?;

        extractor.extract(source, file_path)
    }
}

// Register language with inventory
inventory::submit! {
    crate::LanguageDescriptor {
        name: "typescript",
        extensions: &["ts", "tsx"],
        factory: |repo_id, pkg_name, src_root, repo_root| Ok(Box::new(TypeScriptExtractor::new(
            repo_id.to_string(),
            pkg_name.map(String::from),
            src_root.map(PathBuf::from),
            repo_root.to_path_buf(),
        )?)),
    }
}
