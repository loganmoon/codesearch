//! Test logging utilities

use std::sync::Once;

static INIT_LOGGING: Once = Once::new();

/// Initialize test logging based on environment variables
///
/// Checks the following environment variables in order:
/// 1. CODESEARCH_TEST_LOG - Specific to tests
/// 2. RUST_LOG - General Rust logging
/// 3. Default: "error" level
///
/// This function is safe to call multiple times; logging will only be
/// initialized once per test run.
pub fn init_test_logging() {
    INIT_LOGGING.call_once(|| {
        let log_level = std::env::var("CODESEARCH_TEST_LOG")
            .or_else(|_| std::env::var("RUST_LOG"))
            .unwrap_or_else(|_| "error".to_string());

        tracing_subscriber::fmt()
            .with_env_filter(log_level)
            .with_test_writer()
            .try_init()
            .ok(); // Ignore error if already initialized
    });
}

/// Run a test function with verbose logging temporarily enabled
///
/// This is useful for debugging specific tests without affecting the entire suite.
pub fn with_verbose_logging<F, T>(test_fn: F) -> T
where
    F: FnOnce() -> T,
{
    // Save current log level
    let original = std::env::var("RUST_LOG").ok();

    // Set verbose logging
    std::env::set_var("RUST_LOG", "debug");

    // Run test
    let result = test_fn();

    // Restore original log level
    match original {
        Some(level) => std::env::set_var("RUST_LOG", level),
        None => std::env::remove_var("RUST_LOG"),
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_logging_is_idempotent() {
        init_test_logging();
        init_test_logging();
        init_test_logging();
        // Should not panic or error
    }

    #[test]
    fn test_with_verbose_logging_restores_env() {
        let original = std::env::var("RUST_LOG").ok();

        with_verbose_logging(|| {
            // Inside the closure, RUST_LOG should be "debug"
            assert_eq!(std::env::var("RUST_LOG").unwrap(), "debug");
        });

        // After the closure, it should be restored
        assert_eq!(std::env::var("RUST_LOG").ok(), original);
    }
}
