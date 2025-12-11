//! TSG execution wrapper for extracting resolution nodes from source code
//!
//! This module provides a wrapper around tree-sitter-graph that extracts
//! Definition, Export, Import, and Reference nodes from Rust source files.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use super::graph_types::{ResolutionNode, ResolutionNodeKind};
use anyhow::{anyhow, Result};
use std::path::Path;
use tree_sitter::Parser;
use tree_sitter_graph::ast::File as TsgFile;
use tree_sitter_graph::functions::Functions;
use tree_sitter_graph::graph::Value;
use tree_sitter_graph::{ExecutionConfig, Identifier, NoCancellation, Variables};

/// The TSG rules for Rust source extraction
pub const RUST_TSG_RULES: &str = include_str!("rust.tsg");

/// Extract simple name from a potentially qualified path
/// e.g., "std::io::Read" -> "Read", "crate::module::Foo" -> "Foo"
fn extract_simple_name(path: &str) -> String {
    path.rsplit("::").next().unwrap_or(path).to_string()
}

/// Executor for TSG-based extraction of resolution nodes
pub struct TsgExecutor {
    tsg_file: TsgFile,
    parser: Parser,
}

impl TsgExecutor {
    /// Create a new TSG executor for Rust
    pub fn new_rust() -> Result<Self> {
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        let tsg_file = TsgFile::from_str(language.clone(), RUST_TSG_RULES)
            .map_err(|e| anyhow!("Failed to parse TSG rules: {e}"))?;

        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|e| anyhow!("Failed to set parser language: {e}"))?;

