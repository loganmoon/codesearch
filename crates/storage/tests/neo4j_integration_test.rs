//! Integration tests for Neo4j client operations

mod common;

use anyhow::Result;
use codesearch_core::{entities::EntityType, CodeEntity};
use codesearch_storage::Neo4jClient;
use common::*;
use std::collections::HashMap;
use uuid::Uuid;

/// Test that verifies Cypher injection protection
#[tokio::test]
#[ignore] // Requires Neo4j to be running
async fn test_cypher_injection_protection() -> Result<()> {
    let config = create_storage_config(6334, 6333, 5432, "test_db");
    let neo4j_client = Neo4jClient::new(&config).await?;

    // Create test database
    let db_name = format!("test_{}", Uuid::new_v4().simple());
    neo4j_client.create_database(&db_name).await?;
    neo4j_client.use_database(&db_name).await?;

    // Create two test entities
    let entity1 = create_test_entity("entity1", EntityType::Function);
    let entity2 = create_test_entity("entity2", EntityType::Function);

    neo4j_client.create_entity_node(&entity1).await?;
    neo4j_client.create_entity_node(&entity2).await?;

    // Attempt to create relationship with invalid type (injection attempt)
    let result = neo4j_client
        .create_relationship(
            &entity1.entity_id,
            &entity2.entity_id,
            "MALICIOUS'; DROP DATABASE test; //",
            &HashMap::new(),
        )
        .await;

    // Should fail with validation error
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid relationship type"));

    // Cleanup
    neo4j_client.drop_database(&db_name).await?;

    Ok(())
}

/// Test batch node creation with UNWIND
#[tokio::test]
#[ignore] // Requires Neo4j to be running
async fn test_batch_create_nodes() -> Result<()> {
    let config = create_storage_config(6334, 6333, 5432, "test_db");
    let neo4j_client = Neo4jClient::new(&config).await?;

    // Create test database
    let db_name = format!("test_{}", Uuid::new_v4().simple());
    neo4j_client.create_database(&db_name).await?;
    neo4j_client.use_database(&db_name).await?;

    // Create multiple test entities of different types
    let entities = vec![
        create_test_entity("func1", EntityType::Function),
        create_test_entity("func2", EntityType::Function),
        create_test_entity("method1", EntityType::Method),
        create_test_entity("class1", EntityType::Class),
        create_test_entity("struct1", EntityType::Struct),
    ];

    // Batch create all nodes
    let node_ids = neo4j_client.batch_create_nodes(&entities).await?;

    // Verify all nodes were created
    assert_eq!(node_ids.len(), 5);

    // Verify nodes exist
    for entity in &entities {
        let exists = neo4j_client.node_exists(&entity.entity_id).await?;
        assert!(exists, "Entity {} should exist", entity.entity_id);
    }

    // Cleanup
    neo4j_client.drop_database(&db_name).await?;

    Ok(())
}

/// Test batch relationship creation with UNWIND
#[tokio::test]
#[ignore] // Requires Neo4j to be running
async fn test_batch_create_relationships() -> Result<()> {
    let config = create_storage_config(6334, 6333, 5432, "test_db");
    let neo4j_client = Neo4jClient::new(&config).await?;

    // Create test database
    let db_name = format!("test_{}", Uuid::new_v4().simple());
    neo4j_client.create_database(&db_name).await?;
    neo4j_client.use_database(&db_name).await?;

    // Create test entities
    let entities = vec![
        create_test_entity("caller1", EntityType::Function),
        create_test_entity("caller2", EntityType::Function),
        create_test_entity("callee1", EntityType::Function),
        create_test_entity("callee2", EntityType::Function),
    ];

    neo4j_client.batch_create_nodes(&entities).await?;

    // Create relationships
    let relationships = vec![
        (
            entities[0].entity_id.clone(),
            entities[2].entity_id.clone(),
            "CALLS".to_string(),
        ),
        (
            entities[0].entity_id.clone(),
            entities[3].entity_id.clone(),
            "CALLS".to_string(),
        ),
        (
            entities[1].entity_id.clone(),
            entities[2].entity_id.clone(),
            "CALLS".to_string(),
        ),
    ];

    // Batch create relationships
    neo4j_client
        .batch_create_relationships(&relationships)
        .await?;

    // Verify relationships were created (would need query support to fully verify)
    // For now, just verify no error occurred

    // Cleanup
    neo4j_client.drop_database(&db_name).await?;

    Ok(())
}

/// Test relationship resolution with varied inputs
#[tokio::test]
#[ignore] // Requires Neo4j to be running
async fn test_relationship_resolution() -> Result<()> {
    let config = create_storage_config(6334, 6333, 5432, "test_db");
    let neo4j_client = Neo4jClient::new(&config).await?;

    // Create test database
    let db_name = format!("test_{}", Uuid::new_v4().simple());
    neo4j_client.create_database(&db_name).await?;
    neo4j_client.use_database(&db_name).await?;

    // Create test entities with various relationship types
    let trait_entity = create_test_entity_with_name("MyTrait", "MyTrait", EntityType::Trait);
    let impl_entity = create_test_entity("impl1", EntityType::Impl);
    let struct_entity = create_test_entity_with_name("MyStruct", "MyStruct", EntityType::Struct);

    neo4j_client
        .batch_create_nodes(&vec![
            trait_entity.clone(),
            impl_entity.clone(),
            struct_entity.clone(),
        ])
        .await?;

    // Test all allowed relationship types
    let relationship_types = vec![
        "IMPLEMENTS",
        "ASSOCIATES",
        "EXTENDS_INTERFACE",
        "INHERITS_FROM",
        "USES",
        "CALLS",
        "IMPORTS",
        "CONTAINS",
    ];

    for rel_type in relationship_types {
        let result = neo4j_client
            .create_relationship(
                &impl_entity.entity_id,
                &trait_entity.entity_id,
                rel_type,
                &HashMap::new(),
            )
            .await;

        assert!(
            result.is_ok(),
            "Relationship type {rel_type} should be allowed"
        );
    }

    // Cleanup
    neo4j_client.drop_database(&db_name).await?;

    Ok(())
}

/// Helper function to create a test entity
fn create_test_entity(id_suffix: &str, entity_type: EntityType) -> CodeEntity {
    create_test_entity_with_name(id_suffix, id_suffix, entity_type)
}

/// Helper function to create a test entity with custom name
fn create_test_entity_with_name(
    id_suffix: &str,
    name: &str,
    entity_type: EntityType,
) -> CodeEntity {
    use codesearch_core::{
        entities::{CodeEntityBuilder, SourceLocation},
        Language, Visibility,
    };
    use std::path::PathBuf;

    CodeEntityBuilder::default()
        .entity_id(format!("test_{id_suffix}"))
        .repository_id(Uuid::new_v4().to_string())
        .qualified_name(format!("test::{name}"))
        .name(name.to_string())
        .entity_type(entity_type)
        .language(Language::Rust)
        .visibility(Visibility::Public)
        .file_path(PathBuf::from(format!("/test/{id_suffix}.rs")))
        .location(SourceLocation {
            start_line: 1,
            start_column: 0,
            end_line: 10,
            end_column: 0,
        })
        .build()
        .expect("Failed to build test entity")
}
