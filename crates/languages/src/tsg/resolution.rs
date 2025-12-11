//! Cross-file FQN resolution using Neo4j
//!
//! This module provides cross-file resolution by:
//! 1. Creating ephemeral Import/Reference nodes in Neo4j
//! 2. Linking Import nodes to existing Entity nodes via qualified_name
//! 3. Building resolution edges (RESOLVES_TO, IMPORTS_FROM)
//! 4. Traversing the graph to get canonical FQNs

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use super::graph_types::{ResolutionNode, ResolutionNodeKind};
use std::collections::HashMap;
use uuid::Uuid;

/// Resolution session for tracking ephemeral nodes
#[derive(Debug, Clone)]
pub struct ResolutionSession {
    /// Unique session ID for node cleanup
    pub session_id: Uuid,
    /// Repository being resolved
    pub repository_id: Uuid,
}

impl ResolutionSession {
    /// Create a new resolution session
    pub fn new(repository_id: Uuid) -> Self {
        Self {
            session_id: Uuid::new_v4(),
            repository_id,
        }
    }
}

/// Statistics from cross-file resolution
#[derive(Debug, Clone, Default)]
pub struct ResolutionStats {
    /// Total Import nodes created
    pub total_imports: usize,
    /// Total Reference nodes created
    pub total_references: usize,
    /// Imports that resolved to Entity nodes
    pub imports_resolved: usize,
    /// Imports that couldn't find a matching Entity
    pub imports_unresolved: usize,
    /// References that resolved (to Import or local Definition)
    pub references_resolved: usize,
    /// References that couldn't resolve
    pub references_unresolved: usize,
}

impl ResolutionStats {
    /// Compute the overall resolution rate
    pub fn resolution_rate(&self) -> f64 {
        if self.total_references == 0 {
            return 0.0;
        }
        self.references_resolved as f64 / self.total_references as f64
    }

    /// Compute the import resolution rate
    pub fn import_resolution_rate(&self) -> f64 {
        if self.total_imports == 0 {
            return 0.0;
        }
        self.imports_resolved as f64 / self.total_imports as f64
    }
}

/// Result of cross-file resolution
#[derive(Debug)]
pub struct ResolutionResult {
    /// Statistics about the resolution
    pub stats: ResolutionStats,
    /// Map from (file_path, local_name) -> canonical FQN
    pub fqn_map: HashMap<(String, String), String>,
    /// Unresolved import paths
    pub unresolved_imports: Vec<String>,
}

/// Cypher queries for resolution graph operations
pub mod queries {
    /// Create Import nodes from ResolutionNodes
    pub const CREATE_IMPORT_NODES: &str = r#"
        UNWIND $imports AS import
        CREATE (n:Import:ResolutionNode {
            session_id: $session_id,
            repository_id: $repository_id,
            name: import.name,
            import_path: import.import_path,
            file_path: import.file_path,
            start_line: import.start_line,
            is_glob: import.is_glob
        })
    "#;

    /// Create Reference nodes from ResolutionNodes
    pub const CREATE_REFERENCE_NODES: &str = r#"
        UNWIND $references AS ref
        CREATE (n:Reference:ResolutionNode {
            session_id: $session_id,
            repository_id: $repository_id,
            name: ref.name,
            file_path: ref.file_path,
            start_line: ref.start_line,
            context: ref.context
        })
    "#;

    /// Create Definition nodes (lightweight, for items not yet in Entity graph)
    pub const CREATE_DEFINITION_NODES: &str = r#"
        UNWIND $definitions AS def
        CREATE (n:Definition:ResolutionNode {
            session_id: $session_id,
            repository_id: $repository_id,
            name: def.name,
            qualified_name: def.qualified_name,
            file_path: def.file_path,
            start_line: def.start_line,
            visibility: def.visibility,
            definition_kind: def.definition_kind
        })
    "#;

    /// Build RESOLVES_TO edges (Reference -> Import, same file, by name)
    pub const BUILD_RESOLVES_TO_EDGES: &str = r#"
        MATCH (ref:Reference:ResolutionNode {session_id: $session_id})
        MATCH (import:Import:ResolutionNode {session_id: $session_id})
        WHERE ref.file_path = import.file_path
          AND ref.name = import.name
        CREATE (ref)-[:RESOLVES_TO]->(import)
    "#;

