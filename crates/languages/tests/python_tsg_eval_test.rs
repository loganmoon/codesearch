//! Evaluation test for Python TSG extraction on the python-dotenv codebase
//!
//! This test clones the python-dotenv repository (BSD licensed) and evaluates
//! our Python TSG extraction against it.
//!
//! Run with: cargo test -p codesearch-languages --test python_tsg_eval_test -- --ignored --nocapture
//!
//! This test uses cross-file resolution to evaluate how well references can be
//! resolved through imports to definitions in other files.

mod common;

use codesearch_languages::tsg::{
    evaluate_cross_file_resolution_with_config, CrossFileEvalConfig, TsgExecutor,
};
use tempfile::TempDir;

const REPO_URL: &str = "https://github.com/theskumar/python-dotenv";
const TARGET_RATE: f64 = 0.80;

#[test]
#[ignore] // Requires network access
fn test_evaluate_python_dotenv_codebase() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let repo_path = temp_dir.path().join("python-dotenv");

    let cloned_path = common::clone_repo(REPO_URL, &repo_path).expect("Failed to clone repository");

    let executor = TsgExecutor::new_python().expect("Failed to create Python TSG executor");

    let config = CrossFileEvalConfig {
        extension: "py",
        skip_dirs: &[
            "__pycache__",
            ".venv",
            "venv",
            ".tox",
            ".git",
            "dist",
            "build",
            ".eggs",
        ],
    };

    let stats = evaluate_cross_file_resolution_with_config(&cloned_path, executor, &config)
        .expect("Failed to evaluate cross-file resolution");

    stats.print_summary();

    assert!(
        stats.total_files > 0,
        "Should process at least one Python file"
    );
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
