//! End-to-end test infrastructure
//!
//! This module provides utilities for E2E testing of the complete codesearch pipeline.

#![allow(dead_code)]
#![allow(unused_imports)]

pub mod assertions;
pub mod cleanup;
pub mod containers;
pub mod fixtures;
pub mod logging;

use anyhow::{Context, Result};
use std::future::Future;
use std::time::Duration;

// Re-export key types and utilities
pub use assertions::*;
pub use containers::*;
pub use fixtures::*;
pub use logging::*;

/// Wrap a test future with a timeout
///
/// Prevents tests from hanging indefinitely by adding a timeout.
/// Returns an error if the future doesn't complete within the specified duration.
pub async fn with_timeout<F, T>(duration: Duration, future: F) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    tokio::time::timeout(duration, future)
        .await
        .context(format!("Test timed out after {duration:?}"))?
}
