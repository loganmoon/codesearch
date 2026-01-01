//! Tests for Python class extraction handlers

use super::*;
use crate::python::handler_impls::{handle_class_impl, handle_method_impl};
use codesearch_core::entities::{EntityType, SourceReference};

#[test]
fn test_simple_class() {
    let source = r#"
class Person:
    def __init__(self, name):
        self.name = name
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Person");
    assert_eq!(entity.qualified_name, "Person");
    assert_eq!(entity.entity_type, EntityType::Class);
    assert!(entity.parent_scope.is_none());
}

#[test]
fn test_class_with_base_class() {
    let source = r#"
class Employee(Person):
    def __init__(self, name, employee_id):
        super().__init__(name)
        self.employee_id = employee_id
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Employee");
    assert_eq!(entity.entity_type, EntityType::Class);
    assert!(entity.metadata.attributes.contains_key("bases"));
    let bases = entity.metadata.attributes.get("bases").unwrap();
    assert!(bases.contains("Person"));
}

#[test]
fn test_class_with_multiple_bases() {
    let source = r#"
class MultiInherit(Base1, Base2, Base3):
    pass
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert!(entity.metadata.attributes.contains_key("bases"));
}

#[test]
fn test_class_with_docstring() {
    let source = r#"
class User:
    """
    Represents a user in the system.

    Attributes:
        name: The user's name
        email: The user's email address
    """
    def __init__(self, name, email):
        self.name = name
        self.email = email
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "User");
    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("Represents a user"));
}

#[test]
fn test_class_with_decorator() {
    let source = r#"
@dataclass
class Point:
    x: int
    y: int
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "Point");
    assert!(entity
        .metadata
        .decorators
        .contains(&"dataclass".to_string()));
}

#[test]
fn test_class_qualified_name() {
    let source = r#"
class SimpleClass:
    pass
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.qualified_name, "SimpleClass");
    assert!(entity.parent_scope.is_none());
}

#[test]
fn test_simple_method() {
    let source = r#"
class Calculator:
    def add(self, a, b):
        return a + b
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "add");
    assert_eq!(entity.entity_type, EntityType::Method);

    let sig = entity.signature.as_ref().expect("Should have signature");
    assert_eq!(sig.parameters.len(), 2);
    assert_eq!(sig.parameters[0].0, "a");
    assert_eq!(sig.parameters[1].0, "b");
}

#[test]
fn test_method_qualified_name_in_class() {
    let source = r#"
class Calculator:
    def multiply(self, a, b):
        return a * b
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "multiply");
    assert_eq!(entity.qualified_name, "Calculator.multiply");
    assert_eq!(entity.parent_scope.as_deref(), Some("Calculator"));
}

#[test]
fn test_async_method() {
    let source = r#"
class DataService:
    async def fetch_data(self, url):
        response = await aiohttp.get(url)
        return await response.json()
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "fetch_data");
    assert!(entity.metadata.is_async);

    let sig = entity.signature.as_ref().expect("Should have signature");
    assert!(sig.is_async);
}

#[test]
fn test_static_method() {
    let source = r#"
class MathUtils:
    @staticmethod
    def square(x):
        return x * x
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "square");
    assert_eq!(
        entity.metadata.attributes.get("static").map(|s| s.as_str()),
        Some("true")
    );
}

#[test]
fn test_classmethod() {
    let source = r#"
class Factory:
    @classmethod
    def create(cls, value):
        return cls(value)
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "create");
    assert_eq!(
        entity
            .metadata
            .attributes
            .get("classmethod")
            .map(|s| s.as_str()),
        Some("true")
    );
}

#[test]
fn test_property_method() {
    let source = r#"
class Circle:
    @property
    def area(self):
        return 3.14159 * self.radius ** 2
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "area");
    assert_eq!(
        entity
            .metadata
            .attributes
            .get("property")
            .map(|s| s.as_str()),
        Some("true")
    );
}

#[test]
fn test_multiple_methods() {
    let source = r#"
class Calculator:
    def add(self, a, b):
        return a + b

    def subtract(self, a, b):
        return a - b

    def multiply(self, a, b):
        return a * b
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract methods");

    assert_eq!(entities.len(), 3);
    assert_eq!(entities[0].name, "add");
    assert_eq!(entities[1].name, "subtract");
    assert_eq!(entities[2].name, "multiply");

    for entity in &entities {
        assert_eq!(entity.parent_scope.as_deref(), Some("Calculator"));
        assert!(entity.qualified_name.starts_with("Calculator."));
    }
}

