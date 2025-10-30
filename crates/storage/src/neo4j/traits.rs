use anyhow::Result;
use async_trait::async_trait;
use codesearch_core::{CodeEntity, EntityType};
use neo4rs::Query;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Trait for Neo4j graph database operations
///
/// This trait provides a safe, validated API for code graph operations.
/// All implementations must enforce security constraints (relationship type validation,
/// property key validation) to prevent Cypher injection attacks.
#[async_trait]
pub trait Neo4jClientTrait: Send + Sync {
    // ===== Database Management =====

    /// Create a new database for a repository
    ///
    /// # Arguments
    /// * `database_name` - Name of the database to create
    ///
    /// # Returns
    /// * `Result<()>` - Success or error
    async fn create_database(&self, database_name: &str) -> Result<()>;

    /// Drop a database
    ///
    /// # Arguments
    /// * `database_name` - Name of the database to drop
    ///
    /// # Returns
    /// * `Result<()>` - Success or error
    async fn drop_database(&self, database_name: &str) -> Result<()>;

    /// Switch to a specific database for subsequent operations
    ///
    /// # Arguments
    /// * `database_name` - Name of the database to use
    ///
    /// # Returns
    /// * `Result<()>` - Success or error
    async fn use_database(&self, database_name: &str) -> Result<()>;

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
    async fn ensure_repository_database(
        &self,
        repository_id: Uuid,
        postgres_client: &dyn crate::postgres::PostgresClientTrait,
    ) -> Result<String>;

    // ===== Node Operations =====

    /// Create a single entity node in the current Neo4j database
    ///
    /// # Arguments
    /// * `entity` - Code entity to create as a node
    ///
    /// # Returns
    /// * `Result<i64>` - Internal Neo4j node ID or error
    async fn create_entity_node(&self, entity: &CodeEntity) -> Result<i64>;

    /// Batch create nodes using UNWIND for better performance
    ///
    /// Creates multiple nodes in a single query per entity type, significantly reducing
    /// network overhead compared to individual inserts.
    ///
    /// # Performance
    /// For N entities of M types: M queries instead of N queries
    /// Example: 1,000 entities of 5 types = 5 queries instead of 1,000
    ///
    /// # Arguments
    /// * `entities` - Slice of entities to create
    ///
    /// # Returns
    /// * `Result<Vec<i64>>` - Internal Neo4j node IDs
    async fn batch_create_nodes(&self, entities: &[CodeEntity]) -> Result<Vec<i64>>;

    /// Delete a node by entity_id
    ///
    /// # Arguments
    /// * `entity_id` - ID of the entity to delete
    ///
    /// # Returns
    /// * `Result<()>` - Success or error
    async fn delete_entity_node(&self, entity_id: &str) -> Result<()>;

    /// Check if a node exists
    ///
    /// # Arguments
    /// * `entity_id` - ID of the entity to check
    ///
    /// # Returns
    /// * `Result<bool>` - True if node exists, false otherwise
    async fn node_exists(&self, entity_id: &str) -> Result<bool>;

    /// Look up an entity by name and type
    ///
    /// # Arguments
    /// * `name` - Name of the entity
    /// * `entity_type` - Type of the entity
    ///
    /// # Returns
    /// * `Result<Option<String>>` - Entity ID if found, None otherwise
    async fn lookup_entity_by_name(
        &self,
        name: &str,
        entity_type: EntityType,
    ) -> Result<Option<String>>;

    /// Create a node from a custom query and return the internal node ID
    ///
    /// # Arguments
    /// * `query` - Neo4j query to execute
    ///
    /// # Returns
    /// * `Result<i64>` - Internal Neo4j node ID
    async fn create_entity_node_from_query(&self, query: Query) -> Result<i64>;

    // ===== Relationship Operations =====

    /// Create a relationship between two entities with Cypher injection protection
    ///
    /// # Arguments
    /// * `from_entity_id` - Source entity ID
    /// * `to_entity_id` - Target entity ID
    /// * `relationship_type` - Type of relationship (must be in allowed list)
    /// * `properties` - Optional properties to attach to the relationship
    ///
    /// # Security
    /// - Validates `relationship_type` against `ALLOWED_RELATIONSHIP_TYPES`
    /// - Validates property keys to prevent Cypher injection
    /// - Uses parameterized queries for all values
    ///
    /// # Allowed Relationship Types
    /// * CONTAINS, IMPLEMENTS, ASSOCIATES, EXTENDS_INTERFACE, INHERITS_FROM, USES, CALLS, IMPORTS
    async fn create_relationship(
        &self,
        from_entity_id: &str,
        to_entity_id: &str,
        relationship_type: &str,
        properties: &HashMap<String, String>,
    ) -> Result<()>;

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
    async fn batch_create_relationships(
        &self,
        relationships: &[(String, String, String)],
    ) -> Result<()>;

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
    async fn store_unresolved_relationship(
        &self,
        entity_id: &str,
        relationship_type: &str,
        target_qualified_name: &str,
    ) -> Result<()>;

