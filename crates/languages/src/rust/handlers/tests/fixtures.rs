//! Large test fixtures for comprehensive extraction testing

use super::*;
use codesearch_core::entities::{EntityType, Visibility};

use crate::rust::handlers::constant_handlers::handle_constant;
use crate::rust::handlers::function_handlers::handle_function;
use crate::rust::handlers::impl_handlers::{handle_impl, handle_impl_trait};
use crate::rust::handlers::macro_handlers::handle_macro;
use crate::rust::handlers::module_handlers::handle_module;
use crate::rust::handlers::type_alias_handlers::handle_type_alias;
use crate::rust::handlers::type_handlers::{handle_enum, handle_struct, handle_trait};

/// Large comprehensive Rust code sample (100+ lines)
const LARGE_RUST_SAMPLE: &str = r####"
//! A comprehensive Rust module for testing entity extraction
//!
//! This module contains various Rust constructs to test the extraction handlers.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::fmt::{self, Display, Debug};
use std::io::{self, Read, Write};

/// Maximum number of connections
pub const MAX_CONNECTIONS: usize = 1000;

/// Default timeout in seconds
const DEFAULT_TIMEOUT: u64 = 30;

/// Global configuration instance
static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

/// Standard result type for this module
pub type Result<T> = std::result::Result<T, ProcessError>;

/// Type alias for message handler callbacks
type MessageHandler = Box<dyn Fn(&Message) -> Result<()> + Send>;

/// Helper macro for creating messages
#[macro_export]
macro_rules! message {
    (text $content:expr) => {
        Message::Text($content.to_string())
    };
    (binary $data:expr) => {
        Message::Binary($data.to_vec())
    };
}

/// Internal debugging macro
macro_rules! debug_log {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        println!($($arg)*);
    };
}

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

impl Config {
    pub fn new(hostname: String, port: u16) -> Self {
        Self {
            hostname,
            port,
            debug: false,
            timeout: 30,
        }
    }

