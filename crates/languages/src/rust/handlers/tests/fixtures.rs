//! Large test fixtures for comprehensive extraction testing

use super::*;
use crate::rust::entities::RustEntityVariant;
use crate::rust::handlers::function_handlers::handle_function;
use crate::rust::handlers::type_handlers::{handle_enum, handle_struct, handle_trait};
use crate::transport::EntityVariant;

/// Large comprehensive Rust code sample (100+ lines)
const LARGE_RUST_SAMPLE: &str = r####"
//! A comprehensive Rust module for testing entity extraction
//!
//! This module contains various Rust constructs to test the extraction handlers.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::fmt::{self, Display, Debug};
use std::io::{self, Read, Write};

/// Configuration options for the application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Server hostname
    pub hostname: String,
    /// Server port
    pub port: u16,
    /// Enable debug mode
    pub debug: bool,
    /// Connection timeout in seconds
    pub timeout: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hostname: "localhost".to_string(),
            port: 8080,
            debug: false,
            timeout: 30,
        }
    }
}

/// Represents different types of messages
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Text message with content
    Text(String),
    /// Binary data
    Binary(Vec<u8>),
    /// Control message
    Control {
        command: String,
        args: Vec<String>
    },
    /// Error with code and description
    Error {
        code: u32,
        description: String
    },
}

/// A trait for entities that can be processed
pub trait Processable {
    /// Process the entity and return a result
    fn process(&self) -> Result<String, ProcessError>;

    /// Validate the entity
    fn validate(&self) -> bool {
        true
    }

    /// Get the entity's identifier
    fn identifier(&self) -> &str;
}

/// A trait with generic parameters and associated types
pub trait Container<T>
where
    T: Clone + Debug,
{
    type Item;
    type Error: std::error::Error;

    /// Add an item to the container
    fn add(&mut self, item: T) -> Result<(), Self::Error>;

    /// Remove an item from the container
    fn remove(&mut self, item: &T) -> Result<(), Self::Error>;

    /// Get the size of the container
    fn size(&self) -> usize;

    /// Clear all items
    fn clear(&mut self) {
        // Default implementation
    }
}

/// A generic function with multiple type parameters and constraints
pub fn process_items<'a, T, U, F>(
    items: &'a [T],
    transformer: F,
    options: &Config,
) -> Vec<U>
where
    T: Clone + Debug + Send + 'a,
    U: From<T> + Default,
    F: Fn(&T) -> U + Send + Sync,
{
    items.iter()
        .map(|item| transformer(item))
        .collect()
}

/// An async function for network operations
pub async fn fetch_data(url: &str, config: &Config) -> io::Result<Vec<u8>> {
    // Simulated async operation
    if config.debug {
        println!("Fetching from: {}", url);
    }

    Ok(vec![1, 2, 3, 4, 5])
}

/// A const function for compile-time computation
pub const fn calculate_buffer_size(base: usize, multiplier: usize) -> usize {
    base * multiplier
}

/// An unsafe function for low-level operations
pub unsafe fn raw_memory_copy(src: *const u8, dst: *mut u8, len: usize) {
    std::ptr::copy_nonoverlapping(src, dst, len);
}

/// A struct with lifetime parameters
pub struct Reference<'a, T>
where
    T: Display + 'a,
{
    data: &'a T,
    name: String,
}

impl<'a, T> Reference<'a, T>
where
    T: Display + 'a,
{
    /// Create a new reference
    pub fn new(data: &'a T, name: String) -> Self {
        Self { data, name }
    }

    /// Get the referenced data
    pub fn get(&self) -> &T {
        self.data
    }
}

/// A tuple struct
pub struct Color(pub u8, pub u8, pub u8, pub u8);

/// A unit struct
pub struct Marker;

/// Error type for processing operations
#[derive(Debug)]
pub enum ProcessError {
    InvalidInput(String),
    Timeout { duration: u64 },
    IoError(io::Error),
    Unknown,
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            ProcessError::Timeout { duration } => write!(f, "Timeout after {} seconds", duration),
            ProcessError::IoError(err) => write!(f, "IO error: {}", err),
            ProcessError::Unknown => write!(f, "Unknown error"),
        }
    }
}

