use anyhow::{anyhow, Context, Result};
use codesearch_core::{CodeEntity, EntityType, StorageConfig};
use neo4rs::{Graph, Query};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

/// Allowed relationship types for Neo4j (prevents Cypher injection)
pub const ALLOWED_RELATIONSHIP_TYPES: &[&str] = &[
    "CONTAINS",
    "IMPLEMENTS",
    "ASSOCIATES",
    "EXTENDS_INTERFACE",
    "INHERITS_FROM",
    "USES",
    "CALLS",
    "IMPORTS",
];

/// Neo4j client for graph database operations
pub struct Neo4jClient {
    graph: Arc<Graph>,
    current_database: Arc<RwLock<Option<String>>>,
}

impl Neo4jClient {
    /// Connect to Neo4j server with the provided configuration
    ///
    /// # Arguments
    /// * `config` - Storage configuration containing Neo4j connection details
    ///
    /// # Returns
    /// * `Result<Self>` - Connected Neo4j client or error
    ///
    /// # Example
    /// ```no_run
    /// use codesearch_storage::Neo4jClient;
    /// use codesearch_core::StorageConfig;
    ///
    /// # async fn example(config: &StorageConfig) -> anyhow::Result<()> {
    /// let client = Neo4jClient::new(config).await?;
    /// # Ok(())
    /// # }
    /// ```
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

    /// Create a single entity node in the current Neo4j database
    ///
    /// # Arguments
    /// * `entity` - Code entity to create as a node
    ///
    /// # Returns
    /// * `Result<i64>` - Internal Neo4j node ID or error
    ///
    /// # Errors
    /// * Returns error if no database is selected (call `use_database()` first)
    /// * Returns error if node creation fails
    ///
    /// # Example
    /// ```no_run
    /// # use codesearch_storage::Neo4jClient;
    /// # use codesearch_core::CodeEntity;
    /// # async fn example(client: &Neo4jClient, entity: &CodeEntity) -> anyhow::Result<()> {
    /// client.use_database("my_db").await?;
    /// let node_id = client.create_entity_node(entity).await?;
    /// # Ok(())
    /// # }
    /// ```
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

