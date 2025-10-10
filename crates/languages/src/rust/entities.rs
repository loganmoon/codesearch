//! Rust-specific entity types
//!
//! This module provides structured types for representing Rust entity metadata.

use codesearch_core::entities::Visibility;
use serde::{Deserialize, Serialize};

/// Information about a struct or enum field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldInfo {
    pub name: String,
    pub field_type: String,
    pub visibility: Visibility,
    pub attributes: Vec<String>,
}

/// Information about an enum variant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantInfo {
    pub name: String,
    pub fields: Vec<FieldInfo>,
    pub discriminant: Option<String>,
}
