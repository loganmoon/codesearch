//! Mock Neo4j client for testing

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use anyhow::Result;
use async_trait::async_trait;
use codesearch_core::{CodeEntity, EntityType};
use neo4rs::Query;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::traits::Neo4jClientTrait;

/// In-memory node data
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Node {
    entity_id: String,
    entity: CodeEntity,
    internal_id: i64,
}

/// In-memory relationship data
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Relationship {
    from_id: String,
    to_id: String,
    rel_type: String,
}

/// In-memory unresolved relationship data
#[derive(Debug, Clone)]
struct UnresolvedRelationship {
    entity_id: String,
    rel_type: String,
    target_qualified_name: String,
}

#[derive(Debug, Default)]
struct MockData {
    databases: HashMap<String, bool>, // database_name -> exists
    current_database: Option<String>,
    nodes: HashMap<String, Node>,             // entity_id -> Node
    node_id_counter: i64,                     // Auto-increment for internal IDs
    relationships: Vec<Relationship>,         // List of relationships
    unresolved: Vec<UnresolvedRelationship>,  // List of unresolved relationships
    database_mappings: HashMap<Uuid, String>, // repository_id -> database_name
}

/// Mock Neo4j client for testing
pub struct MockNeo4jClient {
    data: Arc<Mutex<MockData>>,
}

impl MockNeo4jClient {
    /// Create a new mock client
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(MockData::default())),
        }
    }

    /// Get number of nodes stored
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn node_count(&self) -> usize {
        self.data.lock().unwrap().nodes.len()
    }

    /// Get number of relationships stored
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn relationship_count(&self) -> usize {
        self.data.lock().unwrap().relationships.len()
    }

    /// Get number of unresolved relationships stored
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn unresolved_count(&self) -> usize {
        self.data.lock().unwrap().unresolved.len()
    }
}

