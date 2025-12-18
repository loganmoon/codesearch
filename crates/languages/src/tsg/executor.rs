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
use std::sync::OnceLock;
use tree_sitter::Parser;
use tree_sitter_graph::ast::File as TsgFile;
use tree_sitter_graph::functions::Functions;
use tree_sitter_graph::graph::Value;
use tree_sitter_graph::{ExecutionConfig, Identifier, NoCancellation, Variables};

/// Cached identifiers for common TSG attribute names
struct AttrIds {
    type_: Identifier,
    name: Identifier,
    visibility: Identifier,
    start_row: Identifier,
    end_row: Identifier,
    kind: Identifier,
    path: Identifier,
    base_path: Identifier,
    context: Identifier,
    is_glob: Identifier,
    // Python-specific attributes
    module: Identifier,
    relative_prefix: Identifier,
}

impl AttrIds {
    fn new() -> Self {
        Self {
            type_: Identifier::from("type"),
            name: Identifier::from("name"),
            visibility: Identifier::from("visibility"),
            start_row: Identifier::from("start_row"),
            end_row: Identifier::from("end_row"),
            kind: Identifier::from("kind"),
            path: Identifier::from("path"),
            base_path: Identifier::from("base_path"),
            context: Identifier::from("context"),
            is_glob: Identifier::from("is_glob"),
            // Python-specific attributes
            module: Identifier::from("module"),
            relative_prefix: Identifier::from("relative_prefix"),
        }
    }
}

/// Global cached attribute identifiers
static ATTR_IDS: OnceLock<AttrIds> = OnceLock::new();

fn attr_ids() -> &'static AttrIds {
    ATTR_IDS.get_or_init(AttrIds::new)
}

/// The TSG rules for Rust source extraction
pub const RUST_TSG_RULES: &str = include_str!("rust.tsg");

/// The TSG rules for JavaScript source extraction
pub const JAVASCRIPT_TSG_RULES: &str = include_str!("javascript.tsg");

/// The TSG rules for TypeScript source extraction
pub const TYPESCRIPT_TSG_RULES: &str = include_str!("typescript.tsg");

/// The TSG rules for Python source extraction
pub const PYTHON_TSG_RULES: &str = include_str!("python.tsg");

/// Extract simple name from a potentially qualified path
/// e.g., "std::io::Read" -> "Read", "crate::module::Foo" -> "Foo"
fn extract_simple_name(path: &str) -> String {
    path.rsplit("::").next().unwrap_or(path).to_string()
}

/// Executor for TSG-based extraction of resolution nodes
pub struct TsgExecutor {
    tsg_file: TsgFile,
    parser: Parser,
    functions: Functions,
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

        let functions = Functions::stdlib();