    /// Build RESOLVES_TO edges (Reference -> Definition, same file, by name)
    pub const BUILD_RESOLVES_TO_DEFINITION_EDGES: &str = r#"
        MATCH (ref:Reference:ResolutionNode {session_id: $session_id})
        MATCH (def:Definition:ResolutionNode {session_id: $session_id})
        WHERE ref.file_path = def.file_path
          AND ref.name = def.name
        CREATE (ref)-[:RESOLVES_TO]->(def)
    "#;

    /// Build IMPORTS_FROM edges (Import -> Entity, by qualified_name)
    /// This links ephemeral Import nodes to persistent Entity nodes
    pub const BUILD_IMPORTS_FROM_ENTITY_EDGES: &str = r#"
        MATCH (import:Import:ResolutionNode {session_id: $session_id})
        MATCH (entity:Entity {repository_id: $repository_id})
        WHERE entity.qualified_name = import.import_path
        CREATE (import)-[:IMPORTS_FROM]->(entity)
    "#;

    /// Build IMPORTS_FROM edges (Import -> Definition, for same-crate imports)
    pub const BUILD_IMPORTS_FROM_DEFINITION_EDGES: &str = r#"
        MATCH (import:Import:ResolutionNode {session_id: $session_id})
        MATCH (def:Definition:ResolutionNode {session_id: $session_id})
        WHERE def.qualified_name = import.import_path
        CREATE (import)-[:IMPORTS_FROM]->(def)
    "#;

    /// Count resolved imports (those with IMPORTS_FROM edges)
    pub const COUNT_RESOLVED_IMPORTS: &str = r#"
        MATCH (import:Import:ResolutionNode {session_id: $session_id})
        OPTIONAL MATCH (import)-[:IMPORTS_FROM]->(target)
        RETURN
            count(import) AS total_imports,
            count(target) AS resolved_imports
    "#;

    /// Count resolved references (those with RESOLVES_TO edges)
    pub const COUNT_RESOLVED_REFERENCES: &str = r#"
        MATCH (ref:Reference:ResolutionNode {session_id: $session_id})
        OPTIONAL MATCH (ref)-[:RESOLVES_TO]->(target)
        RETURN
            count(ref) AS total_references,
            count(target) AS resolved_references
    "#;

    /// Get unresolved imports
    pub const GET_UNRESOLVED_IMPORTS: &str = r#"
        MATCH (import:Import:ResolutionNode {session_id: $session_id})
        WHERE NOT (import)-[:IMPORTS_FROM]->()
        RETURN DISTINCT import.import_path AS import_path
        ORDER BY import_path
    "#;

    /// Get resolution chain for a reference (for debugging)
    pub const GET_RESOLUTION_CHAIN: &str = r#"
        MATCH (ref:Reference:ResolutionNode {session_id: $session_id, file_path: $file_path, name: $name})
        OPTIONAL MATCH path = (ref)-[:RESOLVES_TO|IMPORTS_FROM*1..5]->(target)
        WHERE target:Entity OR target:Definition
        RETURN ref.name AS reference_name,
               ref.file_path AS file_path,
               [n IN nodes(path) | labels(n)[0] + ':' + coalesce(n.name, n.qualified_name)] AS chain,
               target.qualified_name AS canonical_fqn
    "#;

    /// Cleanup: delete all nodes for a session
    pub const CLEANUP_SESSION: &str = r#"
        MATCH (n:ResolutionNode {session_id: $session_id})
        DETACH DELETE n
    "#;

    /// Create indexes for resolution nodes (run once at setup)
    pub const CREATE_RESOLUTION_INDEXES: &str = r#"
        CREATE INDEX resolution_session_idx IF NOT EXISTS FOR (n:ResolutionNode) ON (n.session_id);
        CREATE INDEX resolution_file_idx IF NOT EXISTS FOR (n:ResolutionNode) ON (n.file_path);
        CREATE INDEX import_path_idx IF NOT EXISTS FOR (n:Import) ON (n.import_path);
        CREATE INDEX reference_name_idx IF NOT EXISTS FOR (n:Reference) ON (n.name)
    "#;
}

