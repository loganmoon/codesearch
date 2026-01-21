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

#[derive(Debug, Default)]
struct MockData {
    databases: HashMap<String, bool>, // database_name -> exists
    current_database: Option<String>,
    nodes: HashMap<String, Node>,             // entity_id -> Node
    node_id_counter: i64,                     // Auto-increment for internal IDs
    relationships: Vec<Relationship>,         // List of relationships
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

    async fn delete_repository_data(&self, repository_id: Uuid) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        // Remove nodes belonging to this repository
        let repo_id_str = repository_id.to_string();
        data.nodes
            .retain(|_, node| node.entity.repository_id != repo_id_str);
        // Remove database mapping
        data.database_mappings.remove(&repository_id);
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

    async fn batch_create_external_nodes(
        &self,
        _repository_id: &str,
        _external_refs: &[(String, String, Option<String>)],
    ) -> Result<()> {
        // Mock implementation - external nodes are not tracked in mock
        Ok(())
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

    async fn find_function_callees(
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
