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

/// Validates that a property key is safe to use in Cypher queries
///
/// Property keys must consist only of ASCII alphanumeric characters and underscores
/// to prevent Cypher injection attacks.
///
/// # Arguments
/// * `key` - The property key to validate
///
/// # Returns
/// * `Result<()>` - Ok if valid, Err with descriptive message if invalid
fn validate_property_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(anyhow!("Property key cannot be empty"));
    }

    if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(anyhow!(
            "Invalid property key '{key}'. Keys must contain only ASCII alphanumeric characters and underscores"
        ));
    }

    Ok(())
}

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
    ///
    /// Internal use only. External callers should use validated API methods.
    #[allow(dead_code)]
    pub(crate) fn graph(&self) -> &Arc<Graph> {
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

    /// Batch resolve CONTAINS relationships using UNWIND for performance
    ///
    /// This method resolves multiple unresolved CONTAINS relationships in just 2 queries
    /// instead of 3N queries, providing significant performance improvement for large repositories.
    ///
    /// # Arguments
    /// * `unresolved_nodes` - Vec of (child_id, parent_qualified_name) pairs
    ///
    /// # Returns
    /// * `Result<usize>` - Number of relationships successfully created
    pub async fn resolve_contains_relationships_batch(
        &self,
        unresolved_nodes: &[(String, String)],
    ) -> Result<usize> {
        let _db = self.get_current_database().await?;

        if unresolved_nodes.is_empty() {
            return Ok(0);
        }

        // Convert to format Neo4j expects: Vec<HashMap<String, String>>
        let nodes_data: Vec<std::collections::HashMap<String, String>> = unresolved_nodes
            .iter()
            .map(|(child_id, parent_qname)| {
                let mut map = std::collections::HashMap::new();
                map.insert("child_id".to_string(), child_id.clone());
                map.insert("parent_qname".to_string(), parent_qname.clone());
                map
            })
            .collect();

        // Query 1: Batch lookup parents, create relationships, and cleanup in one query
        // Using UNWIND for maximum efficiency
        let batch_query = Query::new(
            "UNWIND $nodes AS node
             MATCH (parent {qualified_name: node.parent_qname})
             MATCH (child {id: node.child_id})
             MERGE (parent)-[:CONTAINS]->(child)
             REMOVE child.unresolved_contains_parent
             RETURN count(*) AS resolved_count"
                .to_string(),
        )
        .param("nodes", nodes_data);

        let mut result = self.graph.execute(batch_query).await?;

        let resolved_count = if let Some(row) = result.next().await? {
            let count: i64 = row.get("resolved_count")?;
            count as usize
        } else {
            0
        };

        Ok(resolved_count)
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

        // Validate property keys to prevent Cypher injection
        for key in properties.keys() {
            validate_property_key(key)?;
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

    /// Store an unresolved relationship as a node property for later resolution
    ///
    /// When a relationship target doesn't exist yet, we store the relationship
    /// information as a temporary node property. Later resolution processes will
    /// query for these properties and create actual relationship edges.
    ///
    /// # Arguments
    /// * `entity_id` - ID of the entity with unresolved relationship
    /// * `relationship_type` - Type of relationship (must be in allowed list)
    /// * `target_qualified_name` - Qualified name of the target entity
    ///
    /// # Property Naming
    /// Property is stored as `unresolved_{rel_type}_parent` (lowercase)
    ///
    /// # Security
    /// - Validates `relationship_type` against `ALLOWED_RELATIONSHIP_TYPES`
    /// - Uses parameterized queries for all values
    /// - Property name derived from validated constant (safe from injection)
    pub async fn store_unresolved_relationship(
        &self,
        entity_id: &str,
        relationship_type: &str,
        target_qualified_name: &str,
    ) -> Result<()> {
        let _db = self.get_current_database().await?;

        // Validate relationship type (Cypher injection protection)
        if !ALLOWED_RELATIONSHIP_TYPES.contains(&relationship_type) {
            return Err(anyhow!(
                "Invalid relationship type '{relationship_type}'. Allowed types: {ALLOWED_RELATIONSHIP_TYPES:?}"
            ));
        }

        let property_name = format!("unresolved_{}_parent", relationship_type.to_lowercase());

        // Use parameterized query for values, format string for property name
        // (property name is derived from validated relationship_type constant)
        let query_str = format!(
            "MATCH (n {{id: $entity_id}})
             SET n.`{property_name}` = $target_qname"
        );

        let query = Query::new(query_str)
            .param("entity_id", entity_id)
            .param("target_qname", target_qualified_name);

        self.graph
            .run(query)
            .await
            .context("Failed to store unresolved relationship")?;

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
    fn get_entity_labels(&self, entity_type: &EntityType) -> &'static [&'static str] {
        match entity_type {
            EntityType::Function => &["Function"],
            EntityType::Method => &["Method"],
            EntityType::Class => &["Class"],
            EntityType::Struct => &["Struct", "Class"],
            EntityType::Interface => &["Interface"],
            EntityType::Trait => &["Trait", "Interface"],
            EntityType::Enum => &["Enum"],
            EntityType::Module => &["Module"],
            EntityType::Package => &["Package"],
            EntityType::Constant => &["Constant"],
            EntityType::Variable => &["Variable"],
            EntityType::TypeAlias => &["TypeAlias"],
            EntityType::Macro => &["Macro"],
            EntityType::Impl => &["ImplBlock"],
        }
    }

    // ===== Graph Query Methods =====

    /// Find all functions contained in a module
    pub async fn find_functions_in_module(
        &self,
        postgres: &std::sync::Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        module_qualified_name: &str,
    ) -> Result<Vec<String>> {
        let db_name = self
            .ensure_repository_database(repository_id, postgres.as_ref())
            .await?;
        self.use_database(&db_name).await?;

        let query = Query::new(
            "MATCH (m:Module {qualified_name: $qname})-[:CONTAINS*]->(f:Function)
             RETURN f.qualified_name AS name"
                .to_string(),
        )
        .param("qname", module_qualified_name);

        let mut result = self.graph.execute(query).await?;

        let mut names = Vec::new();
        while let Ok(Some(row)) = result.next().await {
            if let Ok(name) = row.get::<String>("name") {
                names.push(name);
            }
        }
        Ok(names)
    }

    /// Find all implementations of a trait
    pub async fn find_trait_implementations(
        &self,
        postgres: &std::sync::Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        trait_name: &str,
    ) -> Result<Vec<String>> {
        let db_name = self
            .ensure_repository_database(repository_id, postgres.as_ref())
            .await?;
        self.use_database(&db_name).await?;

        let query = Query::new(
            "MATCH (impl:ImplBlock)-[:IMPLEMENTS]->(trait:Interface {name: $trait_name})
             MATCH (impl)-[:ASSOCIATES]->(type)
             RETURN type.qualified_name AS name"
                .to_string(),
        )
        .param("trait_name", trait_name);

        let mut result = self.graph.execute(query).await?;

        let mut names = Vec::new();
        while let Ok(Some(row)) = result.next().await {
            if let Ok(name) = row.get::<String>("name") {
                names.push(name);
            }
        }
        Ok(names)
    }

    /// Find class inheritance hierarchy
    pub async fn find_class_hierarchy(
        &self,
        postgres: &std::sync::Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        class_name: &str,
    ) -> Result<Vec<Vec<String>>> {
        let db_name = self
            .ensure_repository_database(repository_id, postgres.as_ref())
            .await?;
        self.use_database(&db_name).await?;

        let query = Query::new(
            "MATCH path = (root:Class {name: $class_name})-[:INHERITS_FROM*]->(ancestor:Class)
             RETURN [node in nodes(path) | node.name] AS hierarchy"
                .to_string(),
        )
        .param("class_name", class_name);

        let mut result = self.graph.execute(query).await?;

        let mut hierarchies = Vec::new();
        while let Ok(Some(row)) = result.next().await {
            if let Ok(hierarchy) = row.get::<Vec<String>>("hierarchy") {
                hierarchies.push(hierarchy);
            }
        }
        Ok(hierarchies)
    }

    /// Find call graph (callers of a function)
    pub async fn find_function_callers(
        &self,
        postgres: &std::sync::Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        function_qualified_name: &str,
        max_depth: usize,
    ) -> Result<Vec<(String, usize)>> {
        let db_name = self
            .ensure_repository_database(repository_id, postgres.as_ref())
            .await?;
        self.use_database(&db_name).await?;

        let query_str = format!(
            "MATCH (target {{qualified_name: $qname}})
             MATCH path = (caller)-[:CALLS*1..{max_depth}]->(target)
             RETURN DISTINCT caller.qualified_name AS name, length(path) AS depth
             ORDER BY depth ASC"
        );

        let query = Query::new(query_str).param("qname", function_qualified_name);

        let mut result = self.graph.execute(query).await?;

        let mut callers = Vec::new();
        while let Ok(Some(row)) = result.next().await {
            if let (Ok(name), Ok(depth)) = (row.get::<String>("name"), row.get::<i64>("depth")) {
                callers.push((name, depth as usize));
            }
        }
        Ok(callers)
    }

    /// Find unused functions (no incoming calls, not public)
    pub async fn find_unused_functions(
        &self,
        postgres: &std::sync::Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<String>> {
        let db_name = self
            .ensure_repository_database(repository_id, postgres.as_ref())
            .await?;
        self.use_database(&db_name).await?;

        let query = Query::new(
            "MATCH (f:Function)
             WHERE f.visibility = 'private'
               AND NOT (:Function)-[:CALLS]->(f)
               AND NOT (:Method)-[:CALLS]->(f)
               AND NOT f.name IN ['main', 'test']
               AND NOT f.name STARTS WITH 'test_'
             RETURN f.qualified_name AS name"
                .to_string(),
        );

        let mut result = self.graph.execute(query).await?;

        let mut functions = Vec::new();
        while let Ok(Some(row)) = result.next().await {
            if let Ok(name) = row.get::<String>("name") {
                functions.push(name);
            }
        }
        Ok(functions)
    }

    /// Find module dependencies (imports)
    pub async fn find_module_dependencies(
        &self,
        postgres: &std::sync::Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        module_qualified_name: &str,
    ) -> Result<Vec<String>> {
        let db_name = self
            .ensure_repository_database(repository_id, postgres.as_ref())
            .await?;
        self.use_database(&db_name).await?;

        let query = Query::new(
            "MATCH (m:Module {qualified_name: $qname})-[:IMPORTS]->(imported:Module)
             RETURN imported.qualified_name AS name"
                .to_string(),
        )
        .param("qname", module_qualified_name);

        let mut result = self.graph.execute(query).await?;

        let mut deps = Vec::new();
        while let Ok(Some(row)) = result.next().await {
            if let Ok(name) = row.get::<String>("name") {
                deps.push(name);
            }
        }
        Ok(deps)
    }

    /// Find circular dependencies
    pub async fn find_circular_dependencies(
        &self,
        postgres: &std::sync::Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<Vec<String>>> {
        let db_name = self
            .ensure_repository_database(repository_id, postgres.as_ref())
            .await?;
        self.use_database(&db_name).await?;

        let query = Query::new(
            "MATCH path = (m1:Module)-[:IMPORTS*]->(m2:Module)-[:IMPORTS*]->(m1)
             WHERE m1 <> m2
             RETURN [node in nodes(path) | node.qualified_name] AS cycle,
                    length(path) AS length
             ORDER BY length
             LIMIT 100"
                .to_string(),
        );

        let mut result = self.graph.execute(query).await?;

        let mut cycles = Vec::new();
        while let Ok(Some(row)) = result.next().await {
            if let Ok(cycle) = row.get::<Vec<String>>("cycle") {
                cycles.push(cycle);
            }
        }
        Ok(cycles)
    }
}

// Implement Neo4jClientTrait for Neo4jClient
use super::traits::Neo4jClientTrait;
use async_trait::async_trait;

#[async_trait]
impl Neo4jClientTrait for Neo4jClient {
    async fn create_database(&self, database_name: &str) -> Result<()> {
        Self::create_database(self, database_name).await
    }

    async fn drop_database(&self, database_name: &str) -> Result<()> {
        Self::drop_database(self, database_name).await
    }

    async fn use_database(&self, database_name: &str) -> Result<()> {
        Self::use_database(self, database_name).await
    }

    async fn ensure_repository_database(
        &self,
        repository_id: Uuid,
        postgres_client: &dyn crate::postgres::PostgresClientTrait,
    ) -> Result<String> {
        Self::ensure_repository_database(self, repository_id, postgres_client).await
    }

    async fn create_entity_node(&self, entity: &CodeEntity) -> Result<i64> {
        Self::create_entity_node(self, entity).await
    }

    async fn batch_create_nodes(&self, entities: &[CodeEntity]) -> Result<Vec<i64>> {
        Self::batch_create_nodes(self, entities).await
    }

    async fn delete_entity_node(&self, entity_id: &str) -> Result<()> {
        Self::delete_entity_node(self, entity_id).await
    }

    async fn node_exists(&self, entity_id: &str) -> Result<bool> {
        Self::node_exists(self, entity_id).await
    }

    async fn lookup_entity_by_name(
        &self,
        name: &str,
        entity_type: EntityType,
    ) -> Result<Option<String>> {
        Self::lookup_entity_by_name(self, name, entity_type).await
    }

    async fn create_entity_node_from_query(&self, query: Query) -> Result<i64> {
        Self::create_entity_node_from_query(self, query).await
    }

    async fn create_relationship(
        &self,
        from_entity_id: &str,
        to_entity_id: &str,
        relationship_type: &str,
        properties: &std::collections::HashMap<String, String>,
    ) -> Result<()> {
        Self::create_relationship(
            self,
            from_entity_id,
            to_entity_id,
            relationship_type,
            properties,
        )
        .await
    }

    async fn batch_create_relationships(
        &self,
        relationships: &[(String, String, String)],
    ) -> Result<()> {
        Self::batch_create_relationships(self, relationships).await
    }

    async fn store_unresolved_relationship(
        &self,
        entity_id: &str,
        relationship_type: &str,
        target_qualified_name: &str,
    ) -> Result<()> {
        Self::store_unresolved_relationship(
            self,
            entity_id,
            relationship_type,
            target_qualified_name,
        )
        .await
    }

    async fn find_unresolved_contains_nodes(&self) -> Result<Vec<(String, String)>> {
        Self::find_unresolved_contains_nodes(self).await
    }

    async fn resolve_contains_relationships_batch(
        &self,
        unresolved_nodes: &[(String, String)],
    ) -> Result<usize> {
        Self::resolve_contains_relationships_batch(self, unresolved_nodes).await
    }

    async fn run_query_with_params(
        &self,
        query_str: &str,
        params: &[(&str, String)],
    ) -> Result<()> {
        Self::run_query_with_params(self, query_str, params).await
    }

    async fn find_functions_in_module(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        module_qualified_name: &str,
    ) -> Result<Vec<String>> {
        Self::find_functions_in_module(self, postgres, repository_id, module_qualified_name).await
    }

    async fn find_trait_implementations(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        trait_name: &str,
    ) -> Result<Vec<String>> {
        Self::find_trait_implementations(self, postgres, repository_id, trait_name).await
    }

    async fn find_class_hierarchy(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        class_name: &str,
    ) -> Result<Vec<Vec<String>>> {
        Self::find_class_hierarchy(self, postgres, repository_id, class_name).await
    }

    async fn find_function_callers(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        function_qualified_name: &str,
        max_depth: usize,
    ) -> Result<Vec<(String, usize)>> {
        Self::find_function_callers(
            self,
            postgres,
            repository_id,
            function_qualified_name,
            max_depth,
        )
        .await
    }

    async fn find_unused_functions(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<String>> {
        Self::find_unused_functions(self, postgres, repository_id).await
    }

    async fn find_module_dependencies(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        module_qualified_name: &str,
    ) -> Result<Vec<String>> {
        Self::find_module_dependencies(self, postgres, repository_id, module_qualified_name).await
    }

    async fn find_circular_dependencies(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<Vec<String>>> {
        Self::find_circular_dependencies(self, postgres, repository_id).await
    }
}