#[test]
fn test_method_with_docstring() {
    let source = r#"
class Calculator:
    def add(self, a, b):
        """
        Add two numbers together.

        Args:
            a: First number
            b: Second number

        Returns:
            The sum of a and b
        """
        return a + b
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert!(entity.documentation_summary.is_some());
    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("Add two numbers"));
}

#[test]
fn test_method_with_type_annotations() {
    let source = r#"
class Calculator:
    def add(self, a: int, b: int) -> int:
        return a + b
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    let sig = entity.signature.as_ref().expect("Should have signature");
    assert_eq!(sig.parameters[0].1.as_deref(), Some("int"));
    assert_eq!(sig.return_type.as_deref(), Some("int"));
}

#[test]
fn test_init_method() {
    let source = r#"
class Person:
    def __init__(self, name, age):
        self.name = name
        self.age = age
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "__init__");
    assert_eq!(entity.qualified_name, "Person.__init__");
}

#[test]
fn test_method_self_parameter_filtered() {
    let source = r#"
class MyClass:
    def method(self, x, y):
        return x + y
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    let sig = entity.signature.as_ref().expect("Should have signature");
    assert_eq!(sig.parameters.len(), 2);
    assert!(!sig.parameters.iter().any(|(name, _)| name == "self"));
}

// ============================================================================
// Tests for bases_resolved extraction
// ============================================================================

#[test]
fn test_class_bases_resolved() {
    let source = r#"
from models import BaseModel

class User(BaseModel):
    name: str
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity.name, "User");

    // Should have bases attribute with resolved qualified names (JSON array)
    let bases_attr = entity
        .metadata
        .attributes
        .get("bases")
        .expect("Should have bases");

    let bases: Vec<String> = serde_json::from_str(bases_attr).expect("Should parse bases JSON");
    // Absolute imports are marked with external. prefix
    assert!(bases.contains(&"external.models.BaseModel".to_string()));
}

#[test]
fn test_class_bases_resolved_external() {
    let source = r#"
class MyClass(ExternalBase):
    pass
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    let bases_attr = entity
        .metadata
        .attributes
        .get("bases")
        .expect("Should have bases");

    let bases: Vec<String> = serde_json::from_str(bases_attr).expect("Should parse bases JSON");
    // Should have external prefix for unresolved references
    assert!(bases.contains(&"external.ExternalBase".to_string()));
}

#[test]
fn test_class_multiple_bases_resolved() {
    let source = r#"
from base_a import BaseA
from base_b import BaseB

class MultiInherit(BaseA, BaseB):
    pass
"#;

    let entities = extract_with_handler(source, queries::CLASS_QUERY, handle_class_impl)
        .expect("Failed to extract class");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    let bases_attr = entity
        .metadata
        .attributes
        .get("bases")
        .expect("Should have bases");

    let bases: Vec<String> = serde_json::from_str(bases_attr).expect("Should parse bases JSON");
    assert_eq!(bases.len(), 2);
    // Absolute imports are marked with external. prefix
    assert!(bases.contains(&"external.base_a.BaseA".to_string()));
    assert!(bases.contains(&"external.base_b.BaseB".to_string()));
}

// ============================================================================
// Tests for method calls extraction
// ============================================================================

#[test]
fn test_method_extracts_calls() {
    let source = r#"
class MyService:
    def process(self):
        self.helper()
        print("done")
"#;

    let entities = extract_with_handler(source, queries::METHOD_QUERY, handle_method_impl)
        .expect("Failed to extract method");

    assert_eq!(entities.len(), 1);
    let entity = &entities[0];

    let calls_attr = entity.metadata.attributes.get("calls");
    assert!(calls_attr.is_some(), "Should have calls attribute");

    let calls: Vec<SourceReference> =
        serde_json::from_str(calls_attr.unwrap()).expect("Should parse calls JSON");
    // Should capture the print call
    assert!(calls.iter().any(|c| c.target().contains("print")));
}
