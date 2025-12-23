//! Extract relationships from entity metadata for Neo4j graph construction

use codesearch_core::{CodeEntity, EntityType, Language};
use serde_json::json;
use std::collections::HashMap;


/// Relationship information for Neo4j edge creation
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Relationship {
    pub rel_type: String,
    pub from_id: String,
    pub to_id: Option<String>,
    pub to_name: Option<String>,
    pub properties: HashMap<String, String>,
}

/// Build relationship JSON for outbox payload
pub fn build_contains_relationship_json(
    entity: &CodeEntity,
    name_to_id: &HashMap<&str, &str>,
) -> Vec<serde_json::Value> {
    let mut relationships = Vec::new();

    if let Some(parent_qname) = &entity.parent_scope {
        // Try to resolve parent using the provided name_to_id map (O(1) lookup)
        let parent_id = name_to_id
            .get(parent_qname.as_str())
            .map(|&id| id.to_string());

        if let Some(parent_id) = parent_id {
            // Parent exists in batch, create resolved relationship
            relationships.push(json!({
                "type": "CONTAINS",
                "from_id": parent_id,
                "to_id": entity.entity_id.clone(),
                "resolved": true
            }));
        } else {
            // Parent not in batch, store for deferred resolution
            // Use from_name (not from_id) since we need to resolve the parent
            relationships.push(json!({
                "type": "CONTAINS",
                "from_name": parent_qname,
                "to_id": entity.entity_id.clone(),
                "resolved": false
            }));
        }
    }

    relationships
}

/// Extract IMPLEMENTS relationships from Rust impl blocks
pub fn extract_implements_relationships(entity: &CodeEntity) -> Vec<Relationship> {
    let mut relationships = Vec::new();

    if entity.entity_type == EntityType::Impl {
        // Check for trait implementation
        if let Some(trait_name) = entity.metadata.attributes.get("implements_trait") {
            relationships.push(Relationship {
                rel_type: "IMPLEMENTS".to_string(),
                from_id: entity.entity_id.clone(),
                to_id: None,
                to_name: Some(trait_name.clone()),
                properties: HashMap::new(),
            });
        }

        // Check for type association (impl Foo or impl Trait for Foo)
        if let Some(for_type) = entity.metadata.attributes.get("for_type") {
            relationships.push(Relationship {
                rel_type: "ASSOCIATES".to_string(),
                from_id: entity.entity_id.clone(),
                to_id: None,
                to_name: Some(for_type.clone()),
                properties: HashMap::new(),
            });
        }
    }

    relationships
}

/// Extract EXTENDS_INTERFACE relationships from TypeScript interfaces
pub fn extract_extends_interface_relationships(entity: &CodeEntity) -> Vec<Relationship> {
    let mut relationships = Vec::new();

    if entity.entity_type == EntityType::Interface
        && (entity.language == Language::TypeScript || entity.language == Language::JavaScript)
    {
        if let Some(extends) = entity.metadata.attributes.get("extends") {
            // Parse comma-separated interface names: "Base, ICloneable"
            for interface_name in extends.split(',').map(|s| s.trim()) {
                relationships.push(Relationship {
                    rel_type: "EXTENDS_INTERFACE".to_string(),
                    from_id: entity.entity_id.clone(),
                    to_id: None,
                    to_name: Some(interface_name.to_string()),
                    properties: HashMap::new(),
                });
            }
        }
    }

    relationships
}

/// Extract INHERITS_FROM relationships from class declarations
pub fn extract_inherits_from_relationships(entity: &CodeEntity) -> Vec<Relationship> {
    let mut relationships = Vec::new();

    if entity.entity_type == EntityType::Class {
        if let Some(extends) = entity.metadata.attributes.get("extends") {
            relationships.push(Relationship {
                rel_type: "INHERITS_FROM".to_string(),
                from_id: entity.entity_id.clone(),
                to_id: None,
                to_name: Some(extends.clone()),
                properties: HashMap::new(),
            });
        }
    }

    relationships
}

/// Build IMPLEMENTS and EXTENDS_INTERFACE relationship JSON for outbox payload
pub fn build_trait_relationship_json(entity: &CodeEntity) -> Vec<serde_json::Value> {
    let mut relationships = Vec::new();

    // Extract IMPLEMENTS relationships
    for rel in extract_implements_relationships(entity) {
        relationships.push(json!({
            "type": rel.rel_type,
            "from_id": rel.from_id,
            "to_name": rel.to_name,
            "resolved": false
        }));
    }

    // Extract EXTENDS_INTERFACE relationships
    for rel in extract_extends_interface_relationships(entity) {
        relationships.push(json!({
            "type": rel.rel_type,
            "from_id": rel.from_id,
            "to_name": rel.to_name,
            "resolved": false
        }));
    }

    relationships
}