impl std::error::Error for ProcessError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn test_message_variants() {
        let msg = Message::Text("Hello".to_string());
        assert!(matches!(msg, Message::Text(_)));
    }
}
"####;

#[test]
fn test_large_file_extraction() {
    // Test function extraction
    let function_entities =
        extract_with_handler(LARGE_RUST_SAMPLE, queries::FUNCTION_QUERY, handle_function)
            .expect("Failed to extract functions from large sample");

    // Should find multiple functions
    assert!(function_entities.len() >= 5);

    // Verify some specific functions
    let function_names: Vec<&str> = function_entities.iter().map(|e| e.name.as_str()).collect();

    assert!(function_names.contains(&"process_items"));
    assert!(function_names.contains(&"fetch_data"));
    assert!(function_names.contains(&"calculate_buffer_size"));
    assert!(function_names.contains(&"raw_memory_copy"));

    // Check async function
    let async_func = function_entities
        .iter()
        .find(|e| e.name == "fetch_data")
        .expect("Should find fetch_data function");

    if let EntityVariant::Rust(RustEntityVariant::Function { is_async, .. }) = &async_func.variant {
        assert!(is_async);
    }

    // Check unsafe function
    let unsafe_func = function_entities
        .iter()
        .find(|e| e.name == "raw_memory_copy")
        .expect("Should find raw_memory_copy function");

    if let EntityVariant::Rust(RustEntityVariant::Function { is_unsafe, .. }) = &unsafe_func.variant
    {
        assert!(is_unsafe);
    }

    // Check const function
    let const_func = function_entities
        .iter()
        .find(|e| e.name == "calculate_buffer_size")
        .expect("Should find calculate_buffer_size function");

    if let EntityVariant::Rust(RustEntityVariant::Function { is_const, .. }) = &const_func.variant {
        assert!(is_const);
    }
}

#[test]
fn test_large_file_struct_extraction() {
    let struct_entities =
        extract_with_handler(LARGE_RUST_SAMPLE, queries::STRUCT_QUERY, handle_struct)
            .expect("Failed to extract structs from large sample");

    // Should find multiple structs
    assert!(struct_entities.len() >= 4);

    let struct_names: Vec<&str> = struct_entities.iter().map(|e| e.name.as_str()).collect();

    assert!(struct_names.contains(&"Config"));
    assert!(struct_names.contains(&"Reference"));
    assert!(struct_names.contains(&"Color"));
    assert!(struct_names.contains(&"Marker"));

    // Check Config struct has fields
    let config_struct = struct_entities
        .iter()
        .find(|e| e.name == "Config")
        .expect("Should find Config struct");

    if let EntityVariant::Rust(RustEntityVariant::Struct { fields, .. }) = &config_struct.variant {
        assert_eq!(fields.len(), 4);
        assert!(fields.iter().any(|f| f.name == "hostname"));
        assert!(fields.iter().any(|f| f.name == "port"));
        assert!(fields.iter().any(|f| f.name == "debug"));
        assert!(fields.iter().any(|f| f.name == "timeout"));
    }

    // Check Color is a tuple struct
    let color_struct = struct_entities
        .iter()
        .find(|e| e.name == "Color")
        .expect("Should find Color struct");

    if let EntityVariant::Rust(RustEntityVariant::Struct {
        is_tuple, fields, ..
    }) = &color_struct.variant
    {
        assert!(is_tuple);
        assert_eq!(fields.len(), 4);
    }

    // Check Marker is a unit struct
    let marker_struct = struct_entities
        .iter()
        .find(|e| e.name == "Marker")
        .expect("Should find Marker struct");

    if let EntityVariant::Rust(RustEntityVariant::Struct { fields, .. }) = &marker_struct.variant {
        assert_eq!(fields.len(), 0);
    }
}

