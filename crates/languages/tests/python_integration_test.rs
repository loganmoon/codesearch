//! Integration tests for Python language support

use codesearch_core::entities::EntityType;
use codesearch_languages::create_extractor;
use std::path::Path;

/// Helper to filter entities by type (excludes Module entities used for IMPORTS tracking)
fn filter_by_type(
    entities: &[codesearch_core::CodeEntity],
    entity_type: EntityType,
) -> Vec<&codesearch_core::CodeEntity> {
    entities
        .iter()
        .filter(|e| e.entity_type == entity_type)
        .collect()
}

#[test]
fn test_python_extractor_creation() {
    let result = create_extractor(Path::new("test.py"), "test-repo", None, None);
    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[test]
fn test_pyi_extractor_creation() {
    let result = create_extractor(Path::new("stubs.pyi"), "test-repo", None, None);
    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[test]
fn test_extract_simple_function() {
    let source = r#"
def greet(name):
    return f"Hello, {name}!"
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    assert_eq!(entity.name, "greet");
    assert_eq!(entity.language, codesearch_core::entities::Language::Python);

    if let Some(signature) = &entity.signature {
        assert_eq!(signature.parameters.len(), 1);
        assert_eq!(signature.parameters[0].0, "name");
    } else {
        panic!("Expected function signature");
    }
}

#[test]
fn test_extract_typed_function() {
    let source = r#"
def add(a: int, b: int) -> int:
    return a + b
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    assert_eq!(entity.name, "add");

    if let Some(signature) = &entity.signature {
        assert_eq!(signature.parameters.len(), 2);
        assert_eq!(signature.parameters[0].0, "a");
        assert_eq!(signature.parameters[0].1.as_deref(), Some("int"));
        assert_eq!(signature.parameters[1].0, "b");
        assert_eq!(signature.parameters[1].1.as_deref(), Some("int"));
        assert_eq!(signature.return_type.as_deref(), Some("int"));
    } else {
        panic!("Expected function signature");
    }
}

#[test]
fn test_extract_async_function() {
    let source = r#"
async def fetch_data(url: str) -> dict:
    async with aiohttp.ClientSession() as session:
        async with session.get(url) as response:
            return await response.json()
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    assert_eq!(entity.name, "fetch_data");
    assert!(entity.metadata.is_async);
}

#[test]
fn test_extract_function_with_docstring() {
    let source = r#"
def calculate_sum(a: int, b: int) -> int:
    """
    Calculates the sum of two numbers.

    Args:
        a: First number
        b: Second number

    Returns:
        The sum of a and b
    """
    return a + b
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    assert_eq!(entity.name, "calculate_sum");
    assert!(entity.documentation_summary.is_some());

    let doc = entity.documentation_summary.as_ref().unwrap();
    assert!(doc.contains("Calculates the sum"));
}

#[test]
fn test_extract_decorated_function() {
    let source = r#"
@lru_cache(maxsize=128)
@deprecated("Use new_function instead")
def cached_computation(x: int) -> int:
    return x * 2
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    assert_eq!(entity.name, "cached_computation");
    assert!(!entity.metadata.decorators.is_empty());
    assert!(entity
        .metadata
        .decorators
        .iter()
        .any(|d| d.contains("lru_cache")));
}

#[test]
fn test_extract_class() {
    let source = r#"
class Person:
    """A class representing a person."""

    def __init__(self, name: str, age: int):
        self.name = name
        self.age = age

    def greet(self) -> str:
        return f"Hello, I'm {self.name}"
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    // Should extract class and methods
    assert!(!entities.is_empty());

    // Find the class entity
    let class_entity = entities
        .iter()
        .find(|e| e.name == "Person")
        .expect("Should find Person class");

    assert_eq!(
        class_entity.entity_type,
        codesearch_core::entities::EntityType::Class
    );
    assert!(class_entity.documentation_summary.is_some());

    // Find __init__ method
    let init_method = entities
        .iter()
        .find(|e| e.name == "__init__")
        .expect("Should find __init__ method");

    assert_eq!(
        init_method.entity_type,
        codesearch_core::entities::EntityType::Method
    );

    // Check that parent scope is set correctly
    assert!(init_method.qualified_name.contains("Person"));
}

#[test]
fn test_extract_class_with_inheritance() {
    let source = r#"
class Animal:
    pass

class Dog(Animal):
    def bark(self) -> str:
        return "Woof!"
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    // Find the Dog class
    let dog_class = entities
        .iter()
        .find(|e| e.name == "Dog")
        .expect("Should find Dog class");

    // Check that base class is captured
    assert!(dog_class
        .metadata
        .attributes
        .get("bases")
        .map(|b| b.contains("Animal"))
        .unwrap_or(false));
}

#[test]
fn test_extract_decorated_class() {
    let source = r#"
@dataclass
class Point:
    x: float
    y: float
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let class_entity = entities
        .iter()
        .find(|e| e.name == "Point")
        .expect("Should find Point class");

    assert!(!class_entity.metadata.decorators.is_empty());
    assert!(class_entity
        .metadata
        .decorators
        .iter()
        .any(|d| d.contains("dataclass")));
}

#[test]
fn test_extract_staticmethod() {
    let source = r#"
class Calculator:
    @staticmethod
    def add(a: int, b: int) -> int:
        return a + b
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let method = entities
        .iter()
        .find(|e| e.name == "add")
        .expect("Should find add method");

    assert_eq!(
        method.entity_type,
        codesearch_core::entities::EntityType::Method
    );
    assert!(method
        .metadata
        .decorators
        .iter()
        .any(|d| d == "staticmethod"));
    assert_eq!(
        method.metadata.attributes.get("static"),
        Some(&"true".to_string())
    );
}

#[test]
fn test_extract_classmethod() {
    let source = r#"
class Factory:
    @classmethod
    def create(cls, name: str) -> "Factory":
        return cls(name)
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let method = entities
        .iter()
        .find(|e| e.name == "create")
        .expect("Should find create method");

    assert!(method
        .metadata
        .decorators
        .iter()
        .any(|d| d == "classmethod"));
    assert_eq!(
        method.metadata.attributes.get("classmethod"),
        Some(&"true".to_string())
    );
}

#[test]
fn test_extract_property() {
    let source = r#"
class Circle:
    def __init__(self, radius: float):
        self._radius = radius

    @property
    def radius(self) -> float:
        return self._radius
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let property_method = entities
        .iter()
        .find(|e| e.name == "radius")
        .expect("Should find radius property");

    assert!(property_method
        .metadata
        .decorators
        .iter()
        .any(|d| d == "property"));
    assert_eq!(
        property_method.metadata.attributes.get("property"),
        Some(&"true".to_string())
    );
}

#[test]
fn test_extract_variadic_parameters() {
    let source = r#"
def variadic_func(*args, **kwargs) -> None:
    pass
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    if let Some(signature) = &entity.signature {
        assert_eq!(signature.parameters.len(), 2);
        assert!(signature.parameters[0].0.starts_with('*'));
        assert!(signature.parameters[1].0.starts_with("**"));
    } else {
        panic!("Expected function signature");
    }
}

#[test]
fn test_extract_default_parameters() {
    let source = r#"
def greet(name: str, greeting: str = "Hello") -> str:
    return f"{greeting}, {name}!"
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    if let Some(signature) = &entity.signature {
        assert_eq!(signature.parameters.len(), 2);
        assert_eq!(signature.parameters[0].0, "name");
        assert_eq!(signature.parameters[1].0, "greeting");
    } else {
        panic!("Expected function signature");
    }
}

#[test]
fn test_extract_multiple_entities() {
    let source = r#"
def foo():
    pass

class Bar:
    def method(self):
        pass

async def baz():
    pass
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    // Should have: foo, Bar, Bar.method, baz
    assert!(entities.len() >= 4);

    let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"foo"));
    assert!(names.contains(&"Bar"));
    assert!(names.contains(&"method"));
    assert!(names.contains(&"baz"));
}

