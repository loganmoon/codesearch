use anyhow::{anyhow, Context, Result};
use codesearch_core::{CodeEntity, EntityType, StorageConfig};
use neo4rs::{Graph, Query};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

/// Neo4j client for graph database operations
pub struct Neo4jClient {
    graph: Arc<Graph>,
    current_database: Arc<RwLock<Option<String>>>,
}

impl Neo4jClient {
    /// Connect to Neo4j server
    pub async fn new(config: &StorageConfig) -> Result<Self> {
        let uri = format!("bolt://{}:{}", config.neo4j_host, config.neo4j_bolt_port);

        info!("Connecting to Neo4j at {}", uri);

        let graph = Graph::new(&uri, &config.neo4j_user, &config.neo4j_password)
            .await
            .context("Failed to connect to Neo4j")?;

        Ok(Self {
            graph: Arc::new(graph),
            current_database: Arc::new(RwLock::new(None)),
        })
    }

    /// Create a new database for a repository
    pub async fn create_database(&self, database_name: &str) -> Result<()> {
        info!("Creating Neo4j database: {}", database_name);

        let query = Query::new(format!("CREATE DATABASE `{database_name}` IF NOT EXISTS"));

        self.graph
            .run(query)
            .await
            .context("Failed to create database")?;

        Ok(())
    }

    /// Drop a database
    pub async fn drop_database(&self, database_name: &str) -> Result<()> {
        info!("Dropping Neo4j database: {}", database_name);

        let query = Query::new(format!("DROP DATABASE `{database_name}` IF EXISTS"));

        self.graph
            .run(query)
            .await
            .context("Failed to drop database")?;

        Ok(())
    }

    /// Switch to a specific database
    pub async fn use_database(&self, database_name: &str) -> Result<()> {
        let mut current = self.current_database.write().await;
        *current = Some(database_name.to_string());
        debug!("Switched to database: {}", database_name);
        Ok(())
    }

    /// Get the current database name
    async fn get_current_database(&self) -> Result<String> {
        let current = self.current_database.read().await;
        current
            .clone()
            .ok_or_else(|| anyhow!("No database selected. Call use_database() first"))
    }

    /// Get a reference to the underlying Graph for direct query execution
    pub fn graph(&self) -> &Arc<Graph> {
        &self.graph
    }

    /// Create a single entity node
    pub async fn create_entity_node(&self, entity: &CodeEntity) -> Result<i64> {
        let _db = self.get_current_database().await?;

        let labels = self.get_entity_labels(&entity.entity_type);
        let label_str = labels.join(":");

        let query_str = format!(
            "MERGE (n:{label_str} {{id: $id}})
             SET n.repository_id = $repository_id,
                 n.qualified_name = $qualified_name,
                 n.name = $name,
                 n.language = $language,
                 n.visibility = $visibility,
                 n.is_async = $is_async,
                 n.is_generic = $is_generic,
                 n.is_static = $is_static,
                 n.is_abstract = $is_abstract,
                 n.is_const = $is_const
             RETURN id(n)"
        );

        let query = Query::new(query_str)
            .param("id", entity.entity_id.clone())
            .param("repository_id", entity.repository_id.to_string())
            .param("qualified_name", entity.qualified_name.clone())
            .param("name", entity.name.clone())
            .param("language", entity.language.to_string())
            .param("visibility", entity.visibility.to_string())
            .param("is_async", entity.metadata.is_async)
            .param("is_generic", entity.metadata.is_generic)
            .param("is_static", entity.metadata.is_static)
            .param("is_abstract", entity.metadata.is_abstract)
            .param("is_const", entity.metadata.is_const);

        let mut result = self.graph.execute(query).await?;

        if let Some(row) = result.next().await? {
            let node_id: i64 = row.get("id(n)")?;
            Ok(node_id)
        } else {
            Err(anyhow!("Failed to get node ID after creation"))
        }
    }

    /// Batch create nodes
    pub async fn batch_create_nodes(&self, entities: &[CodeEntity]) -> Result<Vec<i64>> {
        let mut node_ids = Vec::new();

        for entity in entities {
            let node_id = self.create_entity_node(entity).await?;
            node_ids.push(node_id);
        }

        Ok(node_ids)
    }

    /// Delete a node by entity_id
    pub async fn delete_entity_node(&self, entity_id: &str) -> Result<()> {
        let _db = self.get_current_database().await?;

        let query =
            Query::new("MATCH (n {id: $id}) DETACH DELETE n".to_string()).param("id", entity_id);

        self.graph
            .run(query)
            .await
            .context("Failed to delete node")?;

        Ok(())
    }

