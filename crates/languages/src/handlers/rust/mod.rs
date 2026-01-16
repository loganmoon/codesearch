//! Rust entity extraction handlers
//!
//! This module contains all handler functions for extracting Rust code entities.
//! Each handler is registered via `#[entity_handler]` and invoked by the handler engine.

pub(crate) mod building_blocks;
pub(crate) mod functions;
pub(crate) mod impl_blocks;
pub(crate) mod methods;
pub(crate) mod types;