    /// Find all nodes with unresolved CONTAINS relationships
    ///
    /// # Returns
    /// * `Result<Vec<(String, String)>>` - Vec of (child_id, parent_qualified_name) pairs
    async fn find_unresolved_contains_nodes(&self) -> Result<Vec<(String, String)>>;

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
    async fn resolve_contains_relationships_batch(
        &self,
        unresolved_nodes: &[(String, String)],
    ) -> Result<usize>;

    // ===== Utilities =====

    /// Run a query with named parameters
    ///
    /// # Arguments
    /// * `query_str` - Cypher query string
    /// * `params` - Named parameters for the query
    ///
    /// # Returns
    /// * `Result<()>` - Success or error
    async fn run_query_with_params(&self, query_str: &str, params: &[(&str, String)])
        -> Result<()>;

    // ===== Graph Query Methods =====

    /// Find all functions contained in a module
    ///
    /// # Arguments
    /// * `postgres` - PostgreSQL client for repository database lookup
    /// * `repository_id` - UUID of the repository
    /// * `module_qualified_name` - Qualified name of the module
    ///
    /// # Returns
    /// * `Result<Vec<String>>` - List of qualified function names
    async fn find_functions_in_module(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        module_qualified_name: &str,
    ) -> Result<Vec<String>>;

    /// Find all implementations of a trait
    ///
    /// # Arguments
    /// * `postgres` - PostgreSQL client for repository database lookup
    /// * `repository_id` - UUID of the repository
    /// * `trait_name` - Name of the trait
    ///
    /// # Returns
    /// * `Result<Vec<String>>` - List of qualified type names implementing the trait
    async fn find_trait_implementations(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        trait_name: &str,
    ) -> Result<Vec<String>>;

    /// Find class inheritance hierarchy
    ///
    /// # Arguments
    /// * `postgres` - PostgreSQL client for repository database lookup
    /// * `repository_id` - UUID of the repository
    /// * `class_name` - Name of the class
    ///
    /// # Returns
    /// * `Result<Vec<Vec<String>>>` - List of inheritance chains
    async fn find_class_hierarchy(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        class_name: &str,
    ) -> Result<Vec<Vec<String>>>;

    /// Find call graph (callers of a function)
    ///
    /// # Arguments
    /// * `postgres` - PostgreSQL client for repository database lookup
    /// * `repository_id` - UUID of the repository
    /// * `function_qualified_name` - Qualified name of the function
    /// * `max_depth` - Maximum call chain depth to traverse
    ///
    /// # Returns
    /// * `Result<Vec<(String, usize)>>` - List of (caller_name, depth) tuples
    async fn find_function_callers(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        function_qualified_name: &str,
        max_depth: usize,
    ) -> Result<Vec<(String, usize)>>;

    /// Find call graph (callees of a function - functions called by this function)
    ///
    /// # Arguments
    /// * `postgres` - PostgreSQL client for repository database lookup
    /// * `repository_id` - UUID of the repository
    /// * `function_qualified_name` - Qualified name of the function
    /// * `max_depth` - Maximum depth of the call graph to traverse
    ///
    /// # Returns
    /// * `Result<Vec<(String, usize)>>` - List of (callee_name, depth) tuples
    async fn find_function_callees(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        function_qualified_name: &str,
        max_depth: usize,
    ) -> Result<Vec<(String, usize)>>;

    /// Find unused functions (no incoming calls, not public)
    ///
    /// # Arguments
    /// * `postgres` - PostgreSQL client for repository database lookup
    /// * `repository_id` - UUID of the repository
    ///
    /// # Returns
    /// * `Result<Vec<String>>` - List of qualified function names
    async fn find_unused_functions(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<String>>;

    /// Find module dependencies (imports)
    ///
    /// # Arguments
    /// * `postgres` - PostgreSQL client for repository database lookup
    /// * `repository_id` - UUID of the repository
    /// * `module_qualified_name` - Qualified name of the module
    ///
    /// # Returns
    /// * `Result<Vec<String>>` - List of qualified module names
    async fn find_module_dependencies(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
        module_qualified_name: &str,
    ) -> Result<Vec<String>>;

    /// Find circular dependencies
    ///
    /// # Arguments
    /// * `postgres` - PostgreSQL client for repository database lookup
    /// * `repository_id` - UUID of the repository
    ///
    /// # Returns
    /// * `Result<Vec<Vec<String>>>` - List of dependency cycles
    async fn find_circular_dependencies(
        &self,
        postgres: &Arc<dyn crate::postgres::PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<Vec<String>>>;
}
