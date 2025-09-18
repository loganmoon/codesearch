//! Error types for the entity builder

use std::error::Error;
use std::fmt;

/// Errors that can occur during entity building
#[derive(Debug)]
#[allow(dead_code)]
pub enum BuilderError {
    /// Missing required field
    MissingRequiredField(String),

    /// Invalid field value
    InvalidFieldValue { field: String, reason: String },

    /// Core builder error
    CoreBuilderError(String),

    /// Validation error
    ValidationError(String),
}

impl fmt::Display for BuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuilderError::MissingRequiredField(field) => {
                write!(f, "Missing required field: {field}")
            }
            BuilderError::InvalidFieldValue { field, reason } => {
                write!(f, "Invalid value for field '{field}': {reason}")
            }
            BuilderError::CoreBuilderError(msg) => {
                write!(f, "Core builder error: {msg}")
            }
            BuilderError::ValidationError(msg) => {
                write!(f, "Validation error: {msg}")
            }
        }
    }
}

impl Error for BuilderError {}

impl From<String> for BuilderError {
    fn from(msg: String) -> Self {
        BuilderError::CoreBuilderError(msg)
    }
}

impl From<&str> for BuilderError {
    fn from(msg: &str) -> Self {
        BuilderError::CoreBuilderError(msg.to_string())
    }
}
