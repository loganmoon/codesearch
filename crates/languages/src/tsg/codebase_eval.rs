//! Codebase evaluation utilities
//!
//! This module provides tools to evaluate TSG extraction and resolution
//! on a codebase, measuring resolution rates and categorizing unresolved references.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use super::evaluation::{build_intra_file_edges, categorize_unresolved, EvaluationResult};
use super::executor::TsgExecutor;
use super::graph_types::{ResolutionNode, ResolutionNodeKind};
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use walkdir::WalkDir;

/// Evaluate TSG extraction and intra-file resolution on a codebase
///
/// # Arguments
/// * `codebase_path` - Root path of the codebase to evaluate
/// * `verbose` - Whether to print detailed progress information
///
/// # Returns
/// Result containing evaluation results and all extracted nodes
pub fn evaluate_codebase(
    codebase_path: &Path,
    verbose: bool,
) -> Result<(EvaluationResult, Vec<ResolutionNode>)> {
    let mut executor = TsgExecutor::new_rust()?;
    let mut result = EvaluationResult::new();
    let mut all_nodes = Vec::new();

    // Walk through all .rs files
    for entry in WalkDir::new(codebase_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "rs")
                && !e.path().to_string_lossy().contains("/target/")
        })
    {
        let file_path = entry.path();

        // Read file contents
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                if verbose {
                    eprintln!("Warning: Could not read {}: {e}", file_path.display());
                }
                continue;
            }
        };

        // Extract nodes
        let nodes = match executor.extract(&source, file_path) {
            Ok(n) => n,
            Err(e) => {
                if verbose {
                    eprintln!("Warning: Could not extract {}: {e}", file_path.display());
                }
                continue;
            }
        };

        result.total_files += 1;
        result.total_nodes += nodes.len();

        // Count by kind
        for node in &nodes {
            match node.kind {
                ResolutionNodeKind::Definition => result.definition_count += 1,
                ResolutionNodeKind::Export => result.export_count += 1,
                ResolutionNodeKind::Import => result.import_count += 1,
                ResolutionNodeKind::Reference => result.reference_count += 1,
            }
        }

        // Build intra-file edges and count resolved/unresolved
        let (resolved, unresolved) = build_intra_file_edges(&nodes);
        result.intra_file_resolved += resolved;

        for unresolved_ref in &unresolved {
            result.unresolved += 1;
            let category = categorize_unresolved(unresolved_ref);
            *result
                .unresolved_by_pattern
                .entry(category.to_string())
                .or_insert(0) += 1;
        }

        all_nodes.extend(nodes);

        if verbose && result.total_files.is_multiple_of(50) {
            println!(
                "Processed {} files, {} nodes so far...",
                result.total_files, result.total_nodes
            );
        }
    }

    result.compute_rate();

    Ok((result, all_nodes))
}

/// Print a summary of evaluation results
pub fn print_evaluation_summary(result: &EvaluationResult) {
    println!("\n=== TSG Extraction & Resolution Evaluation ===\n");

    println!("Files processed: {}", result.total_files);
    println!("Total nodes extracted: {}", result.total_nodes);
    println!();

    println!("Node counts by type:");
    println!("  Definitions: {}", result.definition_count);
    println!("  Exports: {}", result.export_count);
    println!("  Imports: {}", result.import_count);
    println!("  References: {}", result.reference_count);
    println!();

    println!("Intra-file resolution:");
    println!("  Resolved: {}", result.intra_file_resolved);
    println!("  Unresolved: {}", result.unresolved);
    println!(
        "  Resolution rate: {:.1}%",
        result.intra_file_resolution_rate * 100.0
    );
    println!();

    if !result.unresolved_by_pattern.is_empty() {
        println!("Unresolved by category:");
        let mut categories: Vec<_> = result.unresolved_by_pattern.iter().collect();
        categories.sort_by(|a, b| b.1.cmp(a.1));
        for (category, count) in categories {
            let pct = (*count as f64 / result.unresolved as f64) * 100.0;
            println!("  {category}: {count} ({pct:.1}%)");
        }
    }
}

/// Group nodes by file path for cross-file analysis
pub fn group_by_file(nodes: &[ResolutionNode]) -> HashMap<&Path, Vec<&ResolutionNode>> {
    let mut groups: HashMap<&Path, Vec<&ResolutionNode>> = HashMap::new();
    for node in nodes {
        groups
            .entry(node.file_path.as_path())
            .or_default()
            .push(node);
    }
    groups
}

/// Build cross-file resolution edges
///
/// This matches Import nodes to Export/Definition nodes in other files
/// based on the import path matching the qualified name.
pub fn build_cross_file_edges(all_nodes: &[ResolutionNode]) -> HashMap<String, String> {
    // Build lookup of exports and definitions by qualified name
    let mut definitions_by_qname: HashMap<&str, &ResolutionNode> = HashMap::new();
    let mut exports_by_qname: HashMap<&str, &ResolutionNode> = HashMap::new();

    for node in all_nodes {
        match node.kind {
            ResolutionNodeKind::Definition => {
                definitions_by_qname.insert(&node.qualified_name, node);
            }
            ResolutionNodeKind::Export => {
                exports_by_qname.insert(&node.qualified_name, node);
            }
            _ => {}
        }
    }

    // Match imports to their targets
    let mut edges: HashMap<String, String> = HashMap::new();

    for node in all_nodes {
        if node.kind == ResolutionNodeKind::Import {
            if let Some(import_path) = &node.import_path {
                // First try to match against definitions
                if let Some(def) = definitions_by_qname.get(import_path.as_str()) {
                    edges.insert(node.node_id(), def.node_id());
                }
                // Then try exports
                else if let Some(exp) = exports_by_qname.get(import_path.as_str()) {
                    edges.insert(node.node_id(), exp.node_id());
                }
            }
        }
    }

    edges
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_evaluate_single_file() {
        let mut executor = TsgExecutor::new_rust().unwrap();
        let source = r#"
use std::io::Read;

pub struct Foo {
    field: i32,
}

impl Foo {
    pub fn new() -> Self {
        Self { field: 0 }
    }
}

fn use_foo() {
    let f = Foo::new();
}
"#;

        let nodes = executor.extract(source, &PathBuf::from("test.rs")).unwrap();

        // Should have Definition for Foo, new, use_foo
        // Should have Import for Read
        // Should have References for i32, Self, Foo, etc.

        let definitions: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Definition)
            .collect();
        let imports: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Import)
            .collect();

        assert!(
            definitions.iter().any(|d| d.name == "Foo"),
            "Should have Foo definition"
        );
        assert!(
            imports.iter().any(|i| i.name == "Read"),
            "Should have Read import"
        );
    }
}
