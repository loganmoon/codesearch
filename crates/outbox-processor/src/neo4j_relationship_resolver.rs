//! Neo4j relationship resolution framework
//!
//! This module provides the infrastructure for resolving relationships between entities
//! in Neo4j. Relationship resolution happens in the outbox processor after entities have
//! been created in the graph database.
//!
//! # Architecture
//!
//! Relationships are stored in two ways during entity indexing:
//! 1. **Resolved relationships**: Both source and target entities exist in the same batch.
//!    These are created directly as edges in Neo4j during outbox processing.
//! 2. **Unresolved relationships**: The target entity doesn't exist yet (not in batch).
//!    These are stored as node properties (e.g., `unresolved_contains_parent`) for later resolution.
//!
//! This module handles resolving the unresolved relationships by:
//! - Querying for entities with unresolved relationship properties
//! - Looking up target entities by qualified name
//! - Creating relationship edges in Neo4j
//! - Cleaning up the temporary unresolved properties
//!
//! # Resolution Triggers
//!
//! Relationship resolution is triggered by the outbox processor:
//! - After processing a batch of entity outbox entries
//! - When a repository's `pending_relationship_resolution` flag is set
//! - Periodically to handle any missed resolutions
//!
//! # Resolver Implementations
//!
//! Each relationship type has its own resolver implementation:
//! - `TraitImplResolver`: IMPLEMENTS, ASSOCIATES, EXTENDS_INTERFACE relationships
//! - `InheritanceResolver`: INHERITS_FROM relationships (class inheritance)
//! - `TypeUsageResolver`: USES relationships (field type dependencies)
//! - `CallGraphResolver`: CALLS relationships (function/method calls)
//! - `ImportsResolver`: IMPORTS relationships (module imports)
//! - CONTAINS relationships: Special case handled by dedicated batch resolution

use anyhow::Context;
use async_trait::async_trait;
use codesearch_core::error::Result;
use codesearch_storage::{Neo4jClientTrait, PostgresClientTrait};
use tracing::info;
use uuid::Uuid;

/// Trait for resolving specific relationship types between entities
///
/// Each implementation fetches relevant entities from PostgreSQL, builds lookup maps,
/// and extracts relationships based on entity metadata attributes.
///
/// Implementors provide the complete logic for fetching entities and extracting relationships.
/// The generic `resolve_relationships_generic` function handles database setup, batch creation, and logging.
#[async_trait]
pub trait RelationshipResolver: Send + Sync {
    /// Name of this resolver (for logging)
    fn name(&self) -> &'static str;

