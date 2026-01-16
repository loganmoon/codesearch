//! Spec-driven extractors for each language
//!
//! This module provides extractors that use the spec-driven engine
//! and generated handler configurations.

use super::engine::{extract_with_config, SpecDrivenContext};
use crate::common::import_map::parse_file_imports;
use crate::common::js_ts_shared::{
    module_path as js_module_path, SCOPE_PATTERNS, TS_SCOPE_PATTERNS,
};
use crate::common::path_config::{CRATE_BASED_PATH_CONFIG, MODULE_BASED_PATH_CONFIG};
use crate::qualified_name::{ScopeConfiguration, ScopePattern};
use crate::rust::{edge_case_handlers::RUST_EDGE_CASE_HANDLERS, module_path as rust_module_path};
use crate::{Extractor, LanguageDescriptor};
use codesearch_core::entities::Language;
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::{Path, PathBuf};
use tree_sitter::Parser;

/// Scope patterns for Rust qualified name building
const RUST_SCOPE_PATTERNS: &[ScopePattern] = &[
    ScopePattern {
        node_kind: "mod_item",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "impl_item",
        field_name: "type",
    },
    ScopePattern {
        node_kind: "struct_item",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "enum_item",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "trait_item",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "union_item",
        field_name: "name",
    },
];

// Register scope configurations for qualified name building
inventory::submit! {
    ScopeConfiguration {
        language: "rust",
        separator: "::",
        patterns: RUST_SCOPE_PATTERNS,
        module_path_fn: Some(rust_module_path::derive_module_path),
        path_config: &CRATE_BASED_PATH_CONFIG,
        edge_case_handlers: Some(RUST_EDGE_CASE_HANDLERS),
    }
}

inventory::submit! {
    ScopeConfiguration {
        language: "javascript",
        separator: ".",
        patterns: SCOPE_PATTERNS,
        module_path_fn: Some(js_module_path::derive_module_path),
        path_config: &MODULE_BASED_PATH_CONFIG,
        edge_case_handlers: None,
    }
}

inventory::submit! {
    ScopeConfiguration {
        language: "typescript",
        separator: ".",
        patterns: TS_SCOPE_PATTERNS,
        module_path_fn: Some(js_module_path::derive_module_path),
        path_config: &MODULE_BASED_PATH_CONFIG,
        edge_case_handlers: None,
    }
}

inventory::submit! {
    ScopeConfiguration {
        language: "tsx",
        separator: ".",
        patterns: TS_SCOPE_PATTERNS,
        module_path_fn: Some(js_module_path::derive_module_path),
        path_config: &MODULE_BASED_PATH_CONFIG,
        edge_case_handlers: None,
    }
}

// Register language descriptors for spec-driven extractors
inventory::submit! {
    LanguageDescriptor {
        name: "rust",
        extensions: &["rs"],
        factory: create_rust_extractor,
    }
}

inventory::submit! {
    LanguageDescriptor {
        name: "javascript",
        extensions: &["js", "jsx"],
        factory: create_javascript_extractor,
    }
}

inventory::submit! {
    LanguageDescriptor {
        name: "typescript",
        extensions: &["ts"],
        factory: create_typescript_extractor,
    }
}

inventory::submit! {
    LanguageDescriptor {
        name: "tsx",
        extensions: &["tsx"],
        factory: create_tsx_extractor,
    }
}

