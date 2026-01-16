//! Extract context for entity handlers
//!
//! This module provides the `ExtractContext` type which encapsulates all the data
//! needed by entity handlers to extract code entities from AST matches.

use crate::common::edge_case_handlers::EdgeCaseRegistry;
use crate::common::import_map::ImportMap;
use crate::common::path_config::PathConfig;
use codesearch_core::entities::Language;
use codesearch_core::error::{Error, Result};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Node;

/// Data for a single capture from a query match
#[derive(Debug, Clone, Copy)]
pub struct CaptureData<'a> {
    /// The captured AST node
    pub node: Node<'a>,
    /// The text of the captured node
    pub text: &'a str,
}

/// Builder for constructing ExtractContext instances
pub struct ExtractContextBuilder<'a> {
    node: Option<Node<'a>>,
    source: Option<&'a str>,
    captures: HashMap<&'a str, CaptureData<'a>>,
    file_path: Option<&'a Path>,
    import_map: Option<&'a ImportMap>,
    scope_stack: Vec<String>,
    language: Option<Language>,
    language_str: Option<&'a str>,
    repository_id: Option<&'a str>,
    package_name: Option<&'a str>,
    source_root: Option<&'a Path>,
    repo_root: Option<&'a Path>,
    path_config: Option<&'static PathConfig>,
    edge_case_handlers: Option<&'a EdgeCaseRegistry>,
}

impl<'a> Default for ExtractContextBuilder<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> ExtractContextBuilder<'a> {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            node: None,
            source: None,
            captures: HashMap::new(),
            file_path: None,
            import_map: None,
            scope_stack: Vec::new(),
            language: None,
            language_str: None,
            repository_id: None,
            package_name: None,
            source_root: None,
            repo_root: None,
            path_config: None,
            edge_case_handlers: None,
        }
    }

    /// Set the primary captured node
    pub fn node(mut self, node: Node<'a>) -> Self {
        self.node = Some(node);
        self
    }

    /// Set the source code
    pub fn source(mut self, source: &'a str) -> Self {
        self.source = Some(source);
        self
    }

    /// Add a capture
    pub fn capture(mut self, name: &'a str, data: CaptureData<'a>) -> Self {
        self.captures.insert(name, data);
        self
    }

    /// Set all captures from a map
    pub fn captures(mut self, captures: HashMap<&'a str, CaptureData<'a>>) -> Self {
        self.captures = captures;
        self
    }

    /// Set the file path
    pub fn file_path(mut self, path: &'a Path) -> Self {
        self.file_path = Some(path);
        self
    }

    /// Set the import map
    pub fn import_map(mut self, import_map: &'a ImportMap) -> Self {
        self.import_map = Some(import_map);
        self
    }

    /// Set the scope stack
    pub fn scope_stack(mut self, scope_stack: Vec<String>) -> Self {
        self.scope_stack = scope_stack;
        self
    }

    /// Set the language
    pub fn language(mut self, language: Language) -> Self {
        self.language = Some(language);
        self
    }

    /// Set the language string identifier
    pub fn language_str(mut self, language_str: &'a str) -> Self {
        self.language_str = Some(language_str);
        self
    }

    /// Set the repository ID
    pub fn repository_id(mut self, repository_id: &'a str) -> Self {
        self.repository_id = Some(repository_id);
        self
    }

    /// Set the package name
    pub fn package_name(mut self, package_name: Option<&'a str>) -> Self {
        self.package_name = package_name;
        self
    }

    /// Set the source root
    pub fn source_root(mut self, source_root: Option<&'a Path>) -> Self {
        self.source_root = source_root;
        self
    }

    /// Set the repository root
    pub fn repo_root(mut self, repo_root: &'a Path) -> Self {
        self.repo_root = Some(repo_root);
        self
    }

    /// Set the path configuration
    pub fn path_config(mut self, path_config: &'static PathConfig) -> Self {
        self.path_config = Some(path_config);
        self
    }

    /// Set the edge case handlers
    pub fn edge_case_handlers(mut self, edge_case_handlers: Option<&'a EdgeCaseRegistry>) -> Self {
        self.edge_case_handlers = edge_case_handlers;
        self
    }

    /// Build the ExtractContext
    ///
    /// Returns an error if required fields are missing.
    pub fn build(self) -> Result<ExtractContext<'a>> {
        Ok(ExtractContext {
            node: self
                .node
                .ok_or_else(|| Error::entity_extraction("ExtractContext: node is required"))?,
            source: self
                .source
                .ok_or_else(|| Error::entity_extraction("ExtractContext: source is required"))?,
            captures: self.captures,
            file_path: self
                .file_path
                .ok_or_else(|| Error::entity_extraction("ExtractContext: file_path is required"))?,
            import_map: self.import_map.ok_or_else(|| {
                Error::entity_extraction("ExtractContext: import_map is required")
            })?,
            scope_stack: self.scope_stack,
            language: self
                .language
                .ok_or_else(|| Error::entity_extraction("ExtractContext: language is required"))?,
            language_str: self.language_str.ok_or_else(|| {
                Error::entity_extraction("ExtractContext: language_str is required")
            })?,
            repository_id: self.repository_id.ok_or_else(|| {
                Error::entity_extraction("ExtractContext: repository_id is required")
            })?,
            package_name: self.package_name,
            source_root: self.source_root,
            repo_root: self
                .repo_root
                .ok_or_else(|| Error::entity_extraction("ExtractContext: repo_root is required"))?,
            path_config: self.path_config.ok_or_else(|| {
                Error::entity_extraction("ExtractContext: path_config is required")
            })?,
            edge_case_handlers: self.edge_case_handlers,
        })
    }
}

