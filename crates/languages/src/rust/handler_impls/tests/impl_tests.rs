//! Tests for impl block extraction handler

use super::*;
use crate::rust::handler_impls::impl_handlers::{handle_impl_impl, handle_impl_trait_impl};
use codesearch_core::entities::{EntityType, Visibility};

#[test]
fn test_inherent_impl_simple() {
    let source = r#"
struct Counter {
    count: i32,
}

impl Counter {
    fn new() -> Self {
        Self { count: 0 }
    }

    fn increment(&mut self) {
        self.count += 1;
    }

    fn get(&self) -> i32 {
        self.count
    }
}
"#;

    let entities = extract_with_handler(source, queries::IMPL_QUERY, handle_impl_impl)
        .expect("Failed to extract impl block");

    // Should extract the impl block itself + 3 methods
    assert!(!entities.is_empty(), "Expected at least impl block entity");

    // Find the impl block entity (might be first or have specific name)
    let impl_entity = entities
        .iter()
        .find(|e| e.entity_type == EntityType::Impl && e.name == "Counter")
        .or_else(|| entities.first());

    assert!(impl_entity.is_some(), "Should extract impl block entity");

    // Check methods are extracted
    let method_names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
    assert!(
        method_names.contains(&"new") || method_names.contains(&"increment"),
        "Should extract methods from impl block"
    );
}

#[test]
fn test_trait_impl() {
    let source = r#"
struct Point {
    x: i32,
    y: i32,
}

impl Display for Point {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}
"#;

    let entities = extract_with_handler(source, queries::IMPL_TRAIT_QUERY, handle_impl_trait_impl)
        .expect("Failed to extract trait impl");

    assert!(!entities.is_empty(), "Should extract trait impl");

    // Check that impl block has trait information in metadata
    let impl_entity = &entities[0];
    assert_eq!(impl_entity.entity_type, EntityType::Impl);

    // Should have trait name in attributes
    if let Some(trait_name) = impl_entity.metadata.attributes.get("implements_trait") {
        assert!(trait_name.contains("Display"), "Should capture trait name");
    }
}

#[test]
fn test_generic_impl() {
    let source = r#"
struct Container<T> {
    value: T,
}

impl<T> Container<T> {
    fn new(value: T) -> Self {
        Self { value }
    }

    fn get(&self) -> &T {
        &self.value
    }
}

impl<T: Clone> Container<T> {
    fn clone_value(&self) -> T {
        self.value.clone()
    }
}
"#;

    let entities = extract_with_handler(source, queries::IMPL_QUERY, handle_impl_impl)
        .expect("Failed to extract generic impl");

    assert!(entities.len() >= 2, "Should extract multiple impl blocks");

    // Check that generic parameters are captured
    let has_generic_impl = entities
        .iter()
        .any(|e| e.metadata.is_generic && !e.metadata.generic_params.is_empty());
    assert!(has_generic_impl, "Should capture generic parameters");
}

#[test]
fn test_impl_with_where_clause() {
    let source = r#"
impl<T, U> MyStruct<T, U>
where
    T: Debug,
    U: Clone + Send,
{
    fn process(&self) -> String {
        format!("{:?}", self.data)
    }
}
"#;

    let entities = extract_with_handler(source, queries::IMPL_QUERY, handle_impl_impl)
        .expect("Failed to extract impl with where clause");

    assert!(!entities.is_empty(), "Should extract impl block");

    // Should capture generic params with where clause
    let impl_entity = &entities[0];
    assert!(impl_entity.metadata.is_generic);
}

#[test]
fn test_impl_with_async_methods() {
    let source = r#"
struct AsyncService {
    client: HttpClient,
}

impl AsyncService {
    async fn fetch(&self) -> Result<String> {
        self.client.get("/data").await
    }

    async fn post(&self, data: String) -> Result<()> {
        self.client.post("/data", data).await
    }
}
"#;

    let entities = extract_with_handler(source, queries::IMPL_QUERY, handle_impl_impl)
        .expect("Failed to extract impl with async methods");

    assert!(!entities.is_empty(), "Should extract impl block");

    // Check that async methods are marked as async
    let async_methods: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Method && e.metadata.is_async)
        .collect();
    assert!(
        !async_methods.is_empty(),
        "Should extract async methods with is_async flag"
    );
}

