//! Shared helpers for TSG evaluation tests
//!
//! This module provides common utilities for cloning repositories and evaluating
//! TSG extraction on codebases.

#![allow(dead_code)]

use codesearch_languages::tsg::{
    build_intra_file_edges, categorize_unresolved, EvaluationResult, ResolutionNode,
    ResolutionNodeKind, TsgExecutor,
};
use git2::Repository;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Clone a git repository to a temporary directory, or use existing cache
pub fn clone_repo(repo_url: &str, target_dir: &Path) -> Result<PathBuf, git2::Error> {
    if target_dir.exists() {
        println!("Using cached repository at {}", target_dir.display());
        return Ok(target_dir.to_path_buf());
    }

    println!("Cloning {} to {}...", repo_url, target_dir.display());
    Repository::clone(repo_url, target_dir)?;
    println!("Clone complete.");

    Ok(target_dir.to_path_buf())
}

/// Detailed evaluation results including error files and unresolved names
pub struct DetailedEvaluation {
    pub result: EvaluationResult,
    pub error_count: usize,
    pub error_files: Vec<String>,
    pub unresolved_names: HashMap<String, usize>,
    pub all_nodes: Vec<ResolutionNode>,
}

/// Language-specific evaluation configuration
pub struct EvalConfig<'a> {
    pub extension: &'a str,
    pub skip_dirs: &'a [&'a str],
}

/// JavaScript evaluation configuration
pub fn javascript_config() -> EvalConfig<'static> {
    EvalConfig {
        extension: "js",
        skip_dirs: &["node_modules", "dist", "build", ".git", "coverage"],
    }
}

/// TypeScript evaluation configuration
pub fn typescript_config() -> EvalConfig<'static> {
    EvalConfig {
        extension: "ts",
        skip_dirs: &["node_modules", "dist", "build", ".git", "coverage", "lib"],
    }
}

/// Python evaluation configuration
pub fn python_config() -> EvalConfig<'static> {
    EvalConfig {
        extension: "py",
        skip_dirs: &[
            "__pycache__",
            ".venv",
            "venv",
            ".tox",
            ".git",
            "build",
            "dist",
            ".eggs",
            "*.egg-info",
        ],
    }
}

/// Evaluate TSG extraction on a directory with language-specific configuration
pub fn evaluate_directory(
    dir: &Path,
    executor: &mut TsgExecutor,
    config: &EvalConfig,
) -> DetailedEvaluation {
    let mut result = EvaluationResult::new();
    let mut error_count = 0;
    let mut error_files = Vec::new();
    let mut unresolved_names: HashMap<String, usize> = HashMap::new();
    let mut all_nodes = Vec::new();

    for entry in WalkDir::new(dir)
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
            Err(e) => {
                error_count += 1;
                error_files.push(format!("{}: read error: {e}", file_path.display()));
                continue;
            }
        };

        let nodes = match executor.extract(&source, file_path) {
            Ok(n) => n,
            Err(e) => {
                error_count += 1;
                error_files.push(format!("{}: extraction error: {e}", file_path.display()));
                continue;
            }
        };

        result.total_files += 1;
        result.total_nodes += nodes.len();

        for node in &nodes {
            match node.kind {
                ResolutionNodeKind::Definition => result.definition_count += 1,
                ResolutionNodeKind::Export => result.export_count += 1,
                ResolutionNodeKind::Import => result.import_count += 1,
                ResolutionNodeKind::Reference => result.reference_count += 1,
            }
        }

        // build_intra_file_edges now handles filtering internally
        let (resolved, unresolved) = build_intra_file_edges(&nodes);
        result.intra_file_resolved += resolved;

        for unresolved_ref in &unresolved {
            result.unresolved += 1;
            let category = categorize_unresolved(unresolved_ref);
            *result
                .unresolved_by_pattern
                .entry(category.to_string())
                .or_insert(0) += 1;
            *unresolved_names
                .entry(unresolved_ref.name.clone())
                .or_insert(0) += 1;
        }

        all_nodes.extend(nodes);
    }

    result.compute_rate();

    DetailedEvaluation {
        result,
        error_count,
        error_files,
        unresolved_names,
        all_nodes,
    }
}

/// Print evaluation summary
pub fn print_summary(eval: &DetailedEvaluation, language: &str, target_rate: f64) {
    let result = &eval.result;

    println!("\n=== {language} TSG Evaluation ===\n");
    println!("Files processed: {}", result.total_files);
    println!("Parse/extraction errors: {}", eval.error_count);
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
            let pct = if result.unresolved > 0 {
                (*count as f64 / result.unresolved as f64) * 100.0
            } else {
                0.0
            };
            println!("  {category}: {count} ({pct:.1}%)");
        }
    }

    // Show top unresolved names
    if !eval.unresolved_names.is_empty() {
        println!("\nTop 15 unresolved references:");
        let mut names: Vec<_> = eval.unresolved_names.iter().collect();
        names.sort_by(|a, b| b.1.cmp(a.1));
        for (name, count) in names.iter().take(15) {
            println!("  {name}: {count}");
        }
    }

    // Show error files
    if !eval.error_files.is_empty() {
        println!("\nFirst 5 error files:");
        for err in eval.error_files.iter().take(5) {
            println!("  {err}");
        }
    }

    // Print target status
    if result.intra_file_resolution_rate >= target_rate {
        println!(
            "\nSUCCESS: Achieved {:.1}% resolution rate (target: {:.0}%)",
            result.intra_file_resolution_rate * 100.0,
            target_rate * 100.0
        );
    } else {
        println!(
            "\nPROGRESS: {:.1}% resolution rate (target: {:.0}%)",
            result.intra_file_resolution_rate * 100.0,
            target_rate * 100.0
        );
        let need_resolved = (target_rate * result.reference_count as f64) as usize;
        let additional_needed = need_resolved.saturating_sub(result.intra_file_resolved);
        println!("Need to resolve {additional_needed} more references to hit target.");
    }
}