/// Build INHERITS_FROM relationship JSON for outbox payload
pub fn build_inherits_from_relationship_json(entity: &CodeEntity) -> Vec<serde_json::Value> {
    let mut relationships = Vec::new();

    for rel in extract_inherits_from_relationships(entity) {
        relationships.push(json!({
            "type": rel.rel_type,
            "from_id": rel.from_id,
            "to_name": rel.to_name,
            "resolved": false
        }));
    }

    relationships
}

/// Check if a type name is a primitive type that should be filtered from relationships
/// This filters primitives from both Rust and TypeScript/JavaScript
fn is_primitive_type(type_name: &str) -> bool {
    matches!(
        type_name,
        // Rust primitives
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" |
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" |
        "f32" | "f64" |
        "bool" | "char" | "str" | "String" |
        "()" | "!" |
        // TypeScript/JavaScript primitives
        "string" | "number" | "boolean" | "undefined" | "null" | "any" | "unknown" | "void"
    )
}

/// Extract USES relationships from struct fields
/// Primitive types are filtered here rather than at extraction to keep field metadata complete
pub fn extract_uses_relationships(entity: &CodeEntity) -> Vec<Relationship> {
    let mut relationships = Vec::new();

    if entity.entity_type == EntityType::Struct {
        if let Some(fields_json) = entity.metadata.attributes.get("fields") {
            // Parse fields as JSON array
            if let Ok(fields) = serde_json::from_str::<Vec<serde_json::Value>>(fields_json) {
                for field in fields {
                    if let Some(field_type) = field.get("field_type").and_then(|v| v.as_str()) {
                        if let Some(field_name) = field.get("name").and_then(|v| v.as_str()) {
                            // Strip generics: "Vec<String>" -> "Vec"
                            let type_name = field_type
                                .split('<')
                                .next()
                                .unwrap_or(field_type)
                                .trim()
                                .to_string();

                            // Skip primitive types
                            if is_primitive_type(&type_name) {
                                continue;
                            }

                            let mut props = HashMap::new();
                            props.insert("context".to_string(), "field".to_string());
                            props.insert("field_name".to_string(), field_name.to_string());

                            relationships.push(Relationship {
                                rel_type: "USES".to_string(),
                                from_id: entity.entity_id.clone(),
                                to_id: None,
                                to_name: Some(type_name),
                                properties: props,
                            });
                        }
                    }
                }
            }
        }
    }

    relationships
}

/// Build USES relationship JSON for outbox payload
pub fn build_uses_relationship_json(entity: &CodeEntity) -> Vec<serde_json::Value> {
    let mut relationships = Vec::new();

    for rel in extract_uses_relationships(entity) {
        let mut json_rel = json!({
            "type": rel.rel_type,
            "from_id": rel.from_id,
            "to_name": rel.to_name,
            "resolved": false
        });

        // Add properties if present
        if !rel.properties.is_empty() {
            json_rel["properties"] = json!(rel.properties);
        }

        relationships.push(json_rel);
    }

    relationships
}

/// Extract CALLS relationships from function metadata
pub fn extract_calls_relationships(entity: &CodeEntity) -> Vec<Relationship> {
    let mut relationships = Vec::new();

    if matches!(
        entity.entity_type,
        EntityType::Function | EntityType::Method
    ) {
        if let Some(calls_json) = entity.metadata.attributes.get("calls") {
            if let Ok(calls) = serde_json::from_str::<Vec<String>>(calls_json) {
                for callee_name in calls {
                    relationships.push(Relationship {
                        rel_type: "CALLS".to_string(),
                        from_id: entity.entity_id.clone(),
                        to_id: None,
                        to_name: Some(callee_name),
                        properties: HashMap::new(),
                    });
                }
            }
        }
    }

    relationships
}

/// Extract IMPORTS relationships from module metadata
pub fn extract_imports_relationships(entity: &CodeEntity) -> Vec<Relationship> {
    let mut relationships = Vec::new();

    if entity.entity_type == EntityType::Module {
        if let Some(imports_str) = entity.metadata.attributes.get("imports") {
            for import_path in imports_str.split(',') {
                let import_path = import_path.trim();

                relationships.push(Relationship {
                    rel_type: "IMPORTS".to_string(),
                    from_id: entity.entity_id.clone(),
                    to_id: None,
                    to_name: Some(import_path.to_string()),
                    properties: HashMap::new(),
                });
            }
        }
    }

    relationships
}

/// Build CALLS relationship JSON for outbox payload
pub fn build_calls_relationship_json(entity: &CodeEntity) -> Vec<serde_json::Value> {
    let mut relationships = Vec::new();

    for rel in extract_calls_relationships(entity) {
        relationships.push(json!({
            "type": rel.rel_type,
            "from_id": rel.from_id,
            "to_name": rel.to_name,
            "resolved": false
        }));
    }

    relationships
}

