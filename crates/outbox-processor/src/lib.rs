#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

pub mod config;
pub mod processor;

// Re-export for ease of use
pub use processor::OutboxProcessor;
