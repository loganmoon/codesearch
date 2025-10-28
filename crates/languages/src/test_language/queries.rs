//! Test queries for macro validation

// Using a simple valid Rust query for testing
pub const TEST_QUERY: &str = r#"
(function_item
  name: (identifier) @name
) @function
"#;
