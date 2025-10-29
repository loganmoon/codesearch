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

/// Test the end-to-end relationship resolution workflow
#[tokio::test]
#[ignore] // Requires Neo4j to be running
async fn test_relationship_resolution_workflow() -> Result<()> {
    let config = create_storage_config(6334, 6333, 5432, "test_db");
    let neo4j_client = Neo4jClient::new(&config).await?;

    // Create test database
    let db_name = format!("test_{}", Uuid::new_v4().simple());
    neo4j_client.create_database(&db_name).await?;
    neo4j_client.use_database(&db_name).await?;

    // Step 1: Create a child entity that references a parent that doesn't exist yet
    let child = create_test_entity_with_name("child", "child", EntityType::Function);
    neo4j_client.create_entity_node(&child).await?;

    // Step 2: Store unresolved CONTAINS relationship as node property
    // (Simulates what the outbox processor does when parent doesn't exist)
    let store_unresolved_query = "MATCH (n {id: $child_id})
                                   SET n.`unresolved_contains_parent` = $parent_qname"
        .to_string();
    neo4j_client
        .graph()
        .run(
            neo4rs::Query::new(store_unresolved_query)
                .param("child_id", child.entity_id.as_str())
                .param("parent_qname", "test::Parent"),
        )
        .await?;

    // Step 3: Verify the unresolved property was stored
    let verify_unresolved_query = "MATCH (n {id: $child_id})
                                    RETURN n.`unresolved_contains_parent` as parent_qname"
        .to_string();
    let mut result = neo4j_client
        .graph()
        .execute(
            neo4rs::Query::new(verify_unresolved_query).param("child_id", child.entity_id.as_str()),
        )
        .await?;

    let row = result
        .next()
        .await?
        .ok_or_else(|| anyhow::anyhow!("No row returned"))?;
    let parent_qname: String = row.get("parent_qname")?;
    assert_eq!(parent_qname, "test::Parent");

    // Step 4: Create the parent entity (simulates parent being indexed later)
    let parent = create_test_entity_with_name("parent", "Parent", EntityType::Module);
    neo4j_client.create_entity_node(&parent).await?;

    // Step 5: Resolve the relationship - create edge and remove property
    // (Simulates what the outbox processor does after parent is indexed)
    let relationships = vec![(
        parent.entity_id.clone(),
        child.entity_id.clone(),
        "CONTAINS".to_string(),
    )];
    neo4j_client
        .batch_create_relationships(&relationships)
        .await?;

    // Step 6: Clean up the unresolved property
    let cleanup_query = "MATCH (n {id: $child_id})
                         REMOVE n.`unresolved_contains_parent`"
        .to_string();
    neo4j_client
        .graph()
        .run(neo4rs::Query::new(cleanup_query).param("child_id", child.entity_id.as_str()))
        .await?;

    // Step 7: Verify relationship exists and property was removed
    let verify_relationship_query =
        "MATCH (parent {id: $parent_id})-[:CONTAINS]->(child {id: $child_id})
                                     RETURN parent.id as parent_id, child.id as child_id"
            .to_string();
    let mut rel_result = neo4j_client
        .graph()
        .execute(
            neo4rs::Query::new(verify_relationship_query)
                .param("parent_id", parent.entity_id.as_str())
                .param("child_id", child.entity_id.as_str()),
        )
        .await?;

    let rel_row = rel_result
        .next()
        .await?
        .ok_or_else(|| anyhow::anyhow!("Relationship not found"))?;
    let found_parent_id: String = rel_row.get("parent_id")?;
    let found_child_id: String = rel_row.get("child_id")?;
    assert_eq!(found_parent_id, parent.entity_id);
    assert_eq!(found_child_id, child.entity_id);

    // Verify property was removed
    let verify_no_property_query = "MATCH (n {id: $child_id})
                                    RETURN n.`unresolved_contains_parent` as prop"
        .to_string();
    let mut prop_result = neo4j_client
        .graph()
        .execute(
            neo4rs::Query::new(verify_no_property_query)
                .param("child_id", child.entity_id.as_str()),
        )
        .await?;

    if let Some(prop_row) = prop_result.next().await? {
        let prop: Option<String> = prop_row.get("prop").ok();
        assert!(prop.is_none(), "Unresolved property should be removed");
    }

    // Cleanup
    neo4j_client.drop_database(&db_name).await?;

    Ok(())
}