        Ok(Self { tsg_file, parser })
    }

    /// Extract resolution nodes from source code
    pub fn extract(&mut self, source: &str, file_path: &Path) -> Result<Vec<ResolutionNode>> {
        let tree = self
            .parser
            .parse(source, None)
            .ok_or_else(|| anyhow!("Failed to parse source"))?;

        let functions = Functions::stdlib();
        let globals = Variables::new();
        let config = ExecutionConfig::new(&functions, &globals);

        let graph = self
            .tsg_file
            .execute(&tree, source, &config, &NoCancellation)
            .map_err(|e| anyhow!("TSG execution failed: {e}"))?;

        self.convert_graph(&graph, file_path)
    }

    /// Convert TSG graph nodes to ResolutionNodes
    fn convert_graph(
        &self,
        graph: &tree_sitter_graph::graph::Graph,
        file_path: &Path,
    ) -> Result<Vec<ResolutionNode>> {
        let mut nodes = Vec::new();

        for node_ref in graph.iter_nodes() {
            let graph_node = &graph[node_ref];

            // Get the "type" attribute to determine node kind
            let type_id = Identifier::from("type");
            let node_type = match graph_node.attributes.get(&type_id) {
                Some(Value::String(s)) => s.clone(),
                _ => continue, // Skip nodes without a type
            };

            let kind = match node_type.as_str() {
                "Definition" => ResolutionNodeKind::Definition,
                "Export" => ResolutionNodeKind::Export,
                "Import" => ResolutionNodeKind::Import,
                "Reference" => ResolutionNodeKind::Reference,
                _ => continue, // Unknown type
            };

            // Extract common attributes
            let raw_name = self.get_string_attr(&graph_node.attributes, "name");
            let visibility = self.get_optional_string_attr(&graph_node.attributes, "visibility");
            let start_row = self.get_int_attr(&graph_node.attributes, "start_row");
            let end_row = self.get_int_attr(&graph_node.attributes, "end_row");

            // Extract kind-specific attributes
            let definition_kind = self.get_optional_string_attr(&graph_node.attributes, "kind");
            let path = self.get_optional_string_attr(&graph_node.attributes, "path");
            let base_path = self.get_optional_string_attr(&graph_node.attributes, "base_path");
            let context = self.get_optional_string_attr(&graph_node.attributes, "context");
            let is_glob_str = self.get_optional_string_attr(&graph_node.attributes, "is_glob");
            let is_glob = is_glob_str.as_deref() == Some("true");

            // For imports/exports, extract simple name from full path (e.g., "std::io::Read" -> "Read")
            let name = match kind {
                ResolutionNodeKind::Import | ResolutionNodeKind::Export => {
                    if is_glob {
                        "*".to_string()
                    } else {
                        extract_simple_name(&raw_name)
                    }
                }
                _ => raw_name.clone(),
            };

            // Construct full import path for grouped imports (use base_path + name)
            let full_path = if let Some(base) = &base_path {
                Some(format!("{base}::{raw_name}"))
            } else {
                path.clone()
            };

            // Build qualified name from file path (will be refined later with module path)
            let qualified_name = format!(
                "{}::{}",
                file_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown"),
                &name
            );

            let resolution_node = match kind {
                ResolutionNodeKind::Definition => ResolutionNode::definition(
                    name,
                    qualified_name,
                    file_path.to_path_buf(),
                    start_row,
                    end_row,
                    visibility,
                    definition_kind.unwrap_or_else(|| "unknown".to_string()),
                ),
                ResolutionNodeKind::Export => ResolutionNode::export(
                    name,
                    qualified_name,
                    file_path.to_path_buf(),
                    start_row,
                    end_row,
                    path.unwrap_or(raw_name),
                ),
                ResolutionNodeKind::Import => ResolutionNode::import(
                    name,
                    qualified_name,
                    file_path.to_path_buf(),
                    start_row,
                    end_row,
                    full_path.unwrap_or(raw_name),
                    is_glob,
                ),
                ResolutionNodeKind::Reference => ResolutionNode::reference(
                    name,
                    qualified_name,
                    file_path.to_path_buf(),
                    start_row,
                    end_row,
                    context,
                ),
            };

            nodes.push(resolution_node);
        }

        Ok(nodes)
    }

    /// Get a required string attribute
    fn get_string_attr(&self, attrs: &tree_sitter_graph::graph::Attributes, name: &str) -> String {
        let id = Identifier::from(name);
        match attrs.get(&id) {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        }
    }

    /// Get an optional string attribute
    fn get_optional_string_attr(
        &self,
        attrs: &tree_sitter_graph::graph::Attributes,
        name: &str,
    ) -> Option<String> {
        let id = Identifier::from(name);
        match attrs.get(&id) {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        }
    }

    /// Get an integer attribute (as u32), defaulting to 0
    fn get_int_attr(&self, attrs: &tree_sitter_graph::graph::Attributes, name: &str) -> u32 {
        let id = Identifier::from(name);
        match attrs.get(&id) {
            Some(Value::Integer(i)) => *i,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_executor_creation() {
        let executor = TsgExecutor::new_rust();
        match &executor {
            Ok(_) => {}
            Err(e) => panic!("Failed to create executor: {e}"),
        }
        assert!(executor.is_ok());
    }

    #[test]
    fn test_extract_definitions() {
        let source = r#"
pub struct MyStruct {
    field: i32,
}

fn private_function() {}

pub fn public_function() -> String {
    String::new()
}

pub trait MyTrait {
    fn required(&self);
}

pub enum MyEnum {
    A,
    B,
}

const MY_CONST: i32 = 42;
"#;

        let mut executor = TsgExecutor::new_rust().unwrap();
        let nodes = executor.extract(source, &PathBuf::from("test.rs")).unwrap();

        // Count definitions
        let definitions: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Definition)
            .collect();

        // Should have: MyStruct, private_function, public_function, MyTrait, required, MyEnum, MY_CONST
        assert!(
            definitions.len() >= 5,
            "Expected at least 5 definitions, got {}: {:?}",
            definitions.len(),
            definitions.iter().map(|d| &d.name).collect::<Vec<_>>()
        );

        // Check struct
        let my_struct = definitions.iter().find(|n| n.name == "MyStruct");
        assert!(my_struct.is_some(), "MyStruct should be extracted");
        assert_eq!(
            my_struct.unwrap().definition_kind.as_deref(),
            Some("struct")
        );

        // Check function visibility
        let pub_fn = definitions.iter().find(|n| n.name == "public_function");
        assert!(pub_fn.is_some());
        assert_eq!(pub_fn.unwrap().visibility.as_deref(), Some("pub"));

        let priv_fn = definitions.iter().find(|n| n.name == "private_function");
        assert!(priv_fn.is_some());
        // Private functions don't have visibility modifier
        assert!(
            priv_fn.unwrap().visibility.is_none()
                || priv_fn.unwrap().visibility.as_deref() == Some("")
        );
    }

    #[test]
    fn test_extract_imports() {
        let source = r#"
use std::io::Read;
use std::collections::HashMap;
use crate::module::*;
"#;

        let mut executor = TsgExecutor::new_rust().unwrap();
        let nodes = executor.extract(source, &PathBuf::from("test.rs")).unwrap();

        let imports: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Import)
            .collect();

        // Should have imports for Read, HashMap, and the glob
        assert!(
            imports.len() >= 2,
            "Expected at least 2 imports, got {}: {:?}",
            imports.len(),
            imports
                .iter()
                .map(|i| (&i.name, &i.import_path))
                .collect::<Vec<_>>()
        );

        // Check that simple names are extracted from full paths
        let read_import = imports.iter().find(|n| n.name == "Read");
        assert!(
            read_import.is_some(),
            "Should have Read import with simple name"
        );
        assert_eq!(
            read_import.unwrap().import_path.as_deref(),
            Some("std::io::Read"),
            "Should preserve full path in import_path"
        );

        // Check glob import
        let glob = imports.iter().find(|n| n.is_glob);
        assert!(glob.is_some(), "Should have a glob import");
        assert_eq!(glob.unwrap().name, "*");
    }

    #[test]
    fn test_extract_aliased_imports() {
        let source = r#"
use std::collections::HashMap as Map;
use codesearch_core::error::Result as CoreResult;
"#;

        let mut executor = TsgExecutor::new_rust().unwrap();
        let nodes = executor.extract(source, &PathBuf::from("test.rs")).unwrap();

        let imports: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Import)
            .collect();

        println!(
            "Aliased imports found: {:?}",
            imports
                .iter()
                .map(|i| (&i.name, &i.import_path))
                .collect::<Vec<_>>()
        );

        // Should have imports with aliased names
        // The alias name should be what we use for resolution
        assert!(
            imports
                .iter()
                .any(|n| n.name == "Map" || n.name == "HashMap"),
            "Should extract Map or HashMap import: {:?}",
            imports.iter().map(|i| &i.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_extract_local_functions() {
        let source = r#"
fn public_helper() -> i32 { 42 }

fn main() {
    let x = public_helper();
}

mod inner {
    fn inner_helper() -> i32 { 1 }

    pub fn use_helper() -> i32 {
        inner_helper()
    }
}
"#;

        let mut executor = TsgExecutor::new_rust().unwrap();
        let nodes = executor.extract(source, &PathBuf::from("test.rs")).unwrap();

        let definitions: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Definition)
            .collect();

        println!(
            "Definitions found: {:?}",
            definitions
                .iter()
                .map(|d| (&d.name, d.definition_kind.as_deref()))
                .collect::<Vec<_>>()
        );

        // Should have all function definitions
        assert!(
            definitions.iter().any(|n| n.name == "public_helper"),
            "Should extract public_helper"
        );
        assert!(
            definitions.iter().any(|n| n.name == "inner_helper"),
            "Should extract inner_helper"
        );
        assert!(
            definitions.iter().any(|n| n.name == "use_helper"),
            "Should extract use_helper"
        );
    }

    #[test]
    fn test_extract_references() {
        let source = r#"
fn my_function() {
    let x: MyType = something();
    other_call();
}
"#;

        let mut executor = TsgExecutor::new_rust().unwrap();
        let nodes = executor.extract(source, &PathBuf::from("test.rs")).unwrap();

        let references: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Reference)
            .collect();

        // Should have type reference (MyType) and call references (something, other_call)
        assert!(!references.is_empty(), "Expected some references, got none");

        // Check for type reference
        let type_refs: Vec<_> = references
            .iter()
            .filter(|r| r.reference_context.as_deref() == Some("type"))
            .collect();
        assert!(!type_refs.is_empty(), "Should have type references");

        // Check for call references
        let call_refs: Vec<_> = references
            .iter()
            .filter(|r| r.reference_context.as_deref() == Some("call"))
            .collect();
        assert!(!call_refs.is_empty(), "Should have call references");
    }

    #[test]
    fn test_extract_enum_variants() {
        let source = r#"
enum MyCommand {
    Search,
    Index,
    Clear,
}

fn test() {
    let cmd = MyCommand::Search;
    match cmd {
        MyCommand::Search => println!("search"),
        MyCommand::Index => println!("index"),
        MyCommand::Clear => println!("clear"),
    }
}
"#;

        let mut executor = TsgExecutor::new_rust().unwrap();
        let nodes = executor.extract(source, &PathBuf::from("test.rs")).unwrap();

        let definitions: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Definition)
            .collect();

        println!(
            "Definitions: {:?}",
            definitions
                .iter()
                .map(|d| (&d.name, d.definition_kind.as_deref()))
                .collect::<Vec<_>>()
        );

        // Should have the enum definition
        assert!(
            definitions.iter().any(|n| n.name == "MyCommand"),
            "Should extract MyCommand enum"
        );

        let references: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Reference)
            .collect();

        println!(
            "References: {:?}",
            references
                .iter()
                .map(|r| (&r.name, r.reference_context.as_deref()))
                .collect::<Vec<_>>()
        );

        // Check what we get for enum variant usage like MyCommand::Search
        // The enum type itself should be referenced
        let my_command_refs: Vec<_> = references
            .iter()
            .filter(|r| r.name == "MyCommand")
            .collect();
        println!("MyCommand references: {}", my_command_refs.len());

        // Check for variant name references (e.g., Search, Index, Clear)
        let search_refs: Vec<_> = references.iter().filter(|r| r.name == "Search").collect();
        println!("Search references: {}", search_refs.len());
    }
}