    /// Check if a node exists
    pub async fn node_exists(&self, entity_id: &str) -> Result<bool> {
        let _db = self.get_current_database().await?;

        let query = Query::new("MATCH (n {id: $id}) RETURN count(n) as count".to_string())
            .param("id", entity_id);

        let mut result = self.graph.execute(query).await?;

        if let Some(row) = result.next().await? {
            let count: i64 = row.get("count")?;
            Ok(count > 0)
        } else {
            Ok(false)
        }
    }

    /// Look up an entity by name and type
    pub async fn lookup_entity_by_name(
        &self,
        name: &str,
        entity_type: EntityType,
    ) -> Result<Option<String>> {
        let _db = self.get_current_database().await?;

        let labels = self.get_entity_labels(&entity_type);
        let label_str = labels.join(":");

        let query_str =
            format!("MATCH (n:{label_str} {{name: $name}}) RETURN n.id as entity_id LIMIT 1");

        let query = Query::new(query_str).param("name", name);

        let mut result = self.graph.execute(query).await?;

        if let Some(row) = result.next().await? {
            let entity_id: String = row.get("entity_id")?;
            Ok(Some(entity_id))
        } else {
            Ok(None)
        }
    }

    /// Ensure a repository database exists and return its name
    pub async fn ensure_repository_database(
        &self,
        repository_id: Uuid,
        postgres_client: &dyn crate::postgres::PostgresClientTrait,
    ) -> Result<String> {
        // Check if database name exists in PostgreSQL
        let existing_name = postgres_client
            .get_neo4j_database_name(repository_id)
            .await?;

        if let Some(db_name) = existing_name {
            // Database already tracked, ensure it exists in Neo4j
            self.create_database(&db_name).await?;
            self.create_indexes(&db_name).await?;
            return Ok(db_name);
        }

        // Generate new database name
        let db_name = format!("codesearch_{}", repository_id.simple());

        // Create database in Neo4j
        self.create_database(&db_name).await?;

        // Create indexes
        self.create_indexes(&db_name).await?;

        // Store in PostgreSQL
        postgres_client
            .set_neo4j_database_name(repository_id, &db_name)
            .await?;

        Ok(db_name)
    }

    /// Create indexes for a database
    async fn create_indexes(&self, database_name: &str) -> Result<()> {
        self.use_database(database_name).await?;

        info!("Creating indexes for database: {}", database_name);

        // Core entity lookup
        self.run_query("CREATE INDEX entity_id_idx IF NOT EXISTS FOR (n) ON (n.id)")
            .await?;
        self.run_query("CREATE INDEX repository_id_idx IF NOT EXISTS FOR (n) ON (n.repository_id)")
            .await?;
        self.run_query(
            "CREATE INDEX qualified_name_idx IF NOT EXISTS FOR (n) ON (n.qualified_name)",
        )
        .await?;

        // Filtering
        self.run_query("CREATE INDEX language_idx IF NOT EXISTS FOR (n) ON (n.language)")
            .await?;
        self.run_query("CREATE INDEX visibility_idx IF NOT EXISTS FOR (n) ON (n.visibility)")
            .await?;

        // Composite index for repository queries
        self.run_query(
            "CREATE INDEX repo_entity_idx IF NOT EXISTS FOR (n) ON (n.repository_id, n.id)",
        )
        .await?;

        Ok(())
    }

    /// Run a simple query without parameters
    async fn run_query(&self, query_str: &str) -> Result<()> {
        let query = Query::new(query_str.to_string());
        self.graph
            .run(query)
            .await
            .context(format!("Failed to run query: {query_str}"))?;
        Ok(())
    }

    /// Create a node from a custom query and return the internal node ID
    pub async fn create_entity_node_from_query(&self, query: Query) -> Result<i64> {
        let _db = self.get_current_database().await?;

        let mut result = self.graph.execute(query).await?;

        if let Some(row) = result.next().await? {
            let node_id: i64 = row.get("id(n)")?;
            Ok(node_id)
        } else {
            Err(anyhow!("Failed to get node ID after creation"))
        }
    }

    /// Run a query with named parameters
    pub async fn run_query_with_params(
        &self,
        query_str: &str,
        params: &[(&str, String)],
    ) -> Result<()> {
        let _db = self.get_current_database().await?;

        let mut query = Query::new(query_str.to_string());
        for (key, value) in params {
            query = query.param(key, value.clone());
        }

        self.graph
            .run(query)
            .await
            .context(format!("Failed to run query: {query_str}"))?;
        Ok(())
    }

