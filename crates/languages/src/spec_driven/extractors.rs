//! Spec-driven extractors for each language
//!
//! This module provides extractors that use the spec-driven engine
//! and generated handler configurations.

use super::engine::{extract_with_config, SpecDrivenContext};
use super::HandlerConfig;
use crate::Extractor;
use codesearch_core::entities::Language;
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::{Path, PathBuf};
use tree_sitter::Parser;

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

    /// Extract entities using the given handler configs
    fn extract_with_configs(
        &self,
        source: &str,
        file_path: &Path,
        configs: &[&HandlerConfig],
    ) -> Result<Vec<CodeEntity>> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|e| anyhow::anyhow!("Failed to set Rust language: {e}"))?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse source code"))?;

        let ctx = SpecDrivenContext {
            source,
            file_path,
            repository_id: &self.repository_id,
            package_name: self.package_name.as_deref(),
            source_root: self.source_root.as_deref(),
            repo_root: &self.repo_root,
            language: Language::Rust,
            language_str: "rust",
        };

        let mut all_entities = Vec::new();

        for config in configs {
            let entities = extract_with_config(config, &ctx, tree.root_node())?;
            all_entities.extend(entities);
        }

        Ok(all_entities)
    }
}

impl Extractor for SpecDrivenRustExtractor {
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        use super::rust::handler_configs::ALL_HANDLERS;
        self.extract_with_configs(source, file_path, ALL_HANDLERS)
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

        let ctx = SpecDrivenContext {
            source,
            file_path,
            repository_id: &self.repository_id,
            package_name: self.package_name.as_deref(),
            source_root: self.source_root.as_deref(),
            repo_root: &self.repo_root,
            language: Language::JavaScript,
            language_str: "javascript",
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

        let ctx = SpecDrivenContext {
            source,
            file_path,
            repository_id: &self.repository_id,
            package_name: self.package_name.as_deref(),
            source_root: self.source_root.as_deref(),
            repo_root: &self.repo_root,
            language: Language::TypeScript,
            language_str: "typescript",
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
