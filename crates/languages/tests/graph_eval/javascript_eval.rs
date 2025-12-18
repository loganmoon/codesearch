//! Integration test for JavaScript TSG extraction evaluation on the nanoid codebase
//!
//! Run with: cargo test -p codesearch-languages --test graph_eval javascript -- --ignored --nocapture

use crate::common;
use codesearch_languages::tsg::{
    evaluate_cross_file_resolution_with_config, CrossFileEvalConfig, TsgExecutor,
};
use tempfile::TempDir;

const REPO_URL: &str = "https://github.com/ai/nanoid";
const TARGET_RATE: f64 = 0.80;

#[test]
#[ignore] // Requires network access
fn test_evaluate_nanoid_codebase() {
    let temp_dir =
        TempDir::new().unwrap_or_else(|e| panic!("Failed to create temp directory: {e}"));
    let repo_path = temp_dir.path().join("nanoid");

    let cloned_path = common::clone_repo(REPO_URL, &repo_path)
        .unwrap_or_else(|e| panic!("Failed to clone nanoid repository: {e}"));

    let executor = TsgExecutor::new_javascript()
        .unwrap_or_else(|e| panic!("Failed to create JavaScript executor: {e}"));

    let config = CrossFileEvalConfig {
        extension: "js",
        skip_dirs: &["node_modules", "dist", "build", ".git", "coverage"],
    };

    let stats = evaluate_cross_file_resolution_with_config(&cloned_path, executor, &config)
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