/// Build IMPORTS relationship JSON for outbox payload
pub fn build_imports_relationship_json(entity: &CodeEntity) -> Vec<serde_json::Value> {
    let mut relationships = Vec::new();

    for rel in extract_imports_relationships(entity) {
        relationships.push(json!({
            "type": rel.rel_type,
            "from_id": rel.from_id,
            "to_name": rel.to_name,
            "resolved": false
        }));
    }

    relationships
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesearch_core::{
        entities::{CodeEntityBuilder, EntityMetadata, SourceLocation},
        Visibility,
    };
    use std::path::PathBuf;

    fn create_test_entity(
        id: &str,
        name: &str,
        qualified_name: &str,
        entity_type: EntityType,
        parent_scope: Option<String>,
    ) -> CodeEntity {
        let mut builder = CodeEntityBuilder::default();
        builder
            .entity_id(id.to_string())
            .repository_id("test_repo".to_string())
            .name(name.to_string())
            .qualified_name(qualified_name.to_string())
            .entity_type(entity_type)
            .language(Language::Rust)
            .file_path(PathBuf::from("test.rs"))
            .location(SourceLocation {
                start_line: 1,
                start_column: 0,
                end_line: 10,
                end_column: 0,
            })
            .visibility(Visibility::Public)
            .parent_scope(parent_scope);
        builder.build().expect("Failed to build test entity")
    }


    #[test]
    fn test_build_contains_relationship_json_resolved() {
        let parent = create_test_entity(
            "parent_id",
            "Parent",
            "test::Parent",
            EntityType::Module,
            None,
        );
        let child = create_test_entity(
            "child_id",
            "Child",
            "test::Parent::Child",
            EntityType::Function,
            Some("test::Parent".to_string()),
        );

        // Build name_to_id map
        let entities = [parent.clone(), child.clone()];
        let name_to_id: HashMap<&str, &str> = entities
            .iter()
            .map(|e| (e.qualified_name.as_str(), e.entity_id.as_str()))
            .collect();

        let relationships = build_contains_relationship_json(&child, &name_to_id);

        assert_eq!(relationships.len(), 1);
        assert_eq!(relationships[0]["type"], "CONTAINS");
        assert_eq!(relationships[0]["from_id"], "parent_id");
        assert_eq!(relationships[0]["to_id"], "child_id");
        assert_eq!(relationships[0]["resolved"], true);
    }

    #[test]
    fn test_build_contains_relationship_json_unresolved() {
        let child = create_test_entity(
            "child_id",
            "Child",
            "test::Parent::Child",
            EntityType::Function,
            Some("test::Parent".to_string()),
        );

        // Build name_to_id map (parent not in map, so relationship will be unresolved)
        let entities = [child.clone()];
        let name_to_id: HashMap<&str, &str> = entities
            .iter()
            .map(|e| (e.qualified_name.as_str(), e.entity_id.as_str()))
            .collect();

        let relationships = build_contains_relationship_json(&child, &name_to_id);

        assert_eq!(relationships.len(), 1);
        assert_eq!(relationships[0]["type"], "CONTAINS");
        assert_eq!(relationships[0]["from_name"], "test::Parent");
        assert_eq!(relationships[0]["to_id"], "child_id");
        assert_eq!(relationships[0]["resolved"], false);
    }

    #[test]
    fn test_extract_implements_relationships() {
        let mut metadata = EntityMetadata::default();
        metadata
            .attributes
            .insert("implements_trait".to_string(), "MyTrait".to_string());

        let impl_block = CodeEntityBuilder::default()
            .entity_id("impl_id".to_string())
            .repository_id("test_repo".to_string())
            .name("impl MyTrait for MyStruct".to_string())
            .qualified_name("impl_block".to_string())
            .entity_type(EntityType::Impl)
            .language(Language::Rust)
            .file_path(PathBuf::from("test.rs"))
            .location(SourceLocation {
                start_line: 1,
                start_column: 0,
                end_line: 10,
                end_column: 0,
            })
            .visibility(Visibility::Public)
            .metadata(metadata)
            .build()
            .expect("Failed to build test entity");

        let relationships = extract_implements_relationships(&impl_block);

        assert_eq!(relationships.len(), 1);
        assert_eq!(relationships[0].rel_type, "IMPLEMENTS");
        assert_eq!(relationships[0].from_id, "impl_id");
        assert_eq!(relationships[0].to_name, Some("MyTrait".to_string()));
    }

    #[test]
    fn test_extract_implements_relationships_no_trait() {
        let impl_block =
            create_test_entity("impl_id", "impl", "impl_block", EntityType::Impl, None);

        let relationships = extract_implements_relationships(&impl_block);

        assert_eq!(relationships.len(), 0);
    }

    #[test]
    fn test_extract_implements_relationships_wrong_entity_type() {
        let function = create_test_entity(
            "func_id",
            "my_function",
            "test::my_function",
            EntityType::Function,
            None,
        );

        let relationships = extract_implements_relationships(&function);

        assert_eq!(relationships.len(), 0);
    }
}
