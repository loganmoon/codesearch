//! Rust entity extraction handlers
//!
//! This module exports all handlers for extracting various Rust
//! language constructs from tree-sitter query matches.

pub(crate) mod common;
pub(crate) mod constant_handlers;
pub(crate) mod constants;
pub(crate) mod function_handlers;
pub(crate) mod impl_handlers;
pub(crate) mod macro_handlers;
pub(crate) mod module_handlers;
pub(crate) mod type_alias_handlers;
pub(crate) mod type_handlers;

#[cfg(test)]
mod tests;

pub(crate) use constant_handlers::handle_constant;
pub(crate) use function_handlers::handle_function;
pub(crate) use impl_handlers::{handle_impl, handle_impl_trait};
pub(crate) use macro_handlers::handle_macro;
pub(crate) use module_handlers::handle_module;
pub(crate) use type_alias_handlers::handle_type_alias;
pub(crate) use type_handlers::{handle_enum, handle_struct, handle_trait};
