//! Integration test for TypeScript TSG extraction evaluation on the ms codebase
//!
//! Run with: cargo test -p codesearch-languages --test typescript_tsg_eval_test -- --ignored --nocapture
//!
//! This test uses cross-file resolution to evaluate how well references can be
//! resolved through imports to definitions in other files.

mod common;

use codesearch_languages::tsg::{
    evaluate_cross_file_resolution_with_config, CrossFileEvalConfig, TsgExecutor,
};
use tempfile::TempDir;

const REPO_URL: &str = "https://github.com/vercel/ms";
const TARGET_RATE: f64 = 0.80;

#[test]
#[ignore] // Requires network access
fn test_evaluate_ms_codebase() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let repo_path = temp_dir.path().join("ms");

    let cloned_path =
        common::clone_repo(REPO_URL, &repo_path).expect("Failed to clone ms repository");

    let executor = TsgExecutor::new_typescript().expect("Failed to create TypeScript executor");

    let config = CrossFileEvalConfig {
        extension: "ts",
        skip_dirs: &["node_modules", "dist", "build", ".git", "coverage", "lib"],
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
