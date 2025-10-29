//! Extract relationships from entity metadata for Neo4j graph construction

use codesearch_core::{CodeEntity, EntityType, Language};
use serde_json::json;
use std::collections::HashMap;

/// Relationship information for Neo4j edge creation
#[derive(Debug, Clone)]
pub struct Relationship {
    pub rel_type: String,
    pub from_id: String,
    pub to_id: Option<String>,
    pub to_name: Option<String>,
    pub properties: HashMap<String, String>,
}

/// Extract CONTAINS relationships from parent_scope
pub fn extract_contains_relationships(entities: &[CodeEntity]) -> Vec<Relationship> {
    let mut relationships = Vec::new();

    // Build qualified_name -> entity_id map
    let name_to_id: HashMap<&str, &str> = entities
        .iter()
        .map(|e| (e.qualified_name.as_str(), e.entity_id.as_str()))
        .collect();

    for entity in entities {
        if let Some(parent_qname) = &entity.parent_scope {
            let from_id = if let Some(&parent_id) = name_to_id.get(parent_qname.as_str()) {
                parent_id.to_string()
            } else {
                // Parent not in this batch, defer resolution
                continue;
            };

            relationships.push(Relationship {
                rel_type: "CONTAINS".to_string(),
                from_id,
                to_id: Some(entity.entity_id.clone()),
                to_name: None,
                properties: HashMap::new(),
            });
        }
    }

    relationships
}

/// Build relationship JSON for outbox payload
pub fn build_contains_relationship_json(
    entity: &CodeEntity,
    entities_in_batch: &[CodeEntity],
) -> Vec<serde_json::Value> {
    let mut relationships = Vec::new();

    if let Some(parent_qname) = &entity.parent_scope {
        // Try to resolve parent within current batch
        let parent_id = entities_in_batch
            .iter()
            .find(|e| e.qualified_name == *parent_qname)
            .map(|e| e.entity_id.clone());

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
            relationships.push(json!({
                "type": "CONTAINS",
                "from_qualified_name": parent_qname,
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