/// Factory function for Rust extractor
fn create_rust_extractor(
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Box<dyn Extractor>> {
    Ok(Box::new(SpecDrivenRustExtractor::new(
        repository_id.to_string(),
        package_name.map(String::from),
        source_root.map(PathBuf::from),
        repo_root.to_path_buf(),
    )?))
}

/// Factory function for JavaScript extractor
fn create_javascript_extractor(
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Box<dyn Extractor>> {
    Ok(Box::new(SpecDrivenJavaScriptExtractor::new(
        repository_id.to_string(),
        package_name.map(String::from),
        source_root.map(PathBuf::from),
        repo_root.to_path_buf(),
    )?))
}

/// Factory function for TypeScript extractor
fn create_typescript_extractor(
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Box<dyn Extractor>> {
    Ok(Box::new(SpecDrivenTypeScriptExtractor::new(
        repository_id.to_string(),
        package_name.map(String::from),
        source_root.map(PathBuf::from),
        repo_root.to_path_buf(),
    )?))
}

/// Factory function for TSX extractor
fn create_tsx_extractor(
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Box<dyn Extractor>> {
    Ok(Box::new(SpecDrivenTsxExtractor::new(
        repository_id.to_string(),
        package_name.map(String::from),
        source_root.map(PathBuf::from),
        repo_root.to_path_buf(),
    )?))
}

/// Spec-driven Rust extractor
///
/// Uses the generated handler configurations from rust.yaml to extract
/// entities from Rust source code.
pub struct SpecDrivenRustExtractor {
    repository_id: String,
    package_name: Option<String>,
    source_root: Option<PathBuf>,
    repo_root: PathBuf,
}

impl SpecDrivenRustExtractor {
    /// Create a new spec-driven Rust extractor
    pub fn new(
        repository_id: String,
        package_name: Option<String>,
        source_root: Option<PathBuf>,
        repo_root: PathBuf,
    ) -> Result<Self> {
        Ok(Self {
            repository_id,
            package_name,
            source_root,
            repo_root,
        })
    }
}

impl Extractor for SpecDrivenRustExtractor {
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        use crate::handler_engine::{extract_with_handlers, HandlerContext};

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|e| anyhow::anyhow!("Failed to set Rust language: {e}"))?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse source code"))?;

        // Build import map for reference resolution
        let import_map = parse_file_imports(tree.root_node(), source, Language::Rust, None);

        let ctx = HandlerContext {
            source,
            file_path,
            repository_id: &self.repository_id,
            package_name: self.package_name.as_deref(),
            source_root: self.source_root.as_deref(),
            repo_root: &self.repo_root,
            language: Language::Rust,
            language_str: "rust",
            import_map: &import_map,
        };

        extract_with_handlers(&ctx, tree.root_node())
    }
}

/// Spec-driven JavaScript extractor
pub struct SpecDrivenJavaScriptExtractor {
    repository_id: String,
    package_name: Option<String>,
    source_root: Option<PathBuf>,
    repo_root: PathBuf,
}

impl SpecDrivenJavaScriptExtractor {
    /// Create a new spec-driven JavaScript extractor
    pub fn new(
        repository_id: String,
        package_name: Option<String>,
        source_root: Option<PathBuf>,
        repo_root: PathBuf,
    ) -> Result<Self> {
        Ok(Self {
            repository_id,
            package_name,
            source_root,
            repo_root,
        })
    }
}

impl Extractor for SpecDrivenJavaScriptExtractor {
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .map_err(|e| anyhow::anyhow!("Failed to set JavaScript language: {e}"))?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse source code"))?;

        // Derive module path for relative import resolution
        let module_path = self
            .source_root
            .as_deref()
            .and_then(|root| js_module_path::derive_module_path(file_path, root));

        // Build import map for reference resolution
        let import_map = parse_file_imports(
            tree.root_node(),
            source,
            Language::JavaScript,
            module_path.as_deref(),
        );

        let ctx = SpecDrivenContext {
            source,
            file_path,
            repository_id: &self.repository_id,
            package_name: self.package_name.as_deref(),
            source_root: self.source_root.as_deref(),
            repo_root: &self.repo_root,
            language: Language::JavaScript,
            language_str: "javascript",
            import_map: &import_map,
            path_config: &MODULE_BASED_PATH_CONFIG,
            edge_case_handlers: None, // No JS-specific edge case handlers yet
        };

        use super::javascript::handler_configs::ALL_HANDLERS;

        let mut all_entities = Vec::new();

        for config in ALL_HANDLERS {
            let entities = extract_with_config(config, &ctx, tree.root_node())?;
            all_entities.extend(entities);
        }

        Ok(all_entities)
    }
}

/// Spec-driven TypeScript extractor
pub struct SpecDrivenTypeScriptExtractor {
    repository_id: String,
    package_name: Option<String>,
    source_root: Option<PathBuf>,
    repo_root: PathBuf,
}

impl SpecDrivenTypeScriptExtractor {
    /// Create a new spec-driven TypeScript extractor
    pub fn new(
        repository_id: String,
        package_name: Option<String>,
        source_root: Option<PathBuf>,
        repo_root: PathBuf,
    ) -> Result<Self> {
        Ok(Self {
            repository_id,
            package_name,
            source_root,
            repo_root,
        })
    }
}

impl Extractor for SpecDrivenTypeScriptExtractor {
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .map_err(|e| anyhow::anyhow!("Failed to set TypeScript language: {e}"))?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse source code"))?;

        // Derive module path for relative import resolution
        let module_path = self
            .source_root
            .as_deref()
            .and_then(|root| js_module_path::derive_module_path(file_path, root));

        // Build import map for reference resolution
        let import_map = parse_file_imports(
            tree.root_node(),
            source,
            Language::TypeScript,
            module_path.as_deref(),
        );

        let ctx = SpecDrivenContext {
            source,
            file_path,
            repository_id: &self.repository_id,
            package_name: self.package_name.as_deref(),
            source_root: self.source_root.as_deref(),
            repo_root: &self.repo_root,
            language: Language::TypeScript,
            language_str: "typescript",
            import_map: &import_map,
            path_config: &MODULE_BASED_PATH_CONFIG,
            edge_case_handlers: None, // No TS-specific edge case handlers yet
        };

        use super::typescript::handler_configs::ALL_HANDLERS;

        let mut all_entities = Vec::new();

        for config in ALL_HANDLERS {
            let entities = extract_with_config(config, &ctx, tree.root_node())?;
            all_entities.extend(entities);
        }

        Ok(all_entities)
    }
}

