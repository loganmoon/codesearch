//! Tree-sitter query patterns for JavaScript and TypeScript
//!
//! This module contains the tree-sitter query strings used to extract
//! entities from JavaScript and TypeScript source code.

pub(crate) mod classes;
pub(crate) mod functions;
pub(crate) mod modules;
pub(crate) mod typescript;
pub(crate) mod variables;

pub(crate) use classes::*;
pub(crate) use functions::*;
pub(crate) use modules::*;
pub(crate) use typescript::*;
pub(crate) use variables::*;