/// Context provided to entity handlers during extraction
///
/// This struct encapsulates all the information needed to extract a code entity
/// from an AST match, including the source code, captured nodes, and semantic context.
#[derive(Debug)]
pub struct ExtractContext<'a> {
    /// The primary captured node for this handler
    node: Node<'a>,
    /// Source code being processed
    source: &'a str,
    /// All captures from the query match: name -> (node, text)
    captures: HashMap<&'a str, CaptureData<'a>>,
    /// File being processed
    file_path: &'a Path,
    /// Import map for identifier resolution
    import_map: &'a ImportMap,
    /// Pre-computed scope stack from AST traversal
    scope_stack: Vec<String>,
    /// Language identifier enum
    language: Language,
    /// Language string identifier (e.g., "rust", "javascript")
    language_str: &'a str,
    /// Repository identifier
    repository_id: &'a str,
    /// Optional package/crate name from manifest
    package_name: Option<&'a str>,
    /// Optional source root for module path derivation
    source_root: Option<&'a Path>,
    /// Repository root for repo-relative paths
    repo_root: &'a Path,
    /// Language-specific path configuration for resolution
    path_config: &'static PathConfig,
    /// Optional edge case handlers for language-specific patterns
    edge_case_handlers: Option<&'a EdgeCaseRegistry>,
}

