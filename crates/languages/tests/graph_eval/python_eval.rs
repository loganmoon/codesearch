//! Integration test for Python TSG extraction evaluation on the python-dotenv codebase
//!
//! Run with: cargo test -p codesearch-languages --test graph_eval python -- --ignored --nocapture

use crate::common;
use codesearch_languages::tsg::{
    evaluate_cross_file_resolution_with_config, CrossFileEvalConfig, TsgExecutor,
};
use tempfile::TempDir;

const REPO_URL: &str = "https://github.com/theskumar/python-dotenv";
const TARGET_RATE: f64 = 0.80;

#[test]
#[ignore] // Requires network access
fn test_evaluate_python_dotenv_codebase() {
    let temp_dir =
        TempDir::new().unwrap_or_else(|e| panic!("Failed to create temp directory: {e}"));
    let repo_path = temp_dir.path().join("python-dotenv");

    let cloned_path = common::clone_repo(REPO_URL, &repo_path)
        .unwrap_or_else(|e| panic!("Failed to clone python-dotenv repository: {e}"));

    let executor = TsgExecutor::new_python()
        .unwrap_or_else(|e| panic!("Failed to create Python executor: {e}"));

    let config = CrossFileEvalConfig {
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