    /// Fetch entities and extract relationships
    ///
    /// Returns Vec<(from_id, to_id, relationship_type)>
    async fn resolve(
        &self,
        postgres: &std::sync::Arc<dyn PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<(String, String, String)>>;
}

/// Generic function to resolve relationships using a resolver implementation
///
/// This function provides the common infrastructure for all relationship resolvers:
/// 1. Ensures the Neo4j database is selected
/// 2. Calls the resolver's `resolve()` method to extract relationships
/// 3. Batch creates all relationships in Neo4j
/// 4. Logs progress and results
///
/// # Arguments
/// * `postgres` - PostgreSQL client for fetching entity data
/// * `neo4j` - Neo4j client for creating relationships
/// * `repository_id` - UUID of the repository to resolve relationships for
/// * `resolver` - Implementation of the RelationshipResolver trait
///
/// # Example
/// ```ignore
/// let resolver = TraitImplResolver;
/// resolve_relationships_generic(&postgres, &neo4j, repository_id, &resolver).await?;
/// ```
pub async fn resolve_relationships_generic(
    postgres: &std::sync::Arc<dyn PostgresClientTrait>,
    neo4j: &dyn Neo4jClientTrait,
    repository_id: Uuid,
    resolver: &dyn RelationshipResolver,
) -> Result<()> {
    info!("Resolving {} relationships...", resolver.name());

    // Ensure Neo4j database context
    let db_name = neo4j
        .ensure_repository_database(repository_id, postgres.as_ref())
        .await?;
    neo4j.use_database(&db_name).await?;

    // Resolve relationships
    let relationships = resolver.resolve(postgres, repository_id).await?;

    // Batch create all relationships
    neo4j
        .batch_create_relationships(&relationships)
        .await
        .with_context(|| format!("Failed to batch create {} relationships", resolver.name()))?;

    info!(
        "Resolved {} {} relationships",
        relationships.len(),
        resolver.name()
    );

    Ok(())
}

/// Resolve CONTAINS relationships after entities are indexed
///
/// CONTAINS relationships are special because they form the parent-child hierarchy
/// of code entities. This function uses a dedicated batch resolution method for
/// performance (2 queries instead of N queries).
///
/// # Process
/// 1. Query for all nodes with `unresolved_contains_parent` property
/// 2. Batch resolve using qualified name lookup
/// 3. Create relationship edges
/// 4. Clean up the temporary property
///
/// # Arguments
/// * `postgres` - PostgreSQL client for database context
/// * `neo4j` - Neo4j client for graph operations
/// * `repository_id` - UUID of the repository
///
/// # Example
/// ```ignore
/// resolve_contains_relationships(&postgres, &neo4j, repository_id).await?;
/// ```
pub async fn resolve_contains_relationships(
    postgres: &std::sync::Arc<dyn PostgresClientTrait>,
    neo4j: &dyn Neo4jClientTrait,
    repository_id: Uuid,
) -> Result<()> {
    // Ensure Neo4j database context
    let db_name = neo4j
        .ensure_repository_database(repository_id, postgres.as_ref())
        .await?;
    neo4j.use_database(&db_name).await?;

    // Find all nodes with unresolved parent
    info!("Searching for unresolved CONTAINS relationships...");

    let unresolved_nodes = neo4j.find_unresolved_contains_nodes().await?;

    info!("Found {} unresolved nodes", unresolved_nodes.len());

    if unresolved_nodes.is_empty() {
        return Ok(());
    }

    // Batch resolve all nodes in a single operation for performance
    let total_nodes = unresolved_nodes.len();
    let resolved_count = neo4j
        .resolve_contains_relationships_batch(&unresolved_nodes)
        .await?;

    let failed_count = total_nodes - resolved_count;

    if failed_count > 0 {
        tracing::warn!(
            "{} CONTAINS relationships could not be resolved (parents not found)",
            failed_count
        );
    }

    info!(
        "Resolved {} CONTAINS relationships ({} failed)",
        resolved_count, failed_count
    );

    Ok(())
}

// ============================================================================
// Relationship Resolver Implementations
// ============================================================================

/// Resolver for trait implementations (IMPLEMENTS and ASSOCIATES relationships)
pub struct TraitImplResolver;

#[async_trait]
impl RelationshipResolver for TraitImplResolver {
    fn name(&self) -> &'static str {
        "trait implementations"
    }

