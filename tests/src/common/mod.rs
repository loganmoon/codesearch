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

// Re-export key types and utilities
pub use assertions::*;
pub use containers::*;
pub use fixtures::*;
pub use logging::*;