#[test]
fn test_method_self_parameter_filtered() {
    let source = r#"
class Example:
    def instance_method(self, x: int) -> int:
        return x
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let method = entities
        .iter()
        .find(|e| e.name == "instance_method")
        .expect("Should find instance_method");

    if let Some(signature) = &method.signature {
        // self should be filtered out from display parameters
        assert_eq!(signature.parameters.len(), 1);
        assert_eq!(signature.parameters[0].0, "x");
    } else {
        panic!("Expected method signature");
    }
}

#[test]
fn test_nested_class() {
    let source = r#"
class Outer:
    class Inner:
        def method(self):
            pass
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    // Find the Inner class
    let inner_class = entities.iter().find(|e| e.name == "Inner");
    assert!(inner_class.is_some(), "Should find Inner class");

    // Find the method inside Inner
    let method = entities.iter().find(|e| e.name == "method");
    assert!(method.is_some(), "Should find method in Inner class");
}

#[test]
fn test_unicode_identifiers() {
    let source = r#"
def 计算(数值: int) -> int:
    """计算函数"""
    return 数值 * 2
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, "计算");
}

#[test]
fn test_extract_empty_source() {
    let source = "";

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    assert!(entities.is_empty());
}

#[test]
fn test_extract_whitespace_only_source() {
    let source = "   \n\n  \t  \n";

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    assert!(entities.is_empty());
}