/// Convert ResolutionNodes to maps for Neo4j UNWIND queries
pub fn nodes_to_import_maps(nodes: &[ResolutionNode]) -> Vec<HashMap<String, serde_json::Value>> {
    nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Import)
        .map(|n| {
            let mut map = HashMap::new();
            map.insert("name".to_string(), serde_json::json!(n.name));
            map.insert(
                "import_path".to_string(),
                serde_json::json!(n.import_path.as_deref().unwrap_or("")),
            );
            map.insert(
                "file_path".to_string(),
                serde_json::json!(n.file_path.to_string_lossy()),
            );
            map.insert("start_line".to_string(), serde_json::json!(n.start_line));
            map.insert("is_glob".to_string(), serde_json::json!(n.is_glob));
            map
        })
        .collect()
}

/// Convert ResolutionNodes to maps for Neo4j UNWIND queries
pub fn nodes_to_reference_maps(
    nodes: &[ResolutionNode],
) -> Vec<HashMap<String, serde_json::Value>> {
    nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Reference)
        .map(|n| {
            let mut map = HashMap::new();
            map.insert("name".to_string(), serde_json::json!(n.name));
            map.insert(
                "file_path".to_string(),
                serde_json::json!(n.file_path.to_string_lossy()),
            );
            map.insert("start_line".to_string(), serde_json::json!(n.start_line));
            map.insert(
                "context".to_string(),
                serde_json::json!(n.reference_context.as_deref().unwrap_or("")),
            );
            map
        })
        .collect()
}

/// Convert ResolutionNodes to maps for Neo4j UNWIND queries
pub fn nodes_to_definition_maps(
    nodes: &[ResolutionNode],
) -> Vec<HashMap<String, serde_json::Value>> {
    nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Definition)
        .map(|n| {
            let mut map = HashMap::new();
            map.insert("name".to_string(), serde_json::json!(n.name));
            map.insert(
                "qualified_name".to_string(),
                serde_json::json!(n.qualified_name),
            );
            map.insert(
                "file_path".to_string(),
                serde_json::json!(n.file_path.to_string_lossy()),
            );
            map.insert("start_line".to_string(), serde_json::json!(n.start_line));
            map.insert(
                "visibility".to_string(),
                serde_json::json!(n.visibility.as_deref().unwrap_or("")),
            );
            map.insert(
                "definition_kind".to_string(),
                serde_json::json!(n.definition_kind.as_deref().unwrap_or("")),
            );
            map
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_resolution_session_creation() {
        let repo_id = Uuid::new_v4();
        let session = ResolutionSession::new(repo_id);

        assert_eq!(session.repository_id, repo_id);
        assert_ne!(session.session_id, Uuid::nil());
    }

    #[test]
    fn test_resolution_stats() {
        let stats = ResolutionStats {
            total_references: 100,
            references_resolved: 80,
            total_imports: 50,
            imports_resolved: 40,
            ..Default::default()
        };

        assert!((stats.resolution_rate() - 0.8).abs() < 0.001);
        assert!((stats.import_resolution_rate() - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_nodes_to_import_maps() {
        let nodes = vec![
            ResolutionNode::import(
                "Read".to_string(),
                "test::Read".to_string(),
                PathBuf::from("test.rs"),
                1,
                1,
                "std::io::Read".to_string(),
                false,
            ),
            ResolutionNode::definition(
                "Foo".to_string(),
                "test::Foo".to_string(),
                PathBuf::from("test.rs"),
                5,
                10,
                Some("pub".to_string()),
                "struct".to_string(),
            ),
        ];

        let maps = nodes_to_import_maps(&nodes);

        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0]["name"], serde_json::json!("Read"));
        assert_eq!(maps[0]["import_path"], serde_json::json!("std::io::Read"));
    }

    #[test]
    fn test_nodes_to_definition_maps() {
        let nodes = vec![ResolutionNode::definition(
            "Foo".to_string(),
            "mymod::Foo".to_string(),
            PathBuf::from("src/mymod.rs"),
            5,
            10,
            Some("pub".to_string()),
            "struct".to_string(),
        )];

        let maps = nodes_to_definition_maps(&nodes);

        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0]["name"], serde_json::json!("Foo"));
        assert_eq!(maps[0]["qualified_name"], serde_json::json!("mymod::Foo"));
        assert_eq!(maps[0]["visibility"], serde_json::json!("pub"));
    }
}
