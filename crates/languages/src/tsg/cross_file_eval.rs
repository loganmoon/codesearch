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

/// Configuration for cross-file resolution evaluation
pub struct CrossFileEvalConfig<'a> {
    /// File extension to process (e.g., "rs", "js", "ts", "py")
    pub extension: &'a str,
    /// Directories to skip during traversal
    pub skip_dirs: &'a [&'a str],
    /// Function to check if a name is a builtin (should be skipped in resolution)
    pub is_builtin: fn(&str) -> bool,
}

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
    /// Only counts internal references (excludes references to external dependencies)
    pub fn resolution_rate(&self) -> f64 {
        if self.total_references == 0 {
            return 0.0;
        }
        let resolved = self.resolved_via_local_definition + self.resolved_via_import;
        resolved as f64 / self.total_references as f64
    }

    /// Calculate import resolution rate
    pub fn import_resolution_rate(&self) -> f64 {
        if self.total_imports == 0 {
            return 0.0;
        }
        self.imports_resolved_cross_file as f64 / self.total_imports as f64
    }

    /// Calculate percentage, returning 0.0 if denominator is zero
    fn percent_of(numerator: usize, denominator: usize) -> f64 {
        if denominator == 0 {
            0.0
        } else {
            numerator as f64 / denominator as f64 * 100.0
        }
    }

    /// Print summary statistics
    pub fn print_summary(&self) {
        println!("\n=== Cross-File Resolution Evaluation ===\n");
        println!("Files processed: {}", self.total_files);
        println!("Definitions extracted: {}", self.total_definitions);
        println!(
            "Internal imports: {} (external imports excluded)",
            self.total_imports
        );
        println!(
            "Internal references: {} (references to external deps excluded)",
            self.total_references
        );
        println!();
        println!("Resolution breakdown:");
        println!(
            "  Via local definition: {} ({:.1}%)",
            self.resolved_via_local_definition,
            Self::percent_of(self.resolved_via_local_definition, self.total_references)
        );
        println!(
            "  Via internal import: {} ({:.1}%)",
            self.resolved_via_import,
            Self::percent_of(self.resolved_via_import, self.total_references)
        );
        println!(
            "  Unresolved: {} ({:.1}%)",
            self.references_unresolved,
            Self::percent_of(self.references_unresolved, self.total_references)
        );
        println!();
        println!(
            "Overall resolution rate: {:.1}%",
            self.resolution_rate() * 100.0
        );

        // Print top unresolved references
        if !self.unresolved_reference_names.is_empty() {
            println!("\nTop unresolved reference names:");
            let mut sorted: Vec<_> = self.unresolved_reference_names.iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(a.1));
            for (name, count) in sorted.iter().take(10) {
                println!("  {name}: {count}");
            }
        }
    }
}

/// Evaluate cross-file resolution on a Rust codebase
///
/// This is a convenience wrapper around `evaluate_cross_file_resolution_with_config`
/// that uses Rust-specific settings.
pub fn evaluate_cross_file_resolution(codebase_path: &Path) -> Result<CrossFileEvalStats> {
    let executor = TsgExecutor::new_rust()?;
    let config = CrossFileEvalConfig {
        extension: "rs",
        skip_dirs: &["target", ".git"],
        is_builtin: is_primitive_or_prelude,
    };
    evaluate_cross_file_resolution_with_config(codebase_path, executor, &config)
}

/// Evaluate cross-file resolution on a codebase with the given executor and config
///
/// This builds lookup tables in-memory to simulate what Neo4j would do:
/// 1. Extract all nodes from all files
/// 2. Build definition lookup by qualified_name
/// 3. For each file, try to resolve references through imports
pub fn evaluate_cross_file_resolution_with_config(
    codebase_path: &Path,
    mut executor: TsgExecutor,
    config: &CrossFileEvalConfig,
) -> Result<CrossFileEvalStats> {
    let mut stats = CrossFileEvalStats::default();

    // Phase 1: Extract all nodes from all files
    let mut all_nodes: Vec<ResolutionNode> = Vec::new();
    let mut nodes_by_file: HashMap<String, Vec<ResolutionNode>> = HashMap::new();

    for entry in WalkDir::new(codebase_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            // Check extension
            let has_ext = path.extension().is_some_and(|ext| ext == config.extension);
            // Check skip dirs
            let in_skip_dir = config.skip_dirs.iter().any(|skip| {
                path.components()
                    .any(|c| c.as_os_str().to_string_lossy().contains(skip))
            });
            has_ext && !in_skip_dir
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

        // Classify imports as internal or external
        // Internal: relative paths (./foo, ../foo) or can be resolved to definitions
        // External: npm packages, node:* builtins, @scope/packages
        let mut internal_imports: HashSet<&str> = HashSet::new();
        let mut external_imports: HashSet<&str> = HashSet::new();

        for node in nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Import)
        {
            if let Some(import_path) = &node.import_path {
                let clean_path = import_path.trim_matches(|c| c == '"' || c == '\'');

                // Check if this is an internal import
                let is_relative = clean_path.starts_with('.') || clean_path.starts_with('/');
                let resolves_to_definition = definitions_by_qname
                    .contains_key(import_path.as_str())
                    || definitions_by_qname.contains_key(clean_path)
                    || definitions_by_name.contains_key(node.name.as_str());

                if is_relative || resolves_to_definition {
                    internal_imports.insert(node.name.as_str());
                    stats.total_imports += 1;
                    stats.imports_resolved_cross_file += 1;
                } else {
                    // External import - don't count in totals
                    external_imports.insert(node.name.as_str());
                }
            }
        }

        // Count references - only those to internal definitions/imports
        for node in nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Reference)
        {
            // Skip builtins
            if (config.is_builtin)(&node.name) {
                continue;
            }

            // Skip references to external imports - these are outside our codebase
            if external_imports.contains(node.name.as_str()) {
                continue;
            }

            stats.total_references += 1;

            // Try to resolve: first local definitions, then internal imports, then global definitions
            // Global definitions handle cases like `module.function()` where function is defined
            // in another file but accessed through a module import
            if local_definitions.contains(node.name.as_str()) {
                stats.resolved_via_local_definition += 1;
            } else if internal_imports.contains(node.name.as_str()) {
                stats.resolved_via_import += 1;
            } else if definitions_by_name.contains_key(node.name.as_str()) {
                // Reference matches a definition somewhere in the codebase
                // (e.g., dotenv.load_dotenv where load_dotenv is defined in another file)
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

        // Resolution rate = (local + import) / total = (30 + 50) / 100 = 80%
        assert!((stats.resolution_rate() - 0.80).abs() < 0.001);
    }
}
