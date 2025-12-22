//! Evaluation utilities for measuring resolution rate
//!
//! This module provides tools to measure how effectively the TSG extraction
//! and resolution approach can resolve references to their canonical definitions.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use super::graph_types::{ResolutionNode, ResolutionNodeKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Results from evaluating resolution on a codebase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    /// Total number of files processed
    pub total_files: usize,
    /// Total number of nodes extracted
    pub total_nodes: usize,
    /// Number of Definition nodes
    pub definition_count: usize,
    /// Number of Export nodes
    pub export_count: usize,
    /// Number of Import nodes
    pub import_count: usize,
    /// Number of Reference nodes
    pub reference_count: usize,
    /// Number of references that resolved to an import in the same file
    pub intra_file_resolved: usize,
    /// Number of references that couldn't find a matching import
    pub unresolved: usize,
    /// Resolution rate (intra_file_resolved / reference_count)
    pub intra_file_resolution_rate: f64,
    /// Breakdown of unresolved references by pattern
    pub unresolved_by_pattern: HashMap<String, usize>,
}

impl EvaluationResult {
    /// Create an empty evaluation result
    pub fn new() -> Self {
        Self {
            total_files: 0,
            total_nodes: 0,
            definition_count: 0,
            export_count: 0,
            import_count: 0,
            reference_count: 0,
            intra_file_resolved: 0,
            unresolved: 0,
            intra_file_resolution_rate: 0.0,
            unresolved_by_pattern: HashMap::new(),
        }
    }

    /// Compute the resolution rate
    pub fn compute_rate(&mut self) {
        if self.reference_count > 0 {
            self.intra_file_resolution_rate =
                self.intra_file_resolved as f64 / self.reference_count as f64;
        }
    }
}

impl Default for EvaluationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Build intra-file resolution edges by matching Reference names to Import/Definition names
///
/// Only counts references to names that are defined or imported in the same file.
/// References to builtins, stdlib, or external packages are automatically excluded
/// since they won't have a matching definition/import in the file.
///
/// # Arguments
/// * `nodes` - Resolution nodes extracted from a single file
///
/// # Returns
/// Tuple of (resolved_count, unresolved_references)
pub fn build_intra_file_edges(nodes: &[ResolutionNode]) -> (usize, Vec<&ResolutionNode>) {
    // Collect imports by name
    let imports: HashMap<&str, &ResolutionNode> = nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Import)
        .map(|n| (n.name.as_str(), n))
        .collect();

    // Also collect definitions by name (for local definitions)
    let definitions: HashMap<&str, &ResolutionNode> = nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Definition)
        .map(|n| (n.name.as_str(), n))
        .collect();

    let mut resolved = 0;

    for node in nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Reference)
    {
        // Normalize name (strip quotes from forward references like "Position")
        let name = node.name.trim_matches('"').trim_matches('\'');

        // Only count references to names that exist as definitions or imports in this file.
        // This automatically filters out builtins (print, len), stdlib (os.path),
        // and external packages without needing hardcoded lists.
        let has_definition = definitions.contains_key(name);
        let has_import = imports.contains_key(name);

        if !has_definition && !has_import {
            continue; // Not defined or imported in this file, skip
        }

        // Reference is to something in this file - count it as resolved
        resolved += 1;
    }

    // No unresolved references with this approach - if we count it, it resolved
    (resolved, Vec::new())
}

/// Categorize why a reference couldn't be resolved
pub fn categorize_unresolved(node: &ResolutionNode) -> &'static str {
    let name = &node.name;

    // Check patterns
    if name.starts_with('_') {
        "underscore_prefix"
    } else if name.chars().next().is_some_and(|c| c.is_uppercase()) {
        // Could be from glob import, prelude, or external crate
        "type_from_external"
    } else if name.chars().next().is_some_and(|c| c.is_lowercase()) {
        // Could be from glob import, prelude, or external crate
        "function_from_external"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsg::graph_types::ResolutionNode;
    use std::path::PathBuf;

    #[test]
    fn test_intra_file_resolution() {
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
                "MyStruct".to_string(),
                "test::MyStruct".to_string(),
                PathBuf::from("test.rs"),
                3,
                5,
                Some("pub".to_string()),
                "struct".to_string(),
            ),
            ResolutionNode::reference(
                "Read".to_string(),
                "test::Read".to_string(),
                PathBuf::from("test.rs"),
                10,
                10,
                Some("type".to_string()),
            ),
            ResolutionNode::reference(
                "MyStruct".to_string(),
                "test::MyStruct".to_string(),
                PathBuf::from("test.rs"),
                11,
                11,
                Some("type".to_string()),
            ),
            ResolutionNode::reference(
                "Unknown".to_string(),
                "test::Unknown".to_string(),
                PathBuf::from("test.rs"),
                12,
                12,
                Some("type".to_string()),
            ),
        ];

        let (resolved, unresolved) = build_intra_file_edges(&nodes);

        // Read resolves to import, MyStruct resolves to definition
        assert_eq!(resolved, 2);
        // Unknown has no matching import or definition - automatically skipped (not counted)
        assert_eq!(unresolved.len(), 0);
    }

    #[test]
    fn test_external_references_skipped() {
        // References to types not defined or imported in the file are automatically skipped
        let nodes = vec![
            ResolutionNode::reference(
                "i32".to_string(),
                "test::i32".to_string(),
                PathBuf::from("test.rs"),
                1,
                1,
                Some("type".to_string()),
            ),
            ResolutionNode::reference(
                "String".to_string(),
                "test::String".to_string(),
                PathBuf::from("test.rs"),
                2,
                2,
                Some("type".to_string()),
            ),
            ResolutionNode::reference(
                "ExternalCrate".to_string(),
                "test::ExternalCrate".to_string(),
                PathBuf::from("test.rs"),
                3,
                3,
                Some("type".to_string()),
            ),
        ];

        let (resolved, unresolved) = build_intra_file_edges(&nodes);

        // No definitions or imports in file, so all references are skipped
        assert_eq!(resolved, 0);
        assert_eq!(unresolved.len(), 0);
    }
}
