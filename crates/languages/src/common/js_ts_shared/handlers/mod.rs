//! Entity handlers for JavaScript and TypeScript
//!
//! This module contains handler implementations for extracting code entities
//! from JavaScript and TypeScript source code.

pub mod class_handlers;
pub mod common;
pub mod function_handlers;
pub mod method_handlers;
pub mod property_handlers;
pub mod typescript_handlers;
pub mod variable_handlers;

pub use class_handlers::*;
pub use common::*;
pub use function_handlers::*;
pub use method_handlers::*;
pub use property_handlers::*;
pub use typescript_handlers::*;
pub use variable_handlers::*;
