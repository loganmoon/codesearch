//! Entity handlers for JavaScript and TypeScript
//!
//! This module contains handler implementations for extracting code entities
//! from JavaScript and TypeScript source code.

pub(crate) mod class_handlers;
pub(crate) mod common;
pub(crate) mod function_handlers;
pub(crate) mod method_handlers;
pub(crate) mod property_handlers;
pub(crate) mod typescript_handlers;
pub(crate) mod variable_handlers;

pub(crate) use class_handlers::*;
pub(crate) use function_handlers::*;
pub(crate) use method_handlers::*;
pub(crate) use property_handlers::*;
pub(crate) use typescript_handlers::*;
pub(crate) use variable_handlers::*;