#[test]
fn test_extract_comment_only_source() {
    let source = r#"
# This is a comment
# Another comment
"""
Module docstring
"""
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    // No functions or classes, just comments
    assert!(entities.is_empty());
}

#[test]
fn test_extract_positional_only_parameters() {
    // Python 3.8+ positional-only parameters with /
    let source = r#"
def func(a, b, /, c, d):
    pass
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    if let Some(signature) = &entity.signature {
        // Should have: a, b, /, c, d
        assert_eq!(signature.parameters.len(), 5);
        assert_eq!(signature.parameters[0].0, "a");
        assert_eq!(signature.parameters[1].0, "b");
        assert_eq!(signature.parameters[2].0, "/");
        assert_eq!(signature.parameters[3].0, "c");
        assert_eq!(signature.parameters[4].0, "d");
    } else {
        panic!("Expected function signature");
    }
}

#[test]
fn test_extract_keyword_only_parameters() {
    // Python 3.0+ keyword-only parameters with bare *
    let source = r#"
def func(a, *, b, c):
    pass
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    if let Some(signature) = &entity.signature {
        // Should have: a, *, b, c
        assert_eq!(signature.parameters.len(), 4);
        assert_eq!(signature.parameters[0].0, "a");
        assert_eq!(signature.parameters[1].0, "*");
        assert_eq!(signature.parameters[2].0, "b");
        assert_eq!(signature.parameters[3].0, "c");
    } else {
        panic!("Expected function signature");
    }
}

#[test]
fn test_extract_combined_parameter_syntax() {
    // Python 3.8+ with both positional-only and keyword-only
    let source = r#"
def func(pos_only, /, standard, *, kw_only):
    pass
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let functions = filter_by_type(&entities, EntityType::Function);
    assert_eq!(functions.len(), 1);
    let entity = functions[0];

    if let Some(signature) = &entity.signature {
        // Should have: pos_only, /, standard, *, kw_only
        assert_eq!(signature.parameters.len(), 5);
        assert_eq!(signature.parameters[0].0, "pos_only");
        assert_eq!(signature.parameters[1].0, "/");
        assert_eq!(signature.parameters[2].0, "standard");
        assert_eq!(signature.parameters[3].0, "*");
        assert_eq!(signature.parameters[4].0, "kw_only");
    } else {
        panic!("Expected function signature");
    }
}

#[test]
fn test_extract_async_method() {
    let source = r#"
class Client:
    async def fetch(self, url: str) -> dict:
        pass
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let method = entities
        .iter()
        .find(|e| e.name == "fetch")
        .expect("Should find fetch method");

    assert_eq!(
        method.entity_type,
        codesearch_core::entities::EntityType::Method
    );
    assert!(method.metadata.is_async);
}

#[test]
fn test_extract_multiple_inheritance() {
    let source = r#"
class Child(Parent1, Parent2, Parent3):
    pass
    "#;

    let extractor = create_extractor(Path::new("test.py"), "test-repo", None, None)
        .expect("Failed to create extractor")
        .expect("No extractor for .py");

    let entities = extractor
        .extract(source, Path::new("test.py"))
        .expect("Failed to extract entities");

    let class_entity = entities
        .iter()
        .find(|e| e.name == "Child")
        .expect("Should find Child class");

    let bases = class_entity
        .metadata
        .attributes
        .get("bases")
        .expect("Should have bases attribute");

    assert!(bases.contains("Parent1"));
    assert!(bases.contains("Parent2"));
    assert!(bases.contains("Parent3"));
}