    async fn resolve(
        &self,
        postgres: &std::sync::Arc<dyn PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<(String, String, String)>> {
        use codesearch_core::entities::EntityType;
        use std::collections::HashMap;

        // Fetch all entity types in parallel for better performance
        let (impls_result, traits_result, structs_result, enums_result, interfaces_result) = tokio::join!(
            postgres.get_entities_by_type(repository_id, EntityType::Impl),
            postgres.get_entities_by_type(repository_id, EntityType::Trait),
            postgres.get_entities_by_type(repository_id, EntityType::Struct),
            postgres.get_entities_by_type(repository_id, EntityType::Enum),
            postgres.get_entities_by_type(repository_id, EntityType::Interface),
        );

        let impls = impls_result.context("Failed to get impl blocks")?;
        let traits = traits_result.context("Failed to get traits")?;
        let structs = structs_result.context("Failed to get structs")?;
        let enums = enums_result.context("Failed to get enums")?;
        let interfaces = interfaces_result.context("Failed to get interfaces")?;

        // Build lookup maps
        let trait_map: HashMap<String, String> = traits
            .iter()
            .map(|t| (t.name.clone(), t.entity_id.clone()))
            .collect();

        let mut type_map: HashMap<String, String> = HashMap::new();
        type_map.extend(
            structs
                .iter()
                .map(|s| (s.name.clone(), s.entity_id.clone())),
        );
        type_map.extend(enums.iter().map(|e| (e.name.clone(), e.entity_id.clone())));

        let interface_map: HashMap<String, String> = interfaces
            .iter()
            .map(|i| (i.name.clone(), i.entity_id.clone()))
            .collect();

        // Extract relationships
        let mut relationships = Vec::new();

        for impl_entity in impls {
            // IMPLEMENTS relationships
            if let Some(trait_name) = impl_entity.metadata.attributes.get("implements_trait") {
                if let Some(trait_id) = trait_map.get(trait_name) {
                    // Forward edge: impl -> trait
                    relationships.push((
                        impl_entity.entity_id.clone(),
                        trait_id.clone(),
                        "IMPLEMENTS".to_string(),
                    ));
                    // Reciprocal edge: trait -> impl
                    relationships.push((
                        trait_id.clone(),
                        impl_entity.entity_id.clone(),
                        "IMPLEMENTED_BY".to_string(),
                    ));
                }
            }

            // ASSOCIATES relationships
            if let Some(for_type) = impl_entity.metadata.attributes.get("for_type") {
                let type_name = for_type.split('<').next().unwrap_or(for_type).trim();

                if let Some(type_id) = type_map.get(type_name) {
                    // Forward edge: impl -> type
                    relationships.push((
                        impl_entity.entity_id.clone(),
                        type_id.clone(),
                        "ASSOCIATES".to_string(),
                    ));
                    // Reciprocal edge: type -> impl
                    relationships.push((
                        type_id.clone(),
                        impl_entity.entity_id.clone(),
                        "ASSOCIATED_WITH".to_string(),
                    ));
                }
            }

            // EXTENDS_INTERFACE relationships (TypeScript/JavaScript)
            if let Some(extends) = impl_entity.metadata.attributes.get("extends") {
                if let Some(interface_id) = interface_map.get(extends) {
                    // Forward edge: impl -> interface
                    relationships.push((
                        impl_entity.entity_id.clone(),
                        interface_id.clone(),
                        "EXTENDS_INTERFACE".to_string(),
                    ));
                    // Reciprocal edge: interface -> impl
                    relationships.push((
                        interface_id.clone(),
                        impl_entity.entity_id.clone(),
                        "EXTENDED_BY".to_string(),
                    ));
                }
            }
        }

        Ok(relationships)
    }
}

/// Resolver for class inheritance (INHERITS_FROM relationships)
pub struct InheritanceResolver;

#[async_trait]
impl RelationshipResolver for InheritanceResolver {
    fn name(&self) -> &'static str {
        "class inheritance"
    }

    async fn resolve(
        &self,
        postgres: &std::sync::Arc<dyn PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<(String, String, String)>> {
        use codesearch_core::entities::EntityType;
        use std::collections::HashMap;

        let classes = postgres
            .get_entities_by_type(repository_id, EntityType::Class)
            .await
            .context("Failed to get classes")?;

        let class_map: HashMap<String, String> = classes
            .iter()
            .map(|c| (c.name.clone(), c.entity_id.clone()))
            .collect();

        let mut relationships = Vec::new();

        for class_entity in classes {
            if let Some(extends) = class_entity.metadata.attributes.get("extends") {
                let parent_name = extends.split('<').next().unwrap_or(extends).trim();

                if let Some(parent_id) = class_map.get(parent_name) {
                    // Forward edge: child -> parent
                    relationships.push((
                        class_entity.entity_id.clone(),
                        parent_id.clone(),
                        "INHERITS_FROM".to_string(),
                    ));
                    // Reciprocal edge: parent -> child
                    relationships.push((
                        parent_id.clone(),
                        class_entity.entity_id.clone(),
                        "HAS_SUBCLASS".to_string(),
                    ));
                }
            }
        }

        Ok(relationships)
    }
}

/// Resolver for type usage (USES relationships)
pub struct TypeUsageResolver;

#[async_trait]
impl RelationshipResolver for TypeUsageResolver {
    fn name(&self) -> &'static str {
        "type usage"
    }

