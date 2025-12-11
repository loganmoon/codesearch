//! Graph types for cross-file FQN resolution
//!
//! This module defines the node and edge types used for building a resolution graph
//! that can follow import/export chains to find canonical definition-site FQNs.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The kind of node in the resolution graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResolutionNodeKind {
    /// A definition site (struct, fn, trait, enum, const, etc.)
    Definition,
    /// A public export (`pub use` re-export)
    Export,
    /// An import declaration (`use` statement)
    Import,
    /// A reference to an identifier (usage site)
    Reference,
}

impl ResolutionNodeKind {
    /// Get the Neo4j label for this node kind
    pub fn neo4j_label(&self) -> &'static str {
        match self {
            ResolutionNodeKind::Definition => "Definition",
            ResolutionNodeKind::Export => "Export",
            ResolutionNodeKind::Import => "Import",
            ResolutionNodeKind::Reference => "Reference",
        }
    }
}

/// A node in the resolution graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionNode {
    /// The kind of resolution node
    pub kind: ResolutionNodeKind,
    /// Simple name (e.g., "Foo", "my_function")
    pub name: String,
    /// File-path-derived fully qualified name
    pub qualified_name: String,
    /// Path to the source file
    pub file_path: PathBuf,
    /// Start line (1-indexed)
    pub start_line: u32,
    /// End line (1-indexed)
    pub end_line: u32,
    /// Visibility modifier (e.g., "pub", "pub(crate)", None for private)
    pub visibility: Option<String>,
    /// For imports: the source path being imported (e.g., "std::io::Read")
    pub import_path: Option<String>,
    /// For exports: what this re-exports (e.g., "internal::Foo")
    pub reexport_source: Option<String>,
    /// For definitions: the kind of definition (e.g., "struct", "function", "trait")
    pub definition_kind: Option<String>,
    /// For imports: whether this is a glob import (use foo::*)
    pub is_glob: bool,
    /// For references: the context (e.g., "call", "type", "field")
    pub reference_context: Option<String>,
}

impl ResolutionNode {
    /// Create a new Definition node
    pub fn definition(
        name: String,
        qualified_name: String,
        file_path: PathBuf,
        start_line: u32,
        end_line: u32,
        visibility: Option<String>,
        definition_kind: String,
    ) -> Self {
        Self {
            kind: ResolutionNodeKind::Definition,
            name,
            qualified_name,
            file_path,
            start_line,
            end_line,
            visibility,
            import_path: None,
            reexport_source: None,
            definition_kind: Some(definition_kind),
            is_glob: false,
            reference_context: None,
        }
    }

    /// Create a new Export node
    pub fn export(
        name: String,
        qualified_name: String,
        file_path: PathBuf,
        start_line: u32,
        end_line: u32,
        reexport_source: String,
    ) -> Self {
        Self {
            kind: ResolutionNodeKind::Export,
            name,
            qualified_name,
            file_path,
            start_line,
            end_line,
            visibility: Some("pub".to_string()),
            import_path: None,
            reexport_source: Some(reexport_source),
            definition_kind: None,
            is_glob: false,
            reference_context: None,
        }
    }

    /// Create a new Import node
    pub fn import(
        name: String,
        qualified_name: String,
        file_path: PathBuf,
        start_line: u32,
        end_line: u32,
        import_path: String,
        is_glob: bool,
    ) -> Self {
        Self {
            kind: ResolutionNodeKind::Import,
            name,
            qualified_name,
            file_path,
            start_line,
            end_line,
            visibility: None,
            import_path: Some(import_path),
            reexport_source: None,
            definition_kind: None,
            is_glob,
            reference_context: None,
        }
    }

    /// Create a new Reference node
    pub fn reference(
        name: String,
        qualified_name: String,
        file_path: PathBuf,
        start_line: u32,
        end_line: u32,
        context: Option<String>,
    ) -> Self {
        Self {
            kind: ResolutionNodeKind::Reference,
            name,
            qualified_name,
            file_path,
            start_line,
            end_line,
            visibility: None,
            import_path: None,
            reexport_source: None,
            definition_kind: None,
            is_glob: false,
            reference_context: context,
        }
    }

    /// Generate a unique ID for this node (for Neo4j)
    pub fn node_id(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.kind.neo4j_label().hash(&mut hasher);
        self.qualified_name.hash(&mut hasher);
        self.file_path.hash(&mut hasher);
        self.start_line.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

/// Edge types for the resolution graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResolutionEdgeKind {
    /// Reference resolves through an import (Reference → Import, same file)
    ResolvesTo,
    /// Import targets an export or definition (Import → Export/Definition, cross-file)
    ImportsFrom,
    /// Export re-exports another export or definition (Export → Export/Definition, cross-file)
    Reexports,
}

impl ResolutionEdgeKind {
    /// Get the Neo4j relationship type for this edge kind
    pub fn neo4j_type(&self) -> &'static str {
        match self {
            ResolutionEdgeKind::ResolvesTo => "RESOLVES_TO",
            ResolutionEdgeKind::ImportsFrom => "IMPORTS_FROM",
            ResolutionEdgeKind::Reexports => "REEXPORTS",
        }
    }
}

/// An edge in the resolution graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionEdge {
    /// The kind of edge
    pub kind: ResolutionEdgeKind,
    /// Source node ID
    pub from_id: String,
    /// Target node ID
    pub to_id: String,
}

impl ResolutionEdge {
    /// Create a new edge
    pub fn new(kind: ResolutionEdgeKind, from_id: String, to_id: String) -> Self {
        Self {
            kind,
            from_id,
            to_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definition_node() {
        let node = ResolutionNode::definition(
            "Foo".to_string(),
            "mymod::Foo".to_string(),
            PathBuf::from("src/mymod.rs"),
            10,
            20,
            Some("pub".to_string()),
            "struct".to_string(),
        );

        assert_eq!(node.kind, ResolutionNodeKind::Definition);
        assert_eq!(node.name, "Foo");
        assert_eq!(node.definition_kind, Some("struct".to_string()));
        assert!(!node.is_glob);
    }

    #[test]
    fn test_import_glob() {
        let node = ResolutionNode::import(
            "*".to_string(),
            "mymod::*".to_string(),
            PathBuf::from("src/lib.rs"),
            5,
            5,
            "std::prelude::*".to_string(),
            true,
        );

        assert_eq!(node.kind, ResolutionNodeKind::Import);
        assert!(node.is_glob);
        assert_eq!(node.import_path, Some("std::prelude::*".to_string()));
    }

    #[test]
    fn test_node_id_uniqueness() {
        let node1 = ResolutionNode::definition(
            "Foo".to_string(),
            "mod1::Foo".to_string(),
            PathBuf::from("src/mod1.rs"),
            10,
            20,
            None,
            "struct".to_string(),
        );

        let node2 = ResolutionNode::definition(
            "Foo".to_string(),
            "mod2::Foo".to_string(),
            PathBuf::from("src/mod2.rs"),
            10,
            20,
            None,
            "struct".to_string(),
        );

        assert_ne!(node1.node_id(), node2.node_id());
    }
}
