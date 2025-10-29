//! Graph query implementations for Neo4j
//!
//! Provides structural code queries using the Neo4j graph database:
//! - Hierarchical containment (modules, functions)
//! - Trait/interface implementations
//! - Class inheritance
//! - Call graphs
//! - Module dependencies
//! - Dead code detection

use codesearch_core::error::{Error, Result};
use codesearch_storage::{Neo4jClient, PostgresClientTrait, Query};
use std::sync::Arc;
use uuid::Uuid;

/// Find all functions contained in a module
pub async fn find_functions_in_module(
    neo4j: &Arc<Neo4jClient>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
    module_qualified_name: &str,
) -> Result<Vec<String>> {
    let db_name = neo4j
        .ensure_repository_database(repository_id, postgres.as_ref())
        .await
        .map_err(|e| Error::storage(format!("Failed to ensure repository database: {e}")))?;
    neo4j
        .use_database(&db_name)
        .await
        .map_err(|e| Error::storage(format!("Failed to use database: {e}")))?;

    let query = Query::new(
        "MATCH (m:Module {qualified_name: $qname})-[:CONTAINS*]->(f:Function)
         RETURN f.qualified_name AS name"
            .to_string(),
    )
    .param("qname", module_qualified_name);

    let mut result = neo4j
        .graph()
        .execute(query)
        .await
        .map_err(|e| Error::storage(format!("Query execution failed: {e}")))?;

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
    neo4j: &Arc<Neo4jClient>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
    trait_name: &str,
) -> Result<Vec<String>> {
    let db_name = neo4j
        .ensure_repository_database(repository_id, postgres.as_ref())
        .await
        .map_err(|e| Error::storage(format!("Failed to ensure repository database: {e}")))?;
    neo4j
        .use_database(&db_name)
        .await
        .map_err(|e| Error::storage(format!("Failed to use database: {e}")))?;

    let query = Query::new(
        "MATCH (impl:ImplBlock)-[:IMPLEMENTS]->(trait:Interface {name: $trait_name})
         MATCH (impl)-[:ASSOCIATES]->(type)
         RETURN type.qualified_name AS name"
            .to_string(),
    )
    .param("trait_name", trait_name);

    let mut result = neo4j
        .graph()
        .execute(query)
        .await
        .map_err(|e| Error::storage(format!("Query execution failed: {e}")))?;

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
    neo4j: &Arc<Neo4jClient>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
    class_name: &str,
) -> Result<Vec<Vec<String>>> {
    let db_name = neo4j
        .ensure_repository_database(repository_id, postgres.as_ref())
        .await
        .map_err(|e| Error::storage(format!("Failed to ensure repository database: {e}")))?;
    neo4j
        .use_database(&db_name)
        .await
        .map_err(|e| Error::storage(format!("Failed to use database: {e}")))?;

    let query = Query::new(
        "MATCH path = (root:Class {name: $class_name})-[:INHERITS_FROM*]->(ancestor:Class)
         RETURN [node in nodes(path) | node.name] AS hierarchy"
            .to_string(),
    )
    .param("class_name", class_name);

    let mut result = neo4j
        .graph()
        .execute(query)
        .await
        .map_err(|e| Error::storage(format!("Query execution failed: {e}")))?;

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
    neo4j: &Arc<Neo4jClient>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
    function_qualified_name: &str,
    max_depth: usize,
) -> Result<Vec<(String, usize)>> {
    let db_name = neo4j
        .ensure_repository_database(repository_id, postgres.as_ref())
        .await
        .map_err(|e| Error::storage(format!("Failed to ensure repository database: {e}")))?;
    neo4j
        .use_database(&db_name)
        .await
        .map_err(|e| Error::storage(format!("Failed to use database: {e}")))?;

    let query_str = format!(
        "MATCH (target {{qualified_name: $qname}})
         MATCH path = (caller)-[:CALLS*1..{max_depth}]->(target)
         RETURN DISTINCT caller.qualified_name AS name, length(path) AS depth
         ORDER BY depth ASC"
    );

    let query = Query::new(query_str).param("qname", function_qualified_name);

    let mut result = neo4j
        .graph()
        .execute(query)
        .await
        .map_err(|e| Error::storage(format!("Query execution failed: {e}")))?;

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
    neo4j: &Arc<Neo4jClient>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
) -> Result<Vec<String>> {
    let db_name = neo4j
        .ensure_repository_database(repository_id, postgres.as_ref())
        .await
        .map_err(|e| Error::storage(format!("Failed to ensure repository database: {e}")))?;
    neo4j
        .use_database(&db_name)
        .await
        .map_err(|e| Error::storage(format!("Failed to use database: {e}")))?;

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

    let mut result = neo4j
        .graph()
        .execute(query)
        .await
        .map_err(|e| Error::storage(format!("Query execution failed: {e}")))?;

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
    neo4j: &Arc<Neo4jClient>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
    module_qualified_name: &str,
) -> Result<Vec<String>> {
    let db_name = neo4j
        .ensure_repository_database(repository_id, postgres.as_ref())
        .await
        .map_err(|e| Error::storage(format!("Failed to ensure repository database: {e}")))?;
    neo4j
        .use_database(&db_name)
        .await
        .map_err(|e| Error::storage(format!("Failed to use database: {e}")))?;

    let query = Query::new(
        "MATCH (m:Module {qualified_name: $qname})-[:IMPORTS]->(imported:Module)
         RETURN imported.qualified_name AS name"
            .to_string(),
    )
    .param("qname", module_qualified_name);

    let mut result = neo4j
        .graph()
        .execute(query)
        .await
        .map_err(|e| Error::storage(format!("Query execution failed: {e}")))?;

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
    neo4j: &Arc<Neo4jClient>,
    postgres: &Arc<dyn PostgresClientTrait>,
    repository_id: Uuid,
) -> Result<Vec<Vec<String>>> {
    let db_name = neo4j
        .ensure_repository_database(repository_id, postgres.as_ref())
        .await
        .map_err(|e| Error::storage(format!("Failed to ensure repository database: {e}")))?;
    neo4j
        .use_database(&db_name)
        .await
        .map_err(|e| Error::storage(format!("Failed to use database: {e}")))?;

    let query = Query::new(
        "MATCH path = (m1:Module)-[:IMPORTS*]->(m2:Module)-[:IMPORTS*]->(m1)
         WHERE m1 <> m2
         RETURN [node in nodes(path) | node.qualified_name] AS cycle,
                length(path) AS length
         ORDER BY length
         LIMIT 100"
            .to_string(),
    );

    let mut result = neo4j
        .graph()
        .execute(query)
        .await
        .map_err(|e| Error::storage(format!("Query execution failed: {e}")))?;

    let mut cycles = Vec::new();
    while let Ok(Some(row)) = result.next().await {
        if let Ok(cycle) = row.get::<Vec<String>>("cycle") {
            cycles.push(cycle);
        }
    }
    Ok(cycles)
}
