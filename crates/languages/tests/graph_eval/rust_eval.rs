//! Integration test for Rust TSG extraction evaluation on the codesearch codebase
//!
//! Run with: cargo test -p codesearch-languages --test graph_eval rust -- --ignored --nocapture

use codesearch_languages::tsg::{
    evaluate_cross_file_resolution_with_config, CrossFileEvalConfig, TsgExecutor,
};
use std::path::Path;

// Target rate for Rust. Remaining unresolved are mostly:
// - Test helpers used across test files without imports
// - Method calls requiring type-aware resolution
const TARGET_RATE: f64 = 0.80;

#[test]
#[ignore] // Slow test
fn test_evaluate_codesearch_codebase() {
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| panic!("Failed to get parent directory of CARGO_MANIFEST_DIR"));

    println!("\nEvaluating TSG extraction on codesearch crates...\n");

    let executor =
        TsgExecutor::new_rust().unwrap_or_else(|e| panic!("Failed to create Rust executor: {e}"));

    let config = CrossFileEvalConfig {
        extension: "rs",
        skip_dirs: &["target", ".git"],
    };

    let stats = evaluate_cross_file_resolution_with_config(crates_dir, executor, &config)
        .unwrap_or_else(|e| panic!("Failed to evaluate cross-file resolution: {e}"));

    stats.print_summary();

    assert!(stats.total_files > 0, "Should process at least one file");
    assert!(
        stats.total_definitions > 0,
        "Should extract at least one definition"
    );

    let rate = stats.resolution_rate();
    assert!(
        rate >= TARGET_RATE,
        "Resolution rate {:.1}% is below target {:.0}%",
        rate * 100.0,
        TARGET_RATE * 100.0
    );

    if rate >= TARGET_RATE {
        println!(
            "\nSUCCESS: Achieved {:.1}% resolution rate (target: {:.0}%)",
            rate * 100.0,
            TARGET_RATE * 100.0
        );
    }
}

#[test]
fn test_sample_rust_file() {
    use codesearch_languages::tsg::{build_intra_file_edges, ResolutionNodeKind};

    let mut executor =
        TsgExecutor::new_rust().unwrap_or_else(|e| panic!("Failed to create Rust executor: {e}"));

    let source = r#"
use std::collections::HashMap;
use anyhow::Result;

pub struct MyStruct {
    data: HashMap<String, i32>,
}

impl MyStruct {
    pub fn new() -> Result<Self> {
        Ok(Self {
            data: HashMap::new(),
        })
    }

    pub fn process(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }
}

fn helper() -> Option<MyStruct> {
    MyStruct::new().ok()
}
"#;

    let nodes = executor
        .extract(source, Path::new("sample.rs"))
        .unwrap_or_else(|e| panic!("Failed to extract nodes: {e}"));

    println!("\n=== Sample File Analysis ===\n");

    println!("Definitions:");
    for node in nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Definition)
    {
        println!(
            "  {} ({})",
            node.name,
            node.definition_kind.as_deref().unwrap_or("?")
        );
    }

    println!("\nImports:");
    for node in nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Import)
    {
        println!(
            "  {} <- {}",
            node.name,
            node.import_path.as_deref().unwrap_or("?")
        );
    }

    println!("\nReferences:");
    for node in nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Reference)
    {
        println!(
            "  {} (context: {})",
            node.name,
            node.reference_context.as_deref().unwrap_or("?")
        );
    }

    let (resolved, unresolved) = build_intra_file_edges(&nodes);
    println!(
        "\nResolution: {} resolved, {} unresolved",
        resolved,
        unresolved.len()
    );

    if !unresolved.is_empty() {
        println!("\nUnresolved references:");
        for node in &unresolved {
            println!(
                "  {} (line {}, context: {})",
                node.name,
                node.start_line,
                node.reference_context.as_deref().unwrap_or("?")
            );
        }
    }
}