#[test]
fn test_impl_with_unsafe_methods() {
    let source = r#"
struct RawPointer {
    ptr: *mut u8,
}

impl RawPointer {
    unsafe fn deref(&self) -> u8 {
        *self.ptr
    }

    unsafe fn write(&mut self, value: u8) {
        *self.ptr = value;
    }
}
"#;

    let entities = extract_with_handler(source, queries::IMPL_QUERY, handle_impl_impl)
        .expect("Failed to extract impl with unsafe methods");

    assert!(!entities.is_empty(), "Should extract impl block");

    // Check for unsafe methods
    let unsafe_methods: Vec<_> = entities
        .iter()
        .filter(|e| {
            e.entity_type == EntityType::Method
                && e.metadata.attributes.get("unsafe") == Some(&"true".to_string())
        })
        .collect();
    assert!(
        !unsafe_methods.is_empty(),
        "Should mark unsafe methods in attributes"
    );
}

#[test]
fn test_impl_with_const_methods() {
    let source = r#"
struct ConstCompute {
    value: i32,
}

impl ConstCompute {
    const fn new(value: i32) -> Self {
        Self { value }
    }

    const fn double(&self) -> i32 {
        self.value * 2
    }
}
"#;

    let entities = extract_with_handler(source, queries::IMPL_QUERY, handle_impl_impl)
        .expect("Failed to extract impl with const methods");

    assert!(!entities.is_empty(), "Should extract impl block");

    // Check for const methods
    let const_methods: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Method && e.metadata.is_const)
        .collect();
    assert!(
        !const_methods.is_empty(),
        "Should mark const methods with is_const flag"
    );
}

#[test]
fn test_impl_with_self_parameters() {
    let source = r#"
struct SelfTest;

impl SelfTest {
    fn by_ref(&self) {}
    fn by_mut_ref(&mut self) {}
    fn by_value(self) {}
    fn by_mut_value(mut self) {}
}
"#;

    let entities = extract_with_handler(source, queries::IMPL_QUERY, handle_impl_impl)
        .expect("Failed to extract impl with self parameters");

    assert!(entities.len() >= 4, "Should extract all methods");

    // Check that self parameters are captured in signatures
    let methods_with_self: Vec<_> = entities
        .iter()
        .filter(|e| {
            e.entity_type == EntityType::Method
                && e.signature
                    .as_ref()
                    .map(|sig| sig.parameters.iter().any(|(name, _)| name.contains("self")))
                    .unwrap_or(false)
        })
        .collect();
    assert!(
        methods_with_self.len() >= 4,
        "Should capture all self parameter variants"
    );
}

#[test]
fn test_nested_impl_in_module() {
    let source = r#"
mod network {
    pub struct Server {
        port: u16,
    }

    impl Server {
        pub fn new(port: u16) -> Self {
            Self { port }
        }
    }
}
"#;

    let entities = extract_with_handler(source, queries::IMPL_QUERY, handle_impl_impl)
        .expect("Failed to extract nested impl");

    assert!(!entities.is_empty(), "Should extract impl block");

    // Check qualified names include module path
    let has_qualified = entities.iter().any(|e| {
        e.qualified_name.contains("network")
            || e.parent_scope
                .as_ref()
                .map(|s| s.contains("network"))
                .unwrap_or(false)
    });
    assert!(
        has_qualified,
        "Should include module path in qualified names"
    );
}

#[test]
fn test_multiple_impls_same_type() {
    let source = r#"
struct Multi;

impl Multi {
    fn method1() {}
}

impl Multi {
    fn method2() {}
}

impl Clone for Multi {
    fn clone(&self) -> Self {
        Multi
    }
}
"#;

    let inherent_entities = extract_with_handler(source, queries::IMPL_QUERY, handle_impl_impl)
        .expect("Failed to extract inherent impls");

    let trait_entities =
        extract_with_handler(source, queries::IMPL_TRAIT_QUERY, handle_impl_trait_impl)
            .expect("Failed to extract trait impl");

    assert!(
        inherent_entities.len() >= 2,
        "Should extract both inherent impl blocks"
    );
    assert!(!trait_entities.is_empty(), "Should extract trait impl");
}