/// Test property key validation (Cypher injection protection)
#[tokio::test]
#[ignore] // Requires Neo4j to be running
async fn test_property_key_validation() -> Result<()> {
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

    // Attempt injection via property key
    let mut malicious_props = HashMap::new();
    malicious_props.insert("x; DROP DATABASE test; //".to_string(), "value".to_string());

    let result = neo4j_client
        .create_relationship(
            &entity1.entity_id,
            &entity2.entity_id,
            "CALLS",
            &malicious_props,
        )
        .await;

    // Should fail with validation error
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid property key"));

    // Test empty property key
    let mut empty_key_props = HashMap::new();
    empty_key_props.insert("".to_string(), "value".to_string());

    let result2 = neo4j_client
        .create_relationship(
            &entity1.entity_id,
            &entity2.entity_id,
            "CALLS",
            &empty_key_props,
        )
        .await;

    assert!(result2.is_err());
    assert!(result2
        .unwrap_err()
        .to_string()
        .contains("Property key cannot be empty"));

    // Test valid property keys
    let mut valid_props = HashMap::new();
    valid_props.insert("valid_key_123".to_string(), "value".to_string());
    valid_props.insert("AnotherKey".to_string(), "value2".to_string());

    let result3 = neo4j_client
        .create_relationship(
            &entity1.entity_id,
            &entity2.entity_id,
            "CALLS",
            &valid_props,
        )
        .await;

    assert!(result3.is_ok(), "Valid property keys should be accepted");

    // Cleanup
    neo4j_client.drop_database(&db_name).await?;

    Ok(())
}

/// Test batch CONTAINS relationship resolution
#[tokio::test]
#[ignore] // Requires Neo4j to be running
async fn test_batch_contains_resolution() -> Result<()> {
    let config = create_storage_config(6334, 6333, 5432, "test_db");
    let neo4j_client = Neo4jClient::new(&config).await?;

    // Create test database
    let db_name = format!("test_{}", Uuid::new_v4().simple());
    neo4j_client.create_database(&db_name).await?;
    neo4j_client.use_database(&db_name).await?;

    // Create parent entities
    let parent1 = create_test_entity_with_name("parent1", "Parent1", EntityType::Module);
    let parent2 = create_test_entity_with_name("parent2", "Parent2", EntityType::Module);
    let parent3 = create_test_entity_with_name("parent3", "Parent3", EntityType::Module);

    // Create child entities
    let child1 = create_test_entity_with_name("child1", "child1", EntityType::Function);
    let child2 = create_test_entity_with_name("child2", "child2", EntityType::Function);
    let child3 = create_test_entity_with_name("child3", "child3", EntityType::Function);
    let child4 = create_test_entity_with_name("child4", "child4", EntityType::Function);

    // Create all entities
    neo4j_client
        .batch_create_nodes(&vec![
            parent1.clone(),
            parent2.clone(),
            parent3.clone(),
            child1.clone(),
            child2.clone(),
            child3.clone(),
            child4.clone(),
        ])
        .await?;

    // Prepare batch resolution data: 3 valid + 1 with non-existent parent
    let unresolved_nodes = vec![
        (child1.entity_id.clone(), parent1.qualified_name.clone()),
        (child2.entity_id.clone(), parent2.qualified_name.clone()),
        (child3.entity_id.clone(), parent3.qualified_name.clone()),
        (
            child4.entity_id.clone(),
            "test::NonExistentParent".to_string(),
        ),
    ];

    // Batch resolve
    let resolved_count = neo4j_client
        .resolve_contains_relationships_batch(&unresolved_nodes)
        .await?;

    // Should resolve 3 out of 4 (fourth parent doesn't exist)
    assert_eq!(resolved_count, 3);

    // Verify relationships were created
    for (child_id, parent_qname) in &unresolved_nodes[0..3] {
        let verify_query =
            "MATCH (parent {qualified_name: $parent_qname})-[:CONTAINS]->(child {id: $child_id})
             RETURN count(*) as count"
                .to_string();
        let mut result = neo4j_client
            .graph()
            .execute(
                neo4rs::Query::new(verify_query)
                    .param("parent_qname", parent_qname.as_str())
                    .param("child_id", child_id.as_str()),
            )
            .await?;

        if let Some(row) = result.next().await? {
            let count: i64 = row.get("count")?;
            assert_eq!(count, 1, "CONTAINS relationship should exist");
        }
    }

    // Cleanup
    neo4j_client.drop_database(&db_name).await?;

    Ok(())
}

