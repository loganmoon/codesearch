//! Integration test for JavaScript TSG extraction evaluation on the nanoid codebase
//!
//! Run with: cargo test -p codesearch-languages --test javascript_tsg_eval_test -- --ignored --nocapture
//!
//! This test uses cross-file resolution to evaluate how well references can be
//! resolved through imports to definitions in other files.

mod common;

use codesearch_languages::tsg::{
    evaluate_cross_file_resolution_with_config, is_javascript_builtin, CrossFileEvalConfig,
    TsgExecutor,
};
use tempfile::TempDir;

const REPO_URL: &str = "https://github.com/ai/nanoid";
const TARGET_RATE: f64 = 0.80;

#[test]
#[ignore] // Requires network access
fn test_evaluate_nanoid_codebase() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let repo_path = temp_dir.path().join("nanoid");

    let cloned_path =
        common::clone_repo(REPO_URL, &repo_path).expect("Failed to clone nanoid repository");

    let executor = TsgExecutor::new_javascript().expect("Failed to create JavaScript executor");

    let config = CrossFileEvalConfig {
        extension: "js",
        skip_dirs: &["node_modules", "dist", "build", ".git", "coverage"],
        is_builtin: is_javascript_builtin,
    };

    let stats = evaluate_cross_file_resolution_with_config(&cloned_path, executor, &config)
        .expect("Failed to evaluate cross-file resolution");

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
