//! Tree-sitter query patterns for JavaScript and TypeScript
//!
//! This module contains the tree-sitter query strings used to extract
//! entities from JavaScript and TypeScript source code.

pub mod classes;
pub mod functions;
pub mod modules;
pub mod typescript;
pub mod variables;

pub use classes::*;
pub use functions::*;
pub use modules::*;
pub use typescript::*;
pub use variables::*;