/// Test handling of duplicate relationships
#[tokio::test]
#[ignore] // Requires Neo4j to be running
async fn test_duplicate_relationships() -> Result<()> {
    let config = create_storage_config(6334, 6333, 5432, "test_db");
    let neo4j_client = Neo4jClient::new(&config).await?;

    // Create test database
    let db_name = format!("test_{}", Uuid::new_v4().simple());
    neo4j_client.create_database(&db_name).await?;
    neo4j_client.use_database(&db_name).await?;

    // Create test entities
    let entity1 = create_test_entity("entity1", EntityType::Function);
    let entity2 = create_test_entity("entity2", EntityType::Function);

    neo4j_client
        .batch_create_nodes(&vec![entity1.clone(), entity2.clone()])
        .await?;

    // Create same relationship twice
    let relationships = vec![
        (
            entity1.entity_id.clone(),
            entity2.entity_id.clone(),
            "CALLS".to_string(),
        ),
        (
            entity1.entity_id.clone(),
            entity2.entity_id.clone(),
            "CALLS".to_string(),
        ),
    ];

    // Should succeed without error (MERGE handles duplicates)
    let result = neo4j_client
        .batch_create_relationships(&relationships)
        .await;
    assert!(result.is_ok());

    // Verify only one relationship exists
    let verify_query =
        "MATCH (a {id: $from_id})-[r:CALLS]->(b {id: $to_id}) RETURN count(r) as count".to_string();
    let mut query_result = neo4j_client
        .graph()
        .execute(
            neo4rs::Query::new(verify_query)
                .param("from_id", entity1.entity_id.as_str())
                .param("to_id", entity2.entity_id.as_str()),
        )
        .await?;

    if let Some(row) = query_result.next().await? {
        let count: i64 = row.get("count")?;
        assert_eq!(
            count, 1,
            "Should have exactly one relationship due to MERGE"
        );
    }

    // Cleanup
    neo4j_client.drop_database(&db_name).await?;

    Ok(())
}

/// Test handling of missing entities during relationship creation
#[tokio::test]
#[ignore] // Requires Neo4j to be running
async fn test_missing_entity_in_relationship() -> Result<()> {
    let config = create_storage_config(6334, 6333, 5432, "test_db");
    let neo4j_client = Neo4jClient::new(&config).await?;

    // Create test database
    let db_name = format!("test_{}", Uuid::new_v4().simple());
    neo4j_client.create_database(&db_name).await?;
    neo4j_client.use_database(&db_name).await?;

    // Create only one entity
    let entity1 = create_test_entity("entity1", EntityType::Function);
    neo4j_client.create_entity_node(&entity1).await?;

    // Try to create relationship to non-existent entity
    let relationships = vec![(
        entity1.entity_id.clone(),
        "non_existent_id".to_string(),
        "CALLS".to_string(),
    )];

    // Batch creation should complete without error
    // (MATCH will simply not find the missing entity, no relationship created)
    let result = neo4j_client
        .batch_create_relationships(&relationships)
        .await;
    assert!(result.is_ok());

    // Verify no relationship was created
    let verify_query =
        "MATCH (a {id: $from_id})-[r:CALLS]->() RETURN count(r) as count".to_string();
    let mut query_result = neo4j_client
        .graph()
        .execute(neo4rs::Query::new(verify_query).param("from_id", entity1.entity_id.as_str()))
        .await?;

    if let Some(row) = query_result.next().await? {
        let count: i64 = row.get("count")?;
        assert_eq!(
            count, 0,
            "No relationship should be created for missing entity"
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