    /// Find all nodes with unresolved CONTAINS relationships
    pub async fn find_unresolved_contains_nodes(&self) -> Result<Vec<(String, String)>> {
        let _db = self.get_current_database().await?;

        let query = Query::new(
            "MATCH (child)
             WHERE child.unresolved_contains_parent IS NOT NULL
             RETURN child.id AS child_id, child.unresolved_contains_parent AS parent_qname"
                .to_string(),
        );

        let mut result = self.graph.execute(query).await?;

        let mut nodes = Vec::new();
        while let Some(row) = result.next().await? {
            let child_id: String = row.get("child_id")?;
            let parent_qname: String = row.get("parent_qname")?;
            nodes.push((child_id, parent_qname));
        }

        Ok(nodes)
    }

    /// Resolve a CONTAINS relationship by creating the edge and removing the unresolved property
    /// Returns Ok(true) if successful, Ok(false) if parent not found
    pub async fn resolve_contains_relationship(
        &self,
        child_id: &str,
        parent_qname: &str,
    ) -> Result<bool> {
        let _db = self.get_current_database().await?;

        // Look up parent by qualified_name
        let lookup_query = Query::new(
            "MATCH (parent {qualified_name: $qname})
             RETURN parent.id AS parent_id"
                .to_string(),
        )
        .param("qname", parent_qname);

        let mut lookup_result = self.graph.execute(lookup_query).await?;

        let parent_id = if let Some(row) = lookup_result.next().await? {
            let id: String = row.get("parent_id")?;
            id
        } else {
            // Parent not found
            return Ok(false);
        };

        // Create CONTAINS edge
        let create_edge_query = Query::new(
            "MATCH (parent {id: $parent_id}), (child {id: $child_id})
             MERGE (parent)-[:CONTAINS]->(child)"
                .to_string(),
        )
        .param("parent_id", parent_id)
        .param("child_id", child_id);

        self.graph.run(create_edge_query).await?;

        // Remove unresolved property
        let cleanup_query = Query::new(
            "MATCH (child {id: $child_id})
             REMOVE child.unresolved_contains_parent"
                .to_string(),
        )
        .param("child_id", child_id);

        self.graph.run(cleanup_query).await?;

        Ok(true)
    }

    /// Create a relationship between two entities
    pub async fn create_relationship(
        &self,
        from_entity_id: &str,
        to_entity_id: &str,
        relationship_type: &str,
        properties: &std::collections::HashMap<String, String>,
    ) -> Result<()> {
        let _db = self.get_current_database().await?;

        // Build the relationship creation query
        let mut query = format!(
            "MATCH (from {{id: $from_id}}), (to {{id: $to_id}})
             MERGE (from)-[r:{relationship_type}]->(to)"
        );

        // Add property setters if there are properties
        if !properties.is_empty() {
            query.push_str(" SET ");
            let prop_setters: Vec<String> = properties
                .keys()
                .map(|key| format!("r.{key} = ${key}"))
                .collect();
            query.push_str(&prop_setters.join(", "));
        }

        let mut q = Query::new(query)
            .param("from_id", from_entity_id)
            .param("to_id", to_entity_id);

        // Add property parameters
        for (key, value) in properties {
            q = q.param(key.as_str(), value.as_str());
        }

        self.graph.run(q).await?;

        Ok(())
    }

    /// Get Neo4j labels for an entity type
    fn get_entity_labels(&self, entity_type: &EntityType) -> Vec<String> {
        match entity_type {
            EntityType::Function => vec!["Function".to_string()],
            EntityType::Method => vec!["Method".to_string()],
            EntityType::Class => vec!["Class".to_string()],
            EntityType::Struct => vec!["Struct".to_string(), "Class".to_string()],
            EntityType::Interface => vec!["Interface".to_string()],
            EntityType::Trait => vec!["Trait".to_string(), "Interface".to_string()],
            EntityType::Enum => vec!["Enum".to_string()],
            EntityType::Module => vec!["Module".to_string()],
            EntityType::Package => vec!["Package".to_string()],
            EntityType::Constant => vec!["Constant".to_string()],
            EntityType::Variable => vec!["Variable".to_string()],
            EntityType::TypeAlias => vec!["TypeAlias".to_string()],
            EntityType::Macro => vec!["Macro".to_string()],
            EntityType::Impl => vec!["ImplBlock".to_string()],
        }
    }
}
