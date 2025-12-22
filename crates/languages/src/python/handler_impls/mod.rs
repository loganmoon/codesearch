//! Python entity extraction handler implementations

mod class_handlers;
mod function_handlers;
mod module_handlers;

#[cfg(test)]
mod tests;

pub use class_handlers::{handle_class_impl, handle_method_impl};
pub use function_handlers::handle_function_impl;
pub use module_handlers::handle_module_impl;