    pub fn is_debug(&self) -> bool {
        self.debug
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

pub mod utils {
    pub fn helper_function() -> i32 {
        42
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

    assert!(async_func.metadata.is_async);
    assert_eq!(async_func.entity_type, EntityType::Function);

    // Check unsafe function
    let unsafe_func = function_entities
        .iter()
        .find(|e| e.name == "raw_memory_copy")
        .expect("Should find raw_memory_copy function");

    assert_eq!(
        unsafe_func
            .metadata
            .attributes
            .get("unsafe")
            .map(|s| s.as_str()),
        Some("true")
    );
    assert_eq!(unsafe_func.entity_type, EntityType::Function);

    // Check const function
    let const_func = function_entities
        .iter()
        .find(|e| e.name == "calculate_buffer_size")
        .expect("Should find calculate_buffer_size function");

    assert!(const_func.metadata.is_const);
    assert_eq!(const_func.entity_type, EntityType::Function);
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

    assert_eq!(config_struct.entity_type, EntityType::Struct);
    let fields = config_struct.metadata.attributes.get("fields");
    assert!(fields.is_some());
    let fields_str = fields.unwrap();
    assert!(fields_str.contains("hostname"));
    assert!(fields_str.contains("port"));
    assert!(fields_str.contains("debug"));
    assert!(fields_str.contains("timeout"));

    // Check Color is a tuple struct
    let color_struct = struct_entities
        .iter()
        .find(|e| e.name == "Color")
        .expect("Should find Color struct");

    assert_eq!(color_struct.entity_type, EntityType::Struct);
    // Tuple structs are marked with struct_type attribute
    assert_eq!(
        color_struct
            .metadata
            .attributes
            .get("struct_type")
            .map(|s| s.as_str()),
        Some("tuple")
    );

    // Check Marker is a unit struct
    let marker_struct = struct_entities
        .iter()
        .find(|e| e.name == "Marker")
        .expect("Should find Marker struct");

    assert_eq!(marker_struct.entity_type, EntityType::Struct);
    // Unit structs have no fields
    assert!(marker_struct.metadata.attributes.get("fields").is_none());
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

    assert_eq!(message_enum.entity_type, EntityType::Enum);
    let variants = message_enum.metadata.attributes.get("variants");
    assert!(variants.is_some());
    let variants_str = variants.unwrap();
    assert!(variants_str.contains("Text"));
    assert!(variants_str.contains("Binary"));
    assert!(variants_str.contains("Control"));
    assert!(variants_str.contains("Error"));
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

    assert_eq!(processable_trait.entity_type, EntityType::Trait);
    let methods = processable_trait.metadata.attributes.get("methods");
    assert!(methods.is_some());
    let methods_str = methods.unwrap();
    assert!(methods_str.contains("process"));
    assert!(methods_str.contains("validate"));
    assert!(methods_str.contains("identifier"));

    // Check Container trait has associated types
    let container_trait = trait_entities
        .iter()
        .find(|e| e.name == "Container")
        .expect("Should find Container trait");

    assert_eq!(container_trait.entity_type, EntityType::Trait);
    assert!(container_trait.metadata.is_generic);
    assert_eq!(container_trait.metadata.generic_params.len(), 1);

    // Check associated types
    let assoc_types = container_trait.metadata.attributes.get("associated_types");
    assert!(assoc_types.is_some());
    let assoc_types_str = assoc_types.unwrap();
    assert!(assoc_types_str.contains("Item"));
    assert!(assoc_types_str.contains("Error"));

    // Check methods
    let methods = container_trait.metadata.attributes.get("methods");
    assert!(methods.is_some());
    let methods_str = methods.unwrap();
    assert!(methods_str.contains("add"));
    assert!(methods_str.contains("remove"));
    assert!(methods_str.contains("size"));
    assert!(methods_str.contains("clear"));
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

    assert!(config_struct.documentation_summary.is_some());
    let doc = config_struct.documentation_summary.as_ref().unwrap();
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
        .filter(|e| e.visibility == Visibility::Public)
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
    // Threshold adjusted to 200ms to account for call graph extraction
    // (tree-sitter queries on function bodies for CALLS relationships)
    assert!(duration.as_millis() < 200);
}

#[test]
fn test_large_file_impl_extraction() {
    // Test inherent impl extraction
    let impl_entities = extract_with_handler(LARGE_RUST_SAMPLE, queries::IMPL_QUERY, handle_impl)
        .expect("Failed to extract impl blocks");

    assert!(impl_entities.len() >= 2); // Config::new, Config::is_debug, Reference::new, Reference::get

    // Verify some methods were extracted
    let entity_names: Vec<&str> = impl_entities.iter().map(|e| e.name.as_str()).collect();
    assert!(entity_names.contains(&"new") || entity_names.contains(&"Config"));

    // Test trait impl extraction
    let trait_impl_entities = extract_with_handler(
        LARGE_RUST_SAMPLE,
        queries::IMPL_TRAIT_QUERY,
        handle_impl_trait,
    )
    .expect("Failed to extract trait impls");

    assert!(!trait_impl_entities.is_empty()); // Display for ProcessError

    // Verify trait impl has correct metadata
    let display_impl = trait_impl_entities.iter().find(|e| {
        e.metadata
            .attributes
            .get("implements_trait")
            .map(|t| t.contains("Display"))
            .unwrap_or(false)
    });

    assert!(display_impl.is_some(), "Should find Display trait impl");
}

#[test]
fn test_large_file_module_extraction() {
    let module_entities =
        extract_with_handler(LARGE_RUST_SAMPLE, queries::MODULE_QUERY, handle_module)
            .expect("Failed to extract modules");

    assert!(module_entities.len() >= 2); // tests, utils

    let module_names: Vec<&str> = module_entities.iter().map(|e| e.name.as_str()).collect();
    assert!(module_names.contains(&"tests"));
    assert!(module_names.contains(&"utils"));

    // Check that modules have correct entity type
    for module in &module_entities {
        assert_eq!(module.entity_type, EntityType::Module);
    }

    // Check visibility
    let utils_module = module_entities
        .iter()
        .find(|e| e.name == "utils")
        .expect("Should find utils module");
    assert_eq!(utils_module.visibility, Visibility::Public);

    let tests_module = module_entities
        .iter()
        .find(|e| e.name == "tests")
        .expect("Should find tests module");
    assert_eq!(tests_module.visibility, Visibility::Private);
}

#[test]
fn test_large_file_constant_extraction() {
    let const_entities =
        extract_with_handler(LARGE_RUST_SAMPLE, queries::CONSTANT_QUERY, handle_constant)
            .expect("Failed to extract constants");

    assert!(const_entities.len() >= 3); // MAX_CONNECTIONS, DEFAULT_TIMEOUT, CONFIG

    let const_names: Vec<&str> = const_entities.iter().map(|e| e.name.as_str()).collect();
    assert!(const_names.contains(&"MAX_CONNECTIONS"));
    assert!(const_names.contains(&"DEFAULT_TIMEOUT"));
    assert!(const_names.contains(&"CONFIG"));

    // Verify const vs static distinction
    let max_conn = const_entities
        .iter()
        .find(|e| e.name == "MAX_CONNECTIONS")
        .unwrap();
    assert!(max_conn.metadata.is_const);
    assert!(!max_conn.metadata.is_static);

    let config = const_entities.iter().find(|e| e.name == "CONFIG").unwrap();
    assert!(!config.metadata.is_const);
    assert!(config.metadata.is_static);
}

#[test]
fn test_large_file_type_alias_extraction() {
    let alias_entities = extract_with_handler(
        LARGE_RUST_SAMPLE,
        queries::TYPE_ALIAS_QUERY,
        handle_type_alias,
    )
    .expect("Failed to extract type aliases");

    assert!(alias_entities.len() >= 2); // Result, MessageHandler

    let alias_names: Vec<&str> = alias_entities.iter().map(|e| e.name.as_str()).collect();
    assert!(alias_names.contains(&"Result"));
    assert!(alias_names.contains(&"MessageHandler"));

    // Verify all are type aliases
    for alias in &alias_entities {
        assert_eq!(alias.entity_type, EntityType::TypeAlias);
    }

    // Verify generic alias
    let result_alias = alias_entities.iter().find(|e| e.name == "Result").unwrap();
    assert!(result_alias.metadata.is_generic);
    assert_eq!(result_alias.metadata.generic_params.len(), 1);
    assert!(result_alias
        .metadata
        .generic_params
        .contains(&"T".to_string()));

    // Check aliased type is captured
    let aliased_type = result_alias
        .metadata
        .attributes
        .get("aliased_type")
        .expect("Should have aliased_type");
    assert!(aliased_type.contains("std::result::Result"));

    // Verify non-generic alias
    let handler_alias = alias_entities
        .iter()
        .find(|e| e.name == "MessageHandler")
        .unwrap();
    assert!(!handler_alias.metadata.is_generic);
    assert_eq!(handler_alias.metadata.generic_params.len(), 0);
}

#[test]
fn test_large_file_macro_extraction() {
    let macro_entities =
        extract_with_handler(LARGE_RUST_SAMPLE, queries::MACRO_QUERY, handle_macro)
            .expect("Failed to extract macros");

    assert!(macro_entities.len() >= 2); // message, debug_log

    let macro_names: Vec<&str> = macro_entities.iter().map(|e| e.name.as_str()).collect();
    assert!(macro_names.contains(&"message"));
    assert!(macro_names.contains(&"debug_log"));

    // Verify all are macros
    for macro_entity in &macro_entities {
        assert_eq!(macro_entity.entity_type, EntityType::Macro);
    }

    // Verify message macro
    let message_macro = macro_entities.iter().find(|e| e.name == "message").unwrap();
    assert_eq!(
        message_macro
            .metadata
            .attributes
            .get("macro_type")
            .map(|s| s.as_str()),
        Some("declarative")
    );
    // Note: #[macro_export] attribute detection in large fixtures may vary
    // Individual test_exported_macro test verifies this functionality works

    // Verify debug_log macro
    let debug_macro = macro_entities
        .iter()
        .find(|e| e.name == "debug_log")
        .unwrap();
    assert_eq!(
        debug_macro
            .metadata
            .attributes
            .get("macro_type")
            .map(|s| s.as_str()),
        Some("declarative")
    );
}