impl<'a> ExtractContext<'a> {
    /// Create a new builder for ExtractContext
    pub fn builder() -> ExtractContextBuilder<'a> {
        ExtractContextBuilder::new()
    }

    // === Capture Access ===

    /// Get text of a required capture, error if missing
    pub fn capture_text(&self, name: &str) -> Result<&'a str> {
        self.captures
            .get(name)
            .map(|c| c.text)
            .ok_or_else(|| Error::entity_extraction(format!("Missing required capture: {name}")))
    }

    /// Get node of a required capture, error if missing
    pub fn capture_node(&self, name: &str) -> Result<Node<'a>> {
        self.captures
            .get(name)
            .map(|c| c.node)
            .ok_or_else(|| Error::entity_extraction(format!("Missing required capture: {name}")))
    }

    /// Get text of an optional capture
    pub fn capture_text_opt(&self, name: &str) -> Option<&'a str> {
        self.captures.get(name).map(|c| c.text)
    }

    /// Get node of an optional capture
    pub fn capture_node_opt(&self, name: &str) -> Option<Node<'a>> {
        self.captures.get(name).map(|c| c.node)
    }

    /// Get capture data (both node and text) for a capture
    pub fn capture(&self, name: &str) -> Option<&CaptureData<'a>> {
        self.captures.get(name)
    }

    /// Check if a capture exists
    pub fn has_capture(&self, name: &str) -> bool {
        self.captures.contains_key(name)
    }

    /// Iterate over all captures
    pub fn captures_iter(&self) -> impl Iterator<Item = (&'a str, &CaptureData<'a>)> {
        self.captures.iter().map(|(&k, v)| (k, v))
    }

    // === Node Traversal ===

    /// Get the primary captured node
    pub fn node(&self) -> Node<'a> {
        self.node
    }

    /// Get the source code bytes for extracting text from nodes
    pub fn source_bytes(&self) -> &[u8] {
        self.source.as_bytes()
    }

    /// Check if the primary node has a child with the specified field name
    pub fn has_child_field(&self, field_name: &str) -> bool {
        self.node.child_by_field_name(field_name).is_some()
    }

    /// Get a child node by field name
    pub fn child_by_field(&self, field_name: &str) -> Option<Node<'a>> {
        self.node.child_by_field_name(field_name)
    }

    /// Get text of a child node by field name
    pub fn child_text(&self, field_name: &str) -> Option<&'a str> {
        self.node
            .child_by_field_name(field_name)
            .and_then(|n| n.utf8_text(self.source.as_bytes()).ok())
    }

    /// Find an ancestor of a specific kind
    pub fn find_ancestor(&self, kind: &str) -> Option<Node<'a>> {
        let mut current = self.node;
        while let Some(parent) = current.parent() {
            if parent.kind() == kind {
                return Some(parent);
            }
            current = parent;
        }
        None
    }

    /// Check if the node has an ancestor of a specific kind
    pub fn has_ancestor(&self, kind: &str) -> bool {
        self.find_ancestor(kind).is_some()
    }

    /// Get all children of a specific kind
    pub fn children_of_kind(&self, kind: &str) -> Vec<Node<'a>> {
        let mut cursor = self.node.walk();
        self.node
            .children(&mut cursor)
            .filter(|n| n.kind() == kind)
            .collect()
    }

    /// Get text of the primary node
    pub fn node_text(&self) -> Result<&'a str> {
        self.node
            .utf8_text(self.source.as_bytes())
            .map_err(|e| Error::entity_extraction(format!("Failed to extract node text: {e}")))
    }

    // === Source Access ===

    /// Get the source code
    pub fn source(&self) -> &'a str {
        self.source
    }

    /// Get the file path
    pub fn file_path(&self) -> &'a Path {
        self.file_path
    }

    /// Get the source root (if available)
    pub fn source_root(&self) -> Option<&'a Path> {
        self.source_root
    }

    /// Get the repository root
    pub fn repo_root(&self) -> &'a Path {
        self.repo_root
    }

    // === Semantic Context ===

    /// Get the pre-computed scope stack
    pub fn current_scope(&self) -> &[String] {
        &self.scope_stack
    }

    /// Get the scope as a joined string with the language's separator
    pub fn scope_string(&self, separator: &str) -> String {
        self.scope_stack.join(separator)
    }

    /// Get the language enum
    pub fn language(&self) -> Language {
        self.language
    }

    /// Get the language string identifier
    pub fn language_str(&self) -> &'a str {
        self.language_str
    }

    /// Get the repository ID
    pub fn repository_id(&self) -> &'a str {
        self.repository_id
    }

    /// Get the package name (if available)
    pub fn package_name(&self) -> Option<&'a str> {
        self.package_name
    }

    /// Get the import map for identifier resolution
    pub fn import_map(&self) -> &'a ImportMap {
        self.import_map
    }

    /// Get the path configuration for reference resolution
    pub fn path_config(&self) -> &'static PathConfig {
        self.path_config
    }

    /// Get the edge case handlers (if available)
    pub fn edge_case_handlers(&self) -> Option<&'a EdgeCaseRegistry> {
        self.edge_case_handlers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::path_config::RUST_PATH_CONFIG;

    #[test]
    fn test_capture_access() {
        let source = "fn test() {}";
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let import_map = ImportMap::default();

        let func_node = tree.root_node().child(0).unwrap();
        let name_node = func_node.child_by_field_name("name").unwrap();

        let captures = HashMap::from([(
            "name",
            CaptureData {
                node: name_node,
                text: "test",
            },
        )]);

        let ctx = ExtractContext::builder()
            .node(func_node)
            .source(source)
            .captures(captures)
            .file_path(Path::new("/test/file.rs"))
            .import_map(&import_map)
            .language(Language::Rust)
            .language_str("rust")
            .repository_id("test-repo")
            .repo_root(Path::new("/test"))
            .path_config(&RUST_PATH_CONFIG)
            .build()
            .unwrap();

        // Test required capture access
        assert_eq!(ctx.capture_text("name").unwrap(), "test");
        assert!(ctx.capture_node("name").is_ok());

        // Test optional capture access
        assert_eq!(ctx.capture_text_opt("name"), Some("test"));
        assert!(ctx.capture_text_opt("nonexistent").is_none());

        // Test has_capture
        assert!(ctx.has_capture("name"));
        assert!(!ctx.has_capture("nonexistent"));

        // Test missing required capture returns error
        assert!(ctx.capture_text("missing").is_err());
    }

    #[test]
    fn test_node_traversal() {
        let source = r#"
            impl MyStruct {
                fn method(&self) {}
            }
        "#;

        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let import_map = ImportMap::default();

        // Find the function_item inside the impl
        let impl_node = tree.root_node().child(0).unwrap();
        let decl_list = impl_node.child_by_field_name("body").unwrap();
        let func_node = decl_list.child(1).unwrap(); // Skip the whitespace

        let ctx = ExtractContext::builder()
            .node(func_node)
            .source(source)
            .file_path(Path::new("/test/file.rs"))
            .import_map(&import_map)
            .language(Language::Rust)
            .language_str("rust")
            .repository_id("test-repo")
            .repo_root(Path::new("/test"))
            .path_config(&RUST_PATH_CONFIG)
            .build()
            .unwrap();

        // Test find_ancestor
        let ancestor = ctx.find_ancestor("impl_item");
        assert!(ancestor.is_some());
        assert_eq!(ancestor.unwrap().kind(), "impl_item");

        // Test has_ancestor
        assert!(ctx.has_ancestor("impl_item"));
        assert!(!ctx.has_ancestor("mod_item"));

        // Test has_child_field
        assert!(ctx.has_child_field("name"));
        assert!(ctx.has_child_field("parameters"));
        assert!(!ctx.has_child_field("nonexistent"));
    }

    #[test]
    fn test_builder_missing_required_fields() {
        let result = ExtractContext::builder().build();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("node is required"));
    }

    #[test]
    fn test_scope_stack() {
        let source = "fn test() {}";
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let import_map = ImportMap::default();

        let scope = vec!["my_crate".to_string(), "module".to_string()];

        let ctx = ExtractContext::builder()
            .node(tree.root_node())
            .source(source)
            .file_path(Path::new("/test/file.rs"))
            .import_map(&import_map)
            .language(Language::Rust)
            .language_str("rust")
            .repository_id("test-repo")
            .repo_root(Path::new("/test"))
            .path_config(&RUST_PATH_CONFIG)
            .scope_stack(scope)
            .build()
            .unwrap();

        assert_eq!(ctx.current_scope(), &["my_crate", "module"]);
        assert_eq!(ctx.scope_string("::"), "my_crate::module");
    }
}
