//! Graph query implementations for Neo4j
//!
//! Provides structural code queries using the Neo4j graph database:
//! - Hierarchical containment (modules, functions)
//! - Trait/interface implementations
//! - Class inheritance
//! - Call graphs
//! - Module dependencies
//! - Dead code detection
//!
//! All implementations have been moved to Neo4jClientTrait. This module provides
//! re-exports for backwards compatibility.

use codesearch_core::error::Result;
use codesearch_storage::{Neo4jClientTrait, PostgresClientTrait};
use std::sync::Arc;
use uuid::Uuid;

/// Find all functions contained in a module
pub async fn find_functions_in_module(
    neo4j: &Arc<dyn Neo4jClientTrait>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
    module_qualified_name: &str,
) -> Result<Vec<String>> {
    neo4j
        .find_functions_in_module(postgres, repository_id, module_qualified_name)
        .await
        .map_err(|e| codesearch_core::error::Error::storage(format!("Query execution failed: {e}")))
}

/// Find all implementations of a trait
pub async fn find_trait_implementations(
    neo4j: &Arc<dyn Neo4jClientTrait>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
    trait_name: &str,
) -> Result<Vec<String>> {
    neo4j
        .find_trait_implementations(postgres, repository_id, trait_name)
        .await
        .map_err(|e| codesearch_core::error::Error::storage(format!("Query execution failed: {e}")))
}

/// Find class inheritance hierarchy
pub async fn find_class_hierarchy(
    neo4j: &Arc<dyn Neo4jClientTrait>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
    class_name: &str,
) -> Result<Vec<Vec<String>>> {
    neo4j
        .find_class_hierarchy(postgres, repository_id, class_name)
        .await
        .map_err(|e| codesearch_core::error::Error::storage(format!("Query execution failed: {e}")))
}

/// Find call graph (callers of a function)
pub async fn find_function_callers(
    neo4j: &Arc<dyn Neo4jClientTrait>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
    function_qualified_name: &str,
    max_depth: usize,
) -> Result<Vec<(String, usize)>> {
    neo4j
        .find_function_callers(postgres, repository_id, function_qualified_name, max_depth)
        .await
        .map_err(|e| codesearch_core::error::Error::storage(format!("Query execution failed: {e}")))
}

/// Find unused functions (no incoming calls, not public)
pub async fn find_unused_functions(
    neo4j: &Arc<dyn Neo4jClientTrait>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
) -> Result<Vec<String>> {
    neo4j
        .find_unused_functions(postgres, repository_id)
        .await
        .map_err(|e| codesearch_core::error::Error::storage(format!("Query execution failed: {e}")))
}

/// Find module dependencies (imports)
pub async fn find_module_dependencies(
    neo4j: &Arc<dyn Neo4jClientTrait>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
    module_qualified_name: &str,
) -> Result<Vec<String>> {
    neo4j
        .find_module_dependencies(postgres, repository_id, module_qualified_name)
        .await
        .map_err(|e| codesearch_core::error::Error::storage(format!("Query execution failed: {e}")))
}

/// Find circular dependencies
pub async fn find_circular_dependencies(
    neo4j: &Arc<dyn Neo4jClientTrait>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
) -> Result<Vec<Vec<String>>> {
    neo4j
        .find_circular_dependencies(postgres, repository_id)
        .await
        .map_err(|e| codesearch_core::error::Error::storage(format!("Query execution failed: {e}")))
}
