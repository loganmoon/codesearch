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

pub use constant_handlers::handle_constant_impl;
pub use function_handlers::handle_function_impl;
pub use impl_handlers::{handle_impl_impl, handle_impl_trait_impl};
pub use macro_handlers::handle_macro_impl;
pub use module_handlers::handle_module_impl;
pub use type_alias_handlers::handle_type_alias_impl;
pub use type_handlers::{handle_enum_impl, handle_struct_impl, handle_trait_impl};
