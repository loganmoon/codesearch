//! TypeScript handler implementations

mod function_handlers;
mod type_handlers;

pub use function_handlers::{handle_arrow_function_impl, handle_function_impl};
pub use type_handlers::{
    handle_class_impl, handle_enum_impl, handle_interface_impl, handle_method_impl,
    handle_type_alias_impl,
};