    /// Batch create nodes using UNWIND for better performance
    ///
    /// Creates multiple nodes in a single query per entity type, significantly reducing
    /// network overhead compared to individual inserts.
    ///
    /// # Performance
    /// For N entities of M types: M queries instead of N queries
    /// Example: 1,000 entities of 5 types = 5 queries instead of 1,000
    pub async fn batch_create_nodes(&self, entities: &[CodeEntity]) -> Result<Vec<i64>> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }

        let _db = self.get_current_database().await?;

        // Group entities by type (needed for label assignment)
        let mut entities_by_type: Vec<(EntityType, Vec<&CodeEntity>)> = Vec::new();
        for entity in entities {
            if let Some((_, group)) = entities_by_type
                .iter_mut()
                .find(|(t, _)| *t == entity.entity_type)
            {
                group.push(entity);
            } else {
                entities_by_type.push((entity.entity_type, vec![entity]));
            }
        }

        let mut all_node_ids = Vec::new();

        // Process each entity type group with a single UNWIND query
        for (entity_type, group_entities) in entities_by_type {
            let labels = self.get_entity_labels(&entity_type);
            let label_str = labels.join(":");

            // Build list of entity maps for UNWIND
            let entity_maps: Vec<std::collections::HashMap<String, neo4rs::BoltType>> =
                group_entities
                    .iter()
                    .map(|e| {
                        let mut map = std::collections::HashMap::new();
                        map.insert("id".to_string(), e.entity_id.clone().into());
                        map.insert(
                            "repository_id".to_string(),
                            e.repository_id.to_string().into(),
                        );
                        map.insert(
                            "qualified_name".to_string(),
                            e.qualified_name.clone().into(),
                        );
                        map.insert("name".to_string(), e.name.clone().into());
                        map.insert("language".to_string(), e.language.to_string().into());
                        map.insert("visibility".to_string(), e.visibility.to_string().into());
                        map.insert("is_async".to_string(), e.metadata.is_async.into());
                        map.insert("is_generic".to_string(), e.metadata.is_generic.into());
                        map.insert("is_static".to_string(), e.metadata.is_static.into());
                        map.insert("is_abstract".to_string(), e.metadata.is_abstract.into());
                        map.insert("is_const".to_string(), e.metadata.is_const.into());
                        map
                    })
                    .collect();

            // UNWIND query: processes entire list in single network call
            let query_str = format!(
                "UNWIND $entities AS entity
                 MERGE (n:{label_str} {{id: entity.id}})
                 SET n.repository_id = entity.repository_id,
                     n.qualified_name = entity.qualified_name,
                     n.name = entity.name,
                     n.language = entity.language,
                     n.visibility = entity.visibility,
                     n.is_async = entity.is_async,
                     n.is_generic = entity.is_generic,
                     n.is_static = entity.is_static,
                     n.is_abstract = entity.is_abstract,
                     n.is_const = entity.is_const
                 RETURN id(n) as node_id"
            );

            // Vec<HashMap<String, BoltType>> automatically converts to BoltType
            let query = Query::new(query_str).param("entities", entity_maps);

            let mut result = self.graph.execute(query).await?;

            while let Some(row) = result.next().await? {
                let node_id: i64 = row.get("node_id")?;
                all_node_ids.push(node_id);
            }
        }

        Ok(all_node_ids)
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
    ///
    /// Creates a new Neo4j database for the repository if one doesn't exist, and
    /// stores the database name in PostgreSQL for future lookups.
    ///
    /// # Arguments
    /// * `repository_id` - UUID of the repository
    /// * `postgres_client` - PostgreSQL client for storing database name mapping
    ///
    /// # Returns
    /// * `Result<String>` - Database name (format: `codesearch_{uuid}`)
    ///
    /// # Database Naming
    /// Database names follow the format `codesearch_{repository_uuid}` where uuid
    /// is the simple (no hyphens) representation of the repository UUID.
    ///
    /// # Example
    /// ```no_run
    /// # use codesearch_storage::Neo4jClient;
    /// # use uuid::Uuid;
    /// # async fn example(
    /// #     client: &Neo4jClient,
    /// #     postgres: &dyn codesearch_storage::PostgresClientTrait
    /// # ) -> anyhow::Result<()> {
    /// let repo_id = Uuid::new_v4();
    /// let db_name = client.ensure_repository_database(repo_id, postgres).await?;
    /// client.use_database(&db_name).await?;
    /// # Ok(())
    /// # }
    /// ```
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

    /// Create a relationship between two entities with Cypher injection protection
    ///
    /// # Arguments
    /// * `from_entity_id` - Source entity ID
    /// * `to_entity_id` - Target entity ID
    /// * `relationship_type` - Type of relationship (must be in allowed list)
    /// * `properties` - Optional properties to attach to the relationship
    ///
    /// # Returns
    /// * `Result<()>` - Success or error
    ///
    /// # Errors
    /// * Returns error if `relationship_type` is not in the allowed list (Cypher injection protection)
    /// * Returns error if no database is selected
    /// * Returns error if relationship creation fails
    ///
    /// # Allowed Relationship Types
    /// * CONTAINS, IMPLEMENTS, ASSOCIATES, EXTENDS_INTERFACE, INHERITS_FROM, USES, CALLS, IMPORTS
    ///
    /// # Example
    /// ```no_run
    /// # use codesearch_storage::Neo4jClient;
    /// # use std::collections::HashMap;
    /// # async fn example(client: &Neo4jClient) -> anyhow::Result<()> {
    /// client.use_database("my_db").await?;
    /// client.create_relationship(
    ///     "entity1",
    ///     "entity2",
    ///     "CALLS",
    ///     &HashMap::new()
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_relationship(
        &self,
        from_entity_id: &str,
        to_entity_id: &str,
        relationship_type: &str,
        properties: &std::collections::HashMap<String, String>,
    ) -> Result<()> {
        let _db = self.get_current_database().await?;

        // Validate relationship type to prevent Cypher injection
        if !ALLOWED_RELATIONSHIP_TYPES.contains(&relationship_type) {
            return Err(anyhow!(
                "Invalid relationship type '{relationship_type}'. Allowed types: {ALLOWED_RELATIONSHIP_TYPES:?}"
            ));
        }

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

    /// Batch create relationships using UNWIND for better performance
    ///
    /// Creates multiple relationships in a single query per relationship type,
    /// significantly reducing network overhead compared to individual inserts.
    ///
    /// # Arguments
    /// * `relationships` - List of (from_id, to_id, rel_type) tuples
    ///
    /// # Performance
    /// For N relationships of M types: M queries instead of N queries
    /// Example: 10,000 relationships of 4 types = 4 queries instead of 10,000
    ///
    /// # Security
    /// All relationship types are validated against the allowlist to prevent Cypher injection
    pub async fn batch_create_relationships(
        &self,
        relationships: &[(String, String, String)], // (from_id, to_id, rel_type)
    ) -> Result<()> {
        if relationships.is_empty() {
            return Ok(());
        }

        let _db = self.get_current_database().await?;

        // Validate all relationship types first (fail fast)
        for (_, _, rel_type) in relationships {
            if !ALLOWED_RELATIONSHIP_TYPES.contains(&rel_type.as_str()) {
                return Err(anyhow!(
                    "Invalid relationship type '{rel_type}'. Allowed types: {ALLOWED_RELATIONSHIP_TYPES:?}"
                ));
            }
        }

        // Group by relationship type
        let mut rels_by_type: Vec<(&str, Vec<(&str, &str)>)> = Vec::new();
        for (from_id, to_id, rel_type) in relationships {
            if let Some((_, group)) = rels_by_type
                .iter_mut()
                .find(|(t, _)| *t == rel_type.as_str())
            {
                group.push((from_id.as_str(), to_id.as_str()));
            } else {
                rels_by_type.push((rel_type.as_str(), vec![(from_id.as_str(), to_id.as_str())]));
            }
        }

        // Process each relationship type group with a single UNWIND query
        for (rel_type, pairs) in rels_by_type {
            // Build list of relationship maps for UNWIND
            let rel_maps: Vec<std::collections::HashMap<String, neo4rs::BoltType>> = pairs
                .iter()
                .map(|(from_id, to_id)| {
                    let mut map = std::collections::HashMap::new();
                    map.insert("from_id".to_string(), (*from_id).into());
                    map.insert("to_id".to_string(), (*to_id).into());
                    map
                })
                .collect();

            // UNWIND query: processes entire list in single network call
            let query_str = format!(
                "UNWIND $relationships AS rel
                 MATCH (from {{id: rel.from_id}}), (to {{id: rel.to_id}})
                 MERGE (from)-[:{rel_type}]->(to)"
            );

            // Vec<HashMap<String, BoltType>> automatically converts to BoltType
            let query = Query::new(query_str).param("relationships", rel_maps);

            self.graph.run(query).await?;
        }

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