impl Default for MockNeo4jClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Neo4jClientTrait for MockNeo4jClient {
    async fn create_database(&self, database_name: &str) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        data.databases.insert(database_name.to_string(), true);
        Ok(())
    }

    async fn drop_database(&self, database_name: &str) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        data.databases.remove(database_name);
        Ok(())
    }

    async fn use_database(&self, database_name: &str) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        data.current_database = Some(database_name.to_string());
        Ok(())
    }

    async fn ensure_repository_database(
        &self,
        repository_id: Uuid,
        _postgres_client: &dyn crate::postgres::PostgresClientTrait,
    ) -> Result<String> {
        let db_name = format!("codesearch_{}", repository_id.simple());
        self.create_database(&db_name).await?;

        let mut data = self.data.lock().unwrap();
        data.database_mappings
            .insert(repository_id, db_name.clone());

        Ok(db_name)
    }

    async fn create_entity_node(&self, entity: &CodeEntity) -> Result<i64> {
        let mut data = self.data.lock().unwrap();

        let node_id = data.node_id_counter;
        data.node_id_counter += 1;

        let node = Node {
            entity_id: entity.entity_id.clone(),
            entity: entity.clone(),
            internal_id: node_id,
        };

        data.nodes.insert(entity.entity_id.clone(), node);

        Ok(node_id)
    }

    async fn batch_create_nodes(&self, entities: &[CodeEntity]) -> Result<Vec<i64>> {
        let mut node_ids = Vec::new();
        for entity in entities {
            let node_id = self.create_entity_node(entity).await?;
            node_ids.push(node_id);
        }
        Ok(node_ids)
    }

    async fn delete_entity_node(&self, entity_id: &str) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        data.nodes.remove(entity_id);
        Ok(())
    }

    async fn node_exists(&self, entity_id: &str) -> Result<bool> {
        let data = self.data.lock().unwrap();
        Ok(data.nodes.contains_key(entity_id))
    }

    async fn lookup_entity_by_name(
        &self,
        name: &str,
        entity_type: EntityType,
    ) -> Result<Option<String>> {
        let data = self.data.lock().unwrap();
        for node in data.nodes.values() {
            if node.entity.name == name && node.entity.entity_type == entity_type {
                return Ok(Some(node.entity_id.clone()));
            }
        }
        Ok(None)
    }

    async fn create_entity_node_from_query(&self, _query: Query) -> Result<i64> {
        // For mock, just return a dummy node ID
        Ok(0)
    }

    async fn create_relationship(
        &self,
        from_entity_id: &str,
        to_entity_id: &str,
        relationship_type: &str,
        _properties: &HashMap<String, String>,
    ) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        data.relationships.push(Relationship {
            from_id: from_entity_id.to_string(),
            to_id: to_entity_id.to_string(),
            rel_type: relationship_type.to_string(),
        });
        Ok(())
    }

    async fn batch_create_relationships(
        &self,
        relationships: &[(String, String, String)],
    ) -> Result<()> {
        for (from_id, to_id, rel_type) in relationships {
            self.create_relationship(from_id, to_id, rel_type, &HashMap::new())
                .await?;
        }
        Ok(())
    }

    async fn store_unresolved_relationship(
        &self,
        entity_id: &str,
        relationship_type: &str,
        target_qualified_name: &str,
    ) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        data.unresolved.push(UnresolvedRelationship {
            entity_id: entity_id.to_string(),
            rel_type: relationship_type.to_string(),
            target_qualified_name: target_qualified_name.to_string(),
        });
        Ok(())
    }

    async fn find_unresolved_contains_nodes(&self) -> Result<Vec<(String, String)>> {
        let data = self.data.lock().unwrap();
        let mut result = Vec::new();
        for unresolved in &data.unresolved {
            if unresolved.rel_type == "CONTAINS" {
                result.push((
                    unresolved.entity_id.clone(),
                    unresolved.target_qualified_name.clone(),
                ));
            }
        }
        Ok(result)
    }

    async fn resolve_contains_relationships_batch(
        &self,
        unresolved_nodes: &[(String, String)],
    ) -> Result<usize> {
        let mut data = self.data.lock().unwrap();
        let mut resolved_count = 0;

        for (child_id, parent_qname) in unresolved_nodes {
            // Find parent by qualified name and clone the entity_id to avoid borrow checker issues
            let parent_entity_id = data
                .nodes
                .values()
                .find(|n| n.entity.qualified_name == *parent_qname)
                .map(|parent| parent.entity_id.clone());

            if let Some(parent_id) = parent_entity_id {
                // Create relationship
                data.relationships.push(Relationship {
                    from_id: parent_id,
                    to_id: child_id.clone(),
                    rel_type: "CONTAINS".to_string(),
                });

                // Remove from unresolved
                data.unresolved.retain(|u| {
                    !(u.entity_id == *child_id
                        && u.rel_type == "CONTAINS"
                        && u.target_qualified_name == *parent_qname)
                });

                resolved_count += 1;
            }
        }

        Ok(resolved_count)
    }

    async fn run_query_with_params(
        &self,
        _query_str: &str,
        _params: &[(&str, String)],
    ) -> Result<()> {
        // Mock implementation - just return Ok
        Ok(())
    }

    // Graph query methods - simplified mock implementations

    async fn find_functions_in_module(
        &self,
        _postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        _repository_id: Uuid,
        _module_qualified_name: &str,
    ) -> Result<Vec<String>> {
        // Mock implementation
        Ok(vec![])
    }

    async fn find_trait_implementations(
        &self,
        _postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        _repository_id: Uuid,
        _trait_name: &str,
    ) -> Result<Vec<String>> {
        // Mock implementation
        Ok(vec![])
    }

    async fn find_class_hierarchy(
        &self,
        _postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        _repository_id: Uuid,
        _class_name: &str,
    ) -> Result<Vec<Vec<String>>> {
        // Mock implementation
        Ok(vec![])
    }

    async fn find_function_callers(
        &self,
        _postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        _repository_id: Uuid,
        _function_qualified_name: &str,
        _max_depth: usize,
    ) -> Result<Vec<(String, usize)>> {
        // Mock implementation
        Ok(vec![])
    }

    async fn find_unused_functions(
        &self,
        _postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        _repository_id: Uuid,
    ) -> Result<Vec<String>> {
        // Mock implementation
        Ok(vec![])
    }

    async fn find_module_dependencies(
        &self,
        _postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        _repository_id: Uuid,
        _module_qualified_name: &str,
    ) -> Result<Vec<String>> {
        // Mock implementation
        Ok(vec![])
    }

    async fn find_circular_dependencies(
        &self,
        _postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        _repository_id: Uuid,
    ) -> Result<Vec<Vec<String>>> {
        // Mock implementation
        Ok(vec![])
    }
}