#[test]
fn test_large_file_enum_extraction() {
    let enum_entities = extract_with_handler(LARGE_RUST_SAMPLE, queries::ENUM_QUERY, handle_enum)
        .expect("Failed to extract enums from large sample");

    // Should find Message and ProcessError enums
    assert!(enum_entities.len() >= 2);

    let enum_names: Vec<&str> = enum_entities.iter().map(|e| e.name.as_str()).collect();

    assert!(enum_names.contains(&"Message"));
    assert!(enum_names.contains(&"ProcessError"));

    // Check Message enum variants
    let message_enum = enum_entities
        .iter()
        .find(|e| e.name == "Message")
        .expect("Should find Message enum");

    if let EntityVariant::Rust(RustEntityVariant::Enum { variants, .. }) = &message_enum.variant {
        assert_eq!(variants.len(), 4);
        assert!(variants.iter().any(|v| v.name == "Text"));
        assert!(variants.iter().any(|v| v.name == "Binary"));
        assert!(variants.iter().any(|v| v.name == "Control"));
        assert!(variants.iter().any(|v| v.name == "Error"));
    }
}

#[test]
fn test_large_file_trait_extraction() {
    let trait_entities =
        extract_with_handler(LARGE_RUST_SAMPLE, queries::TRAIT_QUERY, handle_trait)
            .expect("Failed to extract traits from large sample");

    // Should find Processable and Container traits
    assert!(trait_entities.len() >= 2);

    let trait_names: Vec<&str> = trait_entities.iter().map(|e| e.name.as_str()).collect();

    assert!(trait_names.contains(&"Processable"));
    assert!(trait_names.contains(&"Container"));

    // Check Processable trait methods
    let processable_trait = trait_entities
        .iter()
        .find(|e| e.name == "Processable")
        .expect("Should find Processable trait");

    if let EntityVariant::Rust(RustEntityVariant::Trait { methods, .. }) =
        &processable_trait.variant
    {
        assert!(methods.len() >= 3);
        assert!(methods.contains(&"process".to_string()));
        assert!(methods.contains(&"validate".to_string()));
        assert!(methods.contains(&"identifier".to_string()));
    }

    // Check Container trait has associated types
    let container_trait = trait_entities
        .iter()
        .find(|e| e.name == "Container")
        .expect("Should find Container trait");

    if let EntityVariant::Rust(RustEntityVariant::Trait {
        associated_types,
        methods,
        generics,
        ..
    }) = &container_trait.variant
    {
        assert_eq!(associated_types.len(), 2);
        assert!(associated_types.contains(&"Item".to_string()));
        assert!(associated_types.contains(&"Error".to_string()));
        assert_eq!(generics.len(), 1);
        assert!(methods.len() >= 4);
    }
}

#[test]
fn test_documentation_extraction() {
    let struct_entities =
        extract_with_handler(LARGE_RUST_SAMPLE, queries::STRUCT_QUERY, handle_struct)
            .expect("Failed to extract structs");

    // Config struct should have documentation
    let config_struct = struct_entities
        .iter()
        .find(|e| e.name == "Config")
        .expect("Should find Config struct");

    assert!(config_struct.documentation.is_some());
    let doc = config_struct.documentation.as_ref().unwrap();
    assert!(doc.contains("Configuration options"));
}

#[test]
fn test_visibility_extraction() {
    let function_entities =
        extract_with_handler(LARGE_RUST_SAMPLE, queries::FUNCTION_QUERY, handle_function)
            .expect("Failed to extract functions");

    // Most functions should be public
    let public_count = function_entities
        .iter()
        .filter(|e| e.visibility == codesearch_core::entities::Visibility::Public)
        .count();

    assert!(public_count > 0);
}

#[test]
fn test_extremely_large_extraction_performance() {
    use std::time::Instant;

    let start = Instant::now();

    // Run all extractors on the large sample
    let _functions =
        extract_with_handler(LARGE_RUST_SAMPLE, queries::FUNCTION_QUERY, handle_function);

    let _structs = extract_with_handler(LARGE_RUST_SAMPLE, queries::STRUCT_QUERY, handle_struct);

    let _enums = extract_with_handler(LARGE_RUST_SAMPLE, queries::ENUM_QUERY, handle_enum);

    let _traits = extract_with_handler(LARGE_RUST_SAMPLE, queries::TRAIT_QUERY, handle_trait);

    let duration = start.elapsed();

    // Extraction should be fast, even for large files
    // Adjust threshold as needed based on performance requirements
    assert!(duration.as_millis() < 1000);
}
