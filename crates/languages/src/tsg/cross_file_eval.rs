//! Cross-file resolution evaluation (in-memory simulation)
//!
//! This module simulates cross-file resolution without Neo4j by building
//! in-memory lookup tables. This is useful for evaluation and testing.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use super::evaluation::is_primitive_or_prelude;
use super::executor::TsgExecutor;
use super::graph_types::{ResolutionNode, ResolutionNodeKind};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use walkdir::WalkDir;

/// Statistics from cross-file resolution evaluation
#[derive(Debug, Clone, Default)]
pub struct CrossFileEvalStats {
    /// Total files processed
    pub total_files: usize,
    /// Total definitions extracted
    pub total_definitions: usize,
    /// Total imports extracted
    pub total_imports: usize,
    /// Total references extracted (excluding primitives/prelude)
    pub total_references: usize,
    /// References resolved via same-file definitions
    pub resolved_via_local_definition: usize,
    /// References resolved via same-file import
    pub resolved_via_import: usize,
    /// Imports resolved to definitions in other files
    pub imports_resolved_cross_file: usize,
    /// Imports that couldn't be resolved
    pub imports_unresolved: usize,
    /// References that couldn't be resolved at all
    pub references_unresolved: usize,
    /// Top unresolved import paths
    pub unresolved_import_paths: HashMap<String, usize>,
    /// Top unresolved reference names
    pub unresolved_reference_names: HashMap<String, usize>,
}

impl CrossFileEvalStats {
    /// Calculate overall resolution rate (references that can reach a definition)
    pub fn resolution_rate(&self) -> f64 {
        if self.total_references == 0 {
            return 0.0;
        }
        let resolved = self.resolved_via_local_definition
            + self
                .resolved_via_import
                .min(self.imports_resolved_cross_file);
        resolved as f64 / self.total_references as f64
    }

    /// Calculate import resolution rate
    pub fn import_resolution_rate(&self) -> f64 {
        if self.total_imports == 0 {
            return 0.0;
        }
        self.imports_resolved_cross_file as f64 / self.total_imports as f64
    }

    /// Print summary statistics
    pub fn print_summary(&self) {
        println!("\n=== Cross-File Resolution Evaluation ===\n");
        println!("Files processed: {}", self.total_files);
        println!("Definitions extracted: {}", self.total_definitions);
        println!("Imports extracted: {}", self.total_imports);
        println!(
            "References extracted (excluding primitives): {}",
            self.total_references
        );
        println!();
        println!("Resolution breakdown:");
        println!(
            "  Via local definition: {} ({:.1}%)",
            self.resolved_via_local_definition,
            self.resolved_via_local_definition as f64 / self.total_references as f64 * 100.0
        );
        println!(
            "  Via import: {} ({:.1}%)",
            self.resolved_via_import,
            self.resolved_via_import as f64 / self.total_references as f64 * 100.0
        );
        println!(
            "  Unresolved: {} ({:.1}%)",
            self.references_unresolved,
            self.references_unresolved as f64 / self.total_references as f64 * 100.0
        );
        println!();
        println!("Import resolution:");
        println!(
            "  Cross-file resolved: {} ({:.1}%)",
            self.imports_resolved_cross_file,
            self.import_resolution_rate() * 100.0
        );
        println!(
            "  Unresolved: {} ({:.1}%)",
            self.imports_unresolved,
            self.imports_unresolved as f64 / self.total_imports as f64 * 100.0
        );
        println!();
        println!(
            "Overall resolution rate: {:.1}%",
            self.resolution_rate() * 100.0
        );
    }
}

/// Evaluate cross-file resolution on a codebase
///
/// This builds lookup tables in-memory to simulate what Neo4j would do:
/// 1. Extract all nodes from all files
/// 2. Build definition lookup by qualified_name
/// 3. For each file, try to resolve references through imports
pub fn evaluate_cross_file_resolution(codebase_path: &Path) -> Result<CrossFileEvalStats> {
    let mut executor = TsgExecutor::new_rust()?;
    let mut stats = CrossFileEvalStats::default();

    // Phase 1: Extract all nodes from all files
    let mut all_nodes: Vec<ResolutionNode> = Vec::new();
    let mut nodes_by_file: HashMap<String, Vec<ResolutionNode>> = HashMap::new();

    for entry in WalkDir::new(codebase_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "rs")
                && !e.path().to_string_lossy().contains("/target/")
        })
    {
        let file_path = entry.path();
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let nodes = match executor.extract(&source, file_path) {
            Ok(n) => n,
            Err(_) => continue,
        };

        stats.total_files += 1;
        let file_key = file_path.to_string_lossy().to_string();
        nodes_by_file.insert(file_key, nodes.clone());
        all_nodes.extend(nodes);
    }

    // Phase 2: Build global definition lookup by qualified_name
    // In a real system, this would be the Entity nodes in Neo4j
    let mut definitions_by_qname: HashMap<&str, &ResolutionNode> = HashMap::new();
    let mut definitions_by_name: HashMap<&str, Vec<&ResolutionNode>> = HashMap::new();

    for node in &all_nodes {
        if node.kind == ResolutionNodeKind::Definition {
            stats.total_definitions += 1;
            definitions_by_qname.insert(&node.qualified_name, node);
            definitions_by_name
                .entry(&node.name)
                .or_default()
                .push(node);
        }
    }

    // Phase 3: For each file, evaluate resolution
    for nodes in nodes_by_file.values() {
        // Build file-local lookups
        let local_definitions: HashSet<&str> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Definition)
            .map(|n| n.name.as_str())
            .collect();

        let local_imports: HashMap<&str, &ResolutionNode> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Import)
            .map(|n| (n.name.as_str(), n))
            .collect();

        // Count imports
        for node in nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Import)
        {
            stats.total_imports += 1;

            // Try to resolve import to a definition
            if let Some(import_path) = &node.import_path {
                // Try exact qualified_name match or simple name match (for crate-internal imports)
                let resolved = definitions_by_qname.contains_key(import_path.as_str())
                    || definitions_by_name.contains_key(node.name.as_str());

                if resolved {
                    stats.imports_resolved_cross_file += 1;
                } else {
                    stats.imports_unresolved += 1;
                    *stats
                        .unresolved_import_paths
                        .entry(import_path.clone())
                        .or_insert(0) += 1;
                }
            }
        }

        // Count references and resolution
        for node in nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Reference)
        {
            // Skip primitives and prelude
            if is_primitive_or_prelude(&node.name) {
                continue;
            }

            stats.total_references += 1;

            // Try to resolve: first local definitions, then imports
            if local_definitions.contains(node.name.as_str()) {
                stats.resolved_via_local_definition += 1;
            } else if local_imports.contains_key(node.name.as_str()) {
                stats.resolved_via_import += 1;
            } else {
                stats.references_unresolved += 1;
                *stats
                    .unresolved_reference_names
                    .entry(node.name.clone())
                    .or_insert(0) += 1;
            }
        }
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cross_file_stats_calculation() {
        let stats = CrossFileEvalStats {
            total_references: 100,
            resolved_via_local_definition: 30,
            resolved_via_import: 50,
            imports_resolved_cross_file: 40,
            references_unresolved: 20,
            ..Default::default()
        };

        // With 50 resolved via import but only 40 imports resolved cross-file,
        // effective resolution = 30 (local) + 40 (import that resolved) = 70%
        assert!((stats.resolution_rate() - 0.70).abs() < 0.001);
    }
}