    async fn resolve(
        &self,
        postgres: &std::sync::Arc<dyn PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<(String, String, String)>> {
        use codesearch_core::entities::EntityType;
        use std::collections::HashMap;

        // Fetch entity types in parallel
        let (structs_result, all_types_result) = tokio::join!(
            postgres.get_entities_by_type(repository_id, EntityType::Struct),
            postgres.get_all_type_entities(repository_id),
        );

        let structs = structs_result.context("Failed to get structs")?;
        let all_types = all_types_result.context("Failed to get type entities")?;

        let type_map: HashMap<String, String> = all_types
            .iter()
            .map(|t| (t.name.clone(), t.entity_id.clone()))
            .collect();

        let mut relationships = Vec::new();

        for struct_entity in structs {
            if let Some(fields_json) = struct_entity.metadata.attributes.get("fields") {
                if let Ok(fields) = serde_json::from_str::<Vec<serde_json::Value>>(fields_json) {
                    for field in fields {
                        if let Some(field_type) = field.get("field_type").and_then(|v| v.as_str()) {
                            if field.get("name").and_then(|v| v.as_str()).is_some() {
                                let type_name =
                                    field_type.split('<').next().unwrap_or(field_type).trim();

                                if let Some(type_id) = type_map.get(type_name) {
                                    // Forward edge: struct -> type
                                    relationships.push((
                                        struct_entity.entity_id.clone(),
                                        type_id.clone(),
                                        "USES".to_string(),
                                    ));
                                    // Reciprocal edge: type -> struct
                                    relationships.push((
                                        type_id.clone(),
                                        struct_entity.entity_id.clone(),
                                        "USED_BY".to_string(),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(relationships)
    }
}

/// Resolver for call graph (CALLS relationships)
pub struct CallGraphResolver;

#[async_trait]
impl RelationshipResolver for CallGraphResolver {
    fn name(&self) -> &'static str {
        "call graph"
    }

    async fn resolve(
        &self,
        postgres: &std::sync::Arc<dyn PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<(String, String, String)>> {
        use codesearch_core::entities::EntityType;
        use std::collections::HashMap;

        // Fetch entity types in parallel
        let (functions_result, methods_result) = tokio::join!(
            postgres.get_entities_by_type(repository_id, EntityType::Function),
            postgres.get_entities_by_type(repository_id, EntityType::Method),
        );

        let functions = functions_result.context("Failed to get functions")?;
        let methods = methods_result.context("Failed to get methods")?;

        let all_callables: Vec<_> = functions.into_iter().chain(methods).collect();

        let mut callable_map: HashMap<String, String> = HashMap::new();
        for callable in &all_callables {
            callable_map.insert(callable.name.clone(), callable.entity_id.clone());
            callable_map.insert(callable.qualified_name.clone(), callable.entity_id.clone());
        }

        let mut relationships = Vec::new();

        for caller in all_callables {
            if let Some(calls_json) = caller.metadata.attributes.get("calls") {
                if let Ok(calls) = serde_json::from_str::<Vec<String>>(calls_json) {
                    for callee_name in calls {
                        if let Some(callee_id) = callable_map.get(&callee_name) {
                            // Forward edge: caller -> callee
                            relationships.push((
                                caller.entity_id.clone(),
                                callee_id.clone(),
                                "CALLS".to_string(),
                            ));
                            // Reciprocal edge: callee -> caller
                            relationships.push((
                                callee_id.clone(),
                                caller.entity_id.clone(),
                                "CALLED_BY".to_string(),
                            ));
                        }
                    }
                }
            }
        }

        Ok(relationships)
    }
}

/// Resolver for imports (IMPORTS relationships)
pub struct ImportsResolver;

#[async_trait]
impl RelationshipResolver for ImportsResolver {
    fn name(&self) -> &'static str {
        "imports"
    }

    async fn resolve(
        &self,
        postgres: &std::sync::Arc<dyn PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<(String, String, String)>> {
        use codesearch_core::entities::EntityType;
        use std::collections::HashMap;

        let modules = postgres
            .get_entities_by_type(repository_id, EntityType::Module)
            .await
            .context("Failed to get modules")?;

        let module_map: HashMap<String, String> = modules
            .iter()
            .map(|m| (m.qualified_name.clone(), m.entity_id.clone()))
            .collect();

        let mut relationships = Vec::new();

        for module_entity in modules {
            if let Some(imports_json) = module_entity.metadata.attributes.get("imports") {
                if let Ok(imports) = serde_json::from_str::<Vec<String>>(imports_json) {
                    for import_path in imports {
                        if let Some(imported_module_id) = module_map.get(&import_path) {
                            // Forward edge: module -> imported_module
                            relationships.push((
                                module_entity.entity_id.clone(),
                                imported_module_id.clone(),
                                "IMPORTS".to_string(),
                            ));
                            // Reciprocal edge: imported_module -> module
                            relationships.push((
                                imported_module_id.clone(),
                                module_entity.entity_id.clone(),
                                "IMPORTED_BY".to_string(),
                            ));
                        }
                    }
                }
            }
        }

        Ok(relationships)
    }
}
