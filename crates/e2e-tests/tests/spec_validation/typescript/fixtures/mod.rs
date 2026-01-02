//! TypeScript fixture definitions for spec validation tests
//!
//! Fixtures are organized by category matching typescript.yaml spec rules:
//! - modules: E-MOD-*, V-EXPORT-*, Q-MODULE-*, R-IMPORTS-*, R-REEXPORTS-*
//! - classes: E-CLASS-*, E-METHOD-*, E-PROPERTY-*, V-CLASS-*, R-INHERITS-FROM, R-IMPLEMENTS
//! - interfaces: E-INTERFACE-*, E-INDEX-*, E-CALL-*, E-CONSTRUCT-*, R-EXTENDS-INTERFACE
//! - functions: E-FN-*, M-FN-*, R-CALLS-*
//! - types: E-TYPE-ALIAS-*, E-ENUM-*, E-CONST, E-VAR-*
//! - advanced: M-DECORATOR-*, M-GENERIC-*, E-AMBIENT-*, E-JSX-*

use codesearch_core::entities::Visibility;
use codesearch_e2e_tests::common::spec_validation::{
    EntityKind, ExpectedEntity, ExpectedRelationship, Fixture, ProjectType, RelationshipKind,
};

pub mod advanced;
pub mod classes;
pub mod functions;
pub mod interfaces;
pub mod modules;
pub mod types;

pub use advanced::*;
pub use classes::*;
pub use functions::*;
pub use interfaces::*;
pub use modules::*;
pub use types::*;
