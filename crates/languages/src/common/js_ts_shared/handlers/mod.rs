//! Entity handlers for JavaScript and TypeScript
//!
//! This module contains handler implementations for extracting code entities
//! from JavaScript and TypeScript source code.
//!
//! # Handler Architecture
//!
//! Handlers use the [`define_handler!`] macro with language-specific extractors
//! ([`JavaScript`], [`TypeScript`]) from the [`LanguageExtractors`] trait.
//!
//! **Important:** The first type parameter to `define_handler!` determines the
//! `Language` enum value in extracted entities. For shared entity types (functions,
//! classes, methods, etc.), separate handlers exist for each language:
//!
//! - `handle_function_declaration_impl` → `Language::JavaScript`
//! - `handle_ts_function_declaration_impl` → `Language::TypeScript`
//!
//! Always use the correct handler for your language to ensure proper labeling.
//!
//! [`LanguageExtractors`]: crate::common::language_extractors::LanguageExtractors
//! [`JavaScript`]: super::JavaScript
//! [`TypeScript`]: super::TypeScript

pub(crate) mod class_handlers;
pub(crate) mod common;
pub(crate) mod function_handlers;
pub(crate) mod method_handlers;
pub(crate) mod property_handlers;
pub(crate) mod typescript_handlers;
pub(crate) mod variable_handlers;

#[cfg(test)]
mod tests;

pub(crate) use class_handlers::*;
pub(crate) use function_handlers::*;
pub(crate) use method_handlers::*;
pub(crate) use property_handlers::*;
pub(crate) use typescript_handlers::*;
pub(crate) use variable_handlers::*;
