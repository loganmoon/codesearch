//! Entity extraction handlers
//!
//! This module provides handler functions for extracting code entities from AST matches.
//! Handlers are registered via the `#[entity_handler]` proc macro and invoked by the
//! handler engine.

pub(crate) mod rust;