/// Spec-driven TSX extractor
///
/// Uses TypeScript handlers with the TSX parser for JSX support.
pub struct SpecDrivenTsxExtractor {
    repository_id: String,
    package_name: Option<String>,
    source_root: Option<PathBuf>,
    repo_root: PathBuf,
}

impl SpecDrivenTsxExtractor {
    /// Create a new spec-driven TSX extractor
    pub fn new(
        repository_id: String,
        package_name: Option<String>,
        source_root: Option<PathBuf>,
        repo_root: PathBuf,
    ) -> Result<Self> {
        Ok(Self {
            repository_id,
            package_name,
            source_root,
            repo_root,
        })
    }
}

impl Extractor for SpecDrivenTsxExtractor {
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
            .map_err(|e| anyhow::anyhow!("Failed to set TSX language: {e}"))?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse source code"))?;

        // Derive module path for relative import resolution
        let module_path = self
            .source_root
            .as_deref()
            .and_then(|root| js_module_path::derive_module_path(file_path, root));

        // Build import map for reference resolution (use TypeScript language)
        let import_map = parse_file_imports(
            tree.root_node(),
            source,
            Language::TypeScript,
            module_path.as_deref(),
        );

        // TSX uses TypeScript language enum since they share the same type system
        let ctx = SpecDrivenContext {
            source,
            file_path,
            repository_id: &self.repository_id,
            package_name: self.package_name.as_deref(),
            source_root: self.source_root.as_deref(),
            repo_root: &self.repo_root,
            language: Language::TypeScript,
            language_str: "tsx",
            import_map: &import_map,
            path_config: &MODULE_BASED_PATH_CONFIG,
            edge_case_handlers: None, // No TSX-specific edge case handlers yet
        };

        // TSX uses the same handlers as TypeScript
        use super::typescript::handler_configs::ALL_HANDLERS;

        let mut all_entities = Vec::new();

        for config in ALL_HANDLERS {
            let entities = extract_with_config(config, &ctx, tree.root_node())?;
            all_entities.extend(entities);
        }

        Ok(all_entities)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_spec_driven_rust_extractor_basic() {
        let extractor = SpecDrivenRustExtractor::new(
            "test-repo".to_string(),
            Some("test_crate".to_string()),
            None,
            PathBuf::from("/test"),
        )
        .expect("Failed to create extractor");

        let source = r#"
            fn hello() {
                println!("Hello, world!");
            }
        "#;

        let result = extractor.extract(source, Path::new("/test/src/lib.rs"));
        assert!(result.is_ok(), "Extraction failed: {:?}", result.err());

        let entities = result.unwrap();
        // We should extract at least the function
        println!("Extracted {} entities", entities.len());
        for entity in &entities {
            println!("  - {} ({})", entity.name, entity.entity_type);
        }
    }

    #[test]
    fn test_spec_driven_javascript_extractor_basic() {
        let extractor = SpecDrivenJavaScriptExtractor::new(
            "test-repo".to_string(),
            Some("test_package".to_string()),
            None,
            PathBuf::from("/test"),
        )
        .expect("Failed to create extractor");

        let source = r#"
            function hello() {
                console.log("Hello, world!");
            }
        "#;

        let result = extractor.extract(source, Path::new("/test/src/index.js"));
        assert!(result.is_ok(), "Extraction failed: {:?}", result.err());

        let entities = result.unwrap();
        println!("Extracted {} JS entities", entities.len());
        for entity in &entities {
            println!("  - {} ({})", entity.name, entity.entity_type);
        }
    }

    #[test]
    fn test_spec_driven_typescript_extractor_basic() {
        let extractor = SpecDrivenTypeScriptExtractor::new(
            "test-repo".to_string(),
            Some("test_package".to_string()),
            None,
            PathBuf::from("/test"),
        )
        .expect("Failed to create extractor");

        let source = r#"
            function hello(): void {
                console.log("Hello, world!");
            }

            interface User {
                name: string;
                age: number;
            }
        "#;

        let result = extractor.extract(source, Path::new("/test/src/index.ts"));
        assert!(result.is_ok(), "Extraction failed: {:?}", result.err());

        let entities = result.unwrap();
        println!("Extracted {} TS entities", entities.len());
        for entity in &entities {
            println!("  - {} ({})", entity.name, entity.entity_type);
        }
    }
}
