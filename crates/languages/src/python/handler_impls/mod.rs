//! Python entity extraction handler implementations

mod class_handlers;
mod function_handlers;
mod module_handlers;

#[cfg(test)]
mod tests;

pub(crate) use class_handlers::{handle_class_impl, handle_method_impl};
pub(crate) use function_handlers::handle_function_impl;
pub(crate) use module_handlers::handle_module_impl;