        Ok(Self {
            tsg_file,
            parser,
            functions,
        })
    }

    /// Create a new TSG executor for JavaScript
    pub fn new_javascript() -> Result<Self> {
        let language: tree_sitter::Language = tree_sitter_javascript::LANGUAGE.into();
        let tsg_file = TsgFile::from_str(language.clone(), JAVASCRIPT_TSG_RULES)
            .map_err(|e| anyhow!("Failed to parse JavaScript TSG rules: {e}"))?;

        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|e| anyhow!("Failed to set parser language: {e}"))?;

        let functions = Functions::stdlib();

        Ok(Self {
            tsg_file,
            parser,
            functions,
        })
    }

    /// Create a new TSG executor for TypeScript
    pub fn new_typescript() -> Result<Self> {
        let language: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let tsg_file = TsgFile::from_str(language.clone(), TYPESCRIPT_TSG_RULES)
            .map_err(|e| anyhow!("Failed to parse TypeScript TSG rules: {e}"))?;

        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|e| anyhow!("Failed to set parser language: {e}"))?;

        let functions = Functions::stdlib();

        Ok(Self {
            tsg_file,
            parser,
            functions,
        })
    }

    /// Create a new TSG executor for Python
    pub fn new_python() -> Result<Self> {
        let language: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
        let tsg_file = TsgFile::from_str(language.clone(), PYTHON_TSG_RULES)
            .map_err(|e| anyhow!("Failed to parse Python TSG rules: {e}"))?;

        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|e| anyhow!("Failed to set parser language: {e}"))?;

        let functions = Functions::stdlib();

        Ok(Self {
            tsg_file,
            parser,
            functions,
        })
    }

    /// Extract resolution nodes from source code
    pub fn extract(&mut self, source: &str, file_path: &Path) -> Result<Vec<ResolutionNode>> {
        let tree = self
            .parser
            .parse(source, None)
            .ok_or_else(|| anyhow!("Failed to parse source"))?;

        let globals = Variables::new();
        let config = ExecutionConfig::new(&self.functions, &globals);

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
        let ids = attr_ids();

        for node_ref in graph.iter_nodes() {
            let graph_node = &graph[node_ref];

            // Get the "type" attribute to determine node kind
            let node_type = match graph_node.attributes.get(&ids.type_) {
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

            // Extract common attributes using cached identifiers
            let raw_name = get_string_attr(&graph_node.attributes, &ids.name);
            let visibility = get_optional_string_attr(&graph_node.attributes, &ids.visibility);
            let start_row = get_int_attr(&graph_node.attributes, &ids.start_row);
            let end_row = get_int_attr(&graph_node.attributes, &ids.end_row);

            // Extract kind-specific attributes
            let definition_kind = get_optional_string_attr(&graph_node.attributes, &ids.kind);
            let path = get_optional_string_attr(&graph_node.attributes, &ids.path);
            let base_path = get_optional_string_attr(&graph_node.attributes, &ids.base_path);
            let context = get_optional_string_attr(&graph_node.attributes, &ids.context);
            let is_glob_str = get_optional_string_attr(&graph_node.attributes, &ids.is_glob);
            let is_glob = is_glob_str.as_deref() == Some("true");

            // Python-specific: module attribute for from-imports
            let module = get_optional_string_attr(&graph_node.attributes, &ids.module);
            let relative_prefix =
                get_optional_string_attr(&graph_node.attributes, &ids.relative_prefix);

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

            // Construct full import path based on language-specific attributes
            let full_path = if let Some(base) = &base_path {
                // Rust grouped imports: use base_path + name
                Some(format!("{base}::{raw_name}"))
            } else if let Some(mod_path) = &module {
                // Python from-imports: module.name (e.g., "os.path.join")
                if let Some(prefix) = &relative_prefix {
                    // Relative import: prefix + module + name
                    Some(format!("{prefix}{mod_path}.{raw_name}"))
                } else {
                    Some(format!("{mod_path}.{raw_name}"))
                }
            } else if let Some(prefix) = &relative_prefix {
                // Python relative import without module: prefix + name
                Some(format!("{prefix}{raw_name}"))
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
}

/// Get a required string attribute using a pre-cached identifier
fn get_string_attr(attrs: &tree_sitter_graph::graph::Attributes, id: &Identifier) -> String {
    match attrs.get(id) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Get an optional string attribute using a pre-cached identifier
fn get_optional_string_attr(
    attrs: &tree_sitter_graph::graph::Attributes,
    id: &Identifier,
) -> Option<String> {
    match attrs.get(id) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// Get an integer attribute (as u32), defaulting to 0, using a pre-cached identifier
fn get_int_attr(attrs: &tree_sitter_graph::graph::Attributes, id: &Identifier) -> u32 {
    match attrs.get(id) {
        Some(Value::Integer(i)) => *i,
        _ => 0,
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

    #[test]
    fn test_python_executor_creation() {
        let executor = TsgExecutor::new_python();
        match &executor {
            Ok(_) => {}
            Err(e) => panic!("Failed to create Python executor: {e}"),
        }
        assert!(executor.is_ok());
    }

    #[test]
    fn test_python_extract_definitions() {
        let source = r#"
def my_function():
    pass

class MyClass:
    def method(self):
        pass

@decorator
def decorated_function():
    pass

@decorator
class DecoratedClass:
    pass
"#;

        let mut executor = TsgExecutor::new_python().unwrap();
        let nodes = executor.extract(source, &PathBuf::from("test.py")).unwrap();

        let definitions: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Definition)
            .collect();

        println!(
            "Python definitions: {:?}",
            definitions
                .iter()
                .map(|d| (&d.name, d.definition_kind.as_deref()))
                .collect::<Vec<_>>()
        );

        // Should have: my_function, MyClass, method, decorated_function, DecoratedClass
        assert!(
            definitions.len() >= 4,
            "Expected at least 4 definitions, got {}: {:?}",
            definitions.len(),
            definitions.iter().map(|d| &d.name).collect::<Vec<_>>()
        );

        // Check function
        let my_function = definitions.iter().find(|n| n.name == "my_function");
        assert!(my_function.is_some(), "my_function should be extracted");
        assert_eq!(
            my_function.unwrap().definition_kind.as_deref(),
            Some("function")
        );

        // Check class
        let my_class = definitions.iter().find(|n| n.name == "MyClass");
        assert!(my_class.is_some(), "MyClass should be extracted");
        assert_eq!(my_class.unwrap().definition_kind.as_deref(), Some("class"));

        // Check decorated function
        let decorated_fn = definitions.iter().find(|n| n.name == "decorated_function");
        assert!(
            decorated_fn.is_some(),
            "decorated_function should be extracted"
        );
    }

    #[test]
    fn test_python_extract_imports() {
        let source = r#"
import os
import os.path as osp
from collections import defaultdict
from typing import List as L
from os.path import *
from . import utils
from ..helpers import helper as h
"#;

        let mut executor = TsgExecutor::new_python().unwrap();
        let nodes = executor.extract(source, &PathBuf::from("test.py")).unwrap();

        let imports: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Import)
            .collect();

        println!(
            "Python imports: {:?}",
            imports
                .iter()
                .map(|i| (&i.name, &i.import_path, i.is_glob))
                .collect::<Vec<_>>()
        );

        // Should have multiple imports
        assert!(
            imports.len() >= 5,
            "Expected at least 5 imports, got {}: {:?}",
            imports.len(),
            imports.iter().map(|i| &i.name).collect::<Vec<_>>()
        );

        // Check simple import
        let os_import = imports.iter().find(|n| n.name == "os" && !n.is_glob);
        assert!(os_import.is_some(), "Should have os import");

        // Check aliased import
        let osp_import = imports.iter().find(|n| n.name == "osp");
        assert!(osp_import.is_some(), "Should have osp (aliased) import");

        // Check glob import
        let glob_import = imports.iter().find(|n| n.is_glob);
        assert!(glob_import.is_some(), "Should have glob import");
        assert_eq!(glob_import.unwrap().name, "*");
    }

    #[test]
    fn test_python_extract_references() {
        let source = r#"
def process(data: List[str]) -> Result:
    result = transform(data)
    obj.method()
    return result

class MyClass(BaseClass):
    pass
"#;

        let mut executor = TsgExecutor::new_python().unwrap();
        let nodes = executor.extract(source, &PathBuf::from("test.py")).unwrap();

        let references: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Reference)
            .collect();

        println!(
            "Python references: {:?}",
            references
                .iter()
                .map(|r| (&r.name, r.reference_context.as_deref()))
                .collect::<Vec<_>>()
        );

        // Should have references for calls, types, and inheritance
        assert!(!references.is_empty(), "Expected some references, got none");

        // Check for call reference
        let call_refs: Vec<_> = references
            .iter()
            .filter(|r| r.reference_context.as_deref() == Some("call"))
            .collect();
        assert!(!call_refs.is_empty(), "Should have call references");

        // Check for method call reference
        let method_refs: Vec<_> = references
            .iter()
            .filter(|r| r.reference_context.as_deref() == Some("method_call"))
            .collect();
        assert!(
            !method_refs.is_empty(),
            "Should have method call references"
        );

        // Check for inheritance reference
        let inheritance_refs: Vec<_> = references
            .iter()
            .filter(|r| r.reference_context.as_deref() == Some("inheritance"))
            .collect();
        assert!(
            !inheritance_refs.is_empty(),
            "Should have inheritance references"
        );
        assert!(
            inheritance_refs.iter().any(|r| r.name == "BaseClass"),
            "Should reference BaseClass"
        );
    }

    #[test]
    fn test_javascript_variable_declarations() {
        let source = r#"
const foo = () => {}
let bar = customAlphabet()
const { a, b } = obj
export function exported() {}
"#;

        let mut executor = TsgExecutor::new_javascript().unwrap();
        let nodes = executor.extract(source, &PathBuf::from("test.js")).unwrap();

        let definitions: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Definition)
            .collect();

        println!(
            "JavaScript definitions: {:?}",
            definitions.iter().map(|d| &d.name).collect::<Vec<_>>()
        );

        // Should have definitions for foo, bar, a, b, and exported
        let def_names: Vec<_> = definitions.iter().map(|d| d.name.as_str()).collect();
        assert!(
            def_names.contains(&"foo"),
            "Should have definition for 'foo'"
        );
        assert!(
            def_names.contains(&"bar"),
            "Should have definition for 'bar'"
        );
        assert!(
            def_names.contains(&"exported"),
            "Should have definition for 'exported'"
        );
    }
}
