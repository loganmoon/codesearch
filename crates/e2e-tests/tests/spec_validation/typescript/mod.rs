//! TypeScript spec validation tests
//!
//! Contains fixtures and test functions for validating TypeScript code extraction
//! against the specification in crates/languages/specs/typescript.yaml
//!
//! Fixture categories:
//! - modules: ES modules, namespaces, imports, exports
//! - classes: Classes, inheritance, implements
//! - interfaces: Interfaces, declaration merging
//! - functions: Functions, methods, arrow functions
//! - types: Type aliases, enums
//! - advanced: Decorators, generics, ambient declarations, JSX

pub mod fixtures;

pub use fixtures::*;