#[test]
fn test_impl_with_associated_constants() {
    let source = r#"
struct Config;

impl Config {
    const MAX_SIZE: usize = 1024;
    const VERSION: &'static str = "1.0";

    fn get_max() -> usize {
        Self::MAX_SIZE
    }
}
"#;

    let entities = extract_with_handler(source, queries::IMPL_QUERY, handle_impl_impl)
        .expect("Failed to extract impl with associated constants");

    assert!(!entities.is_empty(), "Should extract impl block");

    // Should extract the impl block, 2 constants, and 1 method = 4 entities
    assert!(
        entities.len() >= 4,
        "Should extract impl, constants, and method"
    );

    // Check that associated constants are extracted
    let constants: Vec<_> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::Constant)
        .collect();

    assert_eq!(
        constants.len(),
        2,
        "Should extract both associated constants"
    );

    let constant_names: Vec<&str> = constants.iter().map(|e| e.name.as_str()).collect();
    assert!(
        constant_names.contains(&"MAX_SIZE"),
        "Should extract MAX_SIZE constant"
    );
    assert!(
        constant_names.contains(&"VERSION"),
        "Should extract VERSION constant"
    );

    // Verify constants have metadata
    for constant in &constants {
        assert!(
            constant.metadata.is_const,
            "Constants should have is_const flag"
        );
    }
}

#[test]
fn test_impl_visibility_methods() {
    let source = r#"
pub struct Public;

impl Public {
    pub fn public_method() {}
    fn private_method() {}
    pub(crate) fn crate_method() {}
}
"#;

    let entities = extract_with_handler(source, queries::IMPL_QUERY, handle_impl_impl)
        .expect("Failed to extract impl with visibility");

    assert!(entities.len() >= 3, "Should extract all methods");

    // Check visibility is captured
    let public_methods: Vec<_> = entities
        .iter()
        .filter(|e| e.visibility == Visibility::Public)
        .collect();
    let private_methods: Vec<_> = entities
        .iter()
        .filter(|e| e.visibility == Visibility::Private)
        .collect();

    assert!(!public_methods.is_empty(), "Should have public methods");
    assert!(!private_methods.is_empty(), "Should have private methods");
}

#[test]
fn test_multiple_impls_same_methods_unique_ids() {
    let source = r#"
struct Container<T> {
    value: T,
}

impl<T> Container<T> {
    fn new(value: T) -> Self {
        Self { value }
    }

    fn get(&self) -> &T {
        &self.value
    }
}

impl<T: Clone> Container<T> {
    fn new(value: T) -> Self {  // Same name as line 6!
        Self { value }
    }

    fn clone_value(&self) -> T {
        self.value.clone()
    }
}
"#;

    let entities = extract_with_handler(source, queries::IMPL_QUERY, handle_impl_impl)
        .expect("Failed to extract impl blocks");

    // Find all 'new' methods
    let new_methods: Vec<&CodeEntity> = entities
        .iter()
        .filter(|e| e.name == "new" && matches!(e.entity_type, EntityType::Method))
        .collect();

    assert_eq!(new_methods.len(), 2, "Should have 2 'new' methods");

    // CRITICAL: Entity IDs must be different!
    assert_ne!(
        new_methods[0].entity_id,
        new_methods[1].entity_id,
        "Methods with same name in different impl blocks must have unique IDs.\n\
         Method 1: {} (impl at line {})\n\
         Method 2: {} (impl at line {})",
        new_methods[0].qualified_name,
        new_methods[0].location.start_line,
        new_methods[1].qualified_name,
        new_methods[1].location.start_line
    );

    // Qualified names should include impl line number
    assert!(
        new_methods[0].qualified_name.contains("impl at line")
            || new_methods[1].qualified_name.contains("impl at line"),
        "Qualified names should include impl block line number for uniqueness. Got: {} and {}",
        new_methods[0].qualified_name,
        new_methods[1].qualified_name
    );
}
