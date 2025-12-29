//! Fixture definitions for spec validation tests
//!
//! Fixtures are organized by category:
//! - modules: Module system (declarations, visibility, imports, nesting)
//! - functions: Functions, methods, and call relationships
//! - types: Structs, enums, type aliases
//! - traits: Trait definitions, implementations, associated types
//! - constants_macros: Constants and macro definitions
//! - cross_module: Complex cross-module resolution scenarios
//! - edge_cases: Edge cases (UFCS, const generics, closures, etc.)
//! - workspace: Multi-crate workspace scenarios

use codesearch_e2e_tests::common::spec_validation::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
};

pub mod constants_macros;
pub mod cross_module;
pub mod edge_cases;
pub mod functions;
pub mod modules;
pub mod traits;
pub mod types;
pub mod workspace;

pub use constants_macros::*;
pub use cross_module::*;
pub use edge_cases::*;
pub use functions::*;
pub use modules::*;
pub use traits::*;
pub use types::*;
pub use workspace::*;
