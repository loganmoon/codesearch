//! Handler implementations for JavaScript entities

pub(crate) mod class_handlers;
pub(crate) mod function_handlers;

pub use class_handlers::{handle_class_impl, handle_method_impl};
pub use function_handlers::{handle_arrow_function_impl, handle_function_impl};
