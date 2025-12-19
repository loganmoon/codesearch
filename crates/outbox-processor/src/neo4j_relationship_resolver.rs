//! Neo4j relationship resolution framework
//!
//! This module provides dedicated resolvers for creating relationship edges in Neo4j.
//! Each resolver queries entity_metadata directly using entity attributes to find
//! related entities and create appropriate graph edges.
//!
//! # Architecture
//!
//! Relationship information is stored in entity metadata attributes during extraction:
//! - `parent_scope`: Qualified name of containing entity (for CONTAINS)
//! - `implements_trait`: Trait name being implemented (for IMPLEMENTS)
//! - `for_type`: Type that impl block is for (for ASSOCIATES)
//! - `extends`: Parent class/interface name (for INHERITS_FROM, EXTENDS_INTERFACE)
//! - `fields`: JSON array with field types (for USES)
//! - `calls`: JSON array of called functions (for CALLS)
//! - `imports`: JSON array of imported modules (for IMPORTS)
//!
//! Resolution is triggered once when indexing completes (drain mode) and queries
//! entity_metadata to build lookup maps and create Neo4j edges.
//!
//! # Resolver Implementations
//!
//! Each relationship type has its own resolver:
//! - `ContainsResolver`: CONTAINS relationships (parent_scope -> parent)
//! - `TraitImplResolver`: IMPLEMENTS, ASSOCIATES, EXTENDS_INTERFACE
//! - `InheritanceResolver`: INHERITS_FROM for class inheritance
//! - `TypeUsageResolver`: USES for field type dependencies
//! - `CallGraphResolver`: CALLS for function/method calls
//! - `ImportsResolver`: IMPORTS for module imports

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
/// 1. Calls the resolver's `resolve()` method to extract relationships
/// 2. Batch creates all relationships in Neo4j
/// 3. Logs progress and results
///
/// # Prerequisites
/// The caller MUST ensure the Neo4j database is already selected via `use_database()`
/// before calling this function. This is typically done once per repository in
/// `resolve_pending_relationships()`.
///
/// # Arguments
/// * `postgres` - PostgreSQL client for fetching entity data
/// * `neo4j` - Neo4j client for creating relationships (must have database already selected)
/// * `repository_id` - UUID of the repository to resolve relationships for
/// * `resolver` - Implementation of the RelationshipResolver trait
///
/// # Example
/// ```ignore
/// // Caller must select database first
/// neo4j.use_database(&db_name).await?;
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

        // Build lookup maps using qualified_name for correct resolution
        let trait_map: HashMap<String, String> = traits
            .iter()
            .map(|t| (t.qualified_name.clone(), t.entity_id.clone()))
            .collect();

        let mut type_map: HashMap<String, String> = HashMap::new();
        type_map.extend(
            structs
                .iter()
                .map(|s| (s.qualified_name.clone(), s.entity_id.clone())),
        );
        type_map.extend(
            enums
                .iter()
                .map(|e| (e.qualified_name.clone(), e.entity_id.clone())),
        );

        let interface_map: HashMap<String, String> = interfaces
            .iter()
            .map(|i| (i.qualified_name.clone(), i.entity_id.clone()))
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

        // Build lookup map using qualified_name for correct resolution
        let class_map: HashMap<String, String> = classes
            .iter()
            .map(|c| (c.qualified_name.clone(), c.entity_id.clone()))
            .collect();

        let mut relationships = Vec::new();

        for class_entity in &classes {
            // Check both 'extends' (JS/TS) and 'bases' (Python) attributes
            let parent_names: Vec<String> =
                if let Some(extends) = class_entity.metadata.attributes.get("extends") {
                    // JS/TS: single parent class name
                    vec![extends
                        .split('<')
                        .next()
                        .unwrap_or(extends)
                        .trim()
                        .to_string()]
                } else if let Some(bases_json) = class_entity.metadata.attributes.get("bases") {
                    // Python: JSON array of base class names
                    serde_json::from_str::<Vec<String>>(bases_json).unwrap_or_default()
                } else {
                    continue;
                };

            for parent_name in parent_names {
                let parent_name = parent_name.split('<').next().unwrap_or(&parent_name).trim();

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
///
/// Handles:
/// - Struct field types (from `fields` attribute)
/// - Function/Method parameter and return types (from `uses_types` attribute)
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
        let (structs_result, functions_result, methods_result, all_types_result) = tokio::join!(
            postgres.get_entities_by_type(repository_id, EntityType::Struct),
            postgres.get_entities_by_type(repository_id, EntityType::Function),
            postgres.get_entities_by_type(repository_id, EntityType::Method),
            postgres.get_all_type_entities(repository_id),
        );

        let structs = structs_result.context("Failed to get structs")?;
        let functions = functions_result.context("Failed to get functions")?;
        let methods = methods_result.context("Failed to get methods")?;
        let all_types = all_types_result.context("Failed to get type entities")?;

        // Build type lookup map (qualified_name -> entity_id) for correct resolution
        let type_map: HashMap<String, String> = all_types
            .iter()
            .map(|t| (t.qualified_name.clone(), t.entity_id.clone()))
            .collect();

        let mut relationships = Vec::new();

        // Process struct field types
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

        // Process function and method uses_types
        let callables: Vec<_> = functions.into_iter().chain(methods).collect();
        for callable in callables {
            if let Some(uses_types_json) = callable.metadata.attributes.get("uses_types") {
                if let Ok(types) = serde_json::from_str::<Vec<String>>(uses_types_json) {
                    for type_ref in types {
                        // Strip generics and get the base type name
                        let type_name = type_ref.split('<').next().unwrap_or(&type_ref).trim();

                        if let Some(type_id) = type_map.get(type_name) {
                            // Forward edge: function/method -> type
                            relationships.push((
                                callable.entity_id.clone(),
                                type_id.clone(),
                                "USES".to_string(),
                            ));
                            // Reciprocal edge: type -> function/method
                            relationships.push((
                                type_id.clone(),
                                callable.entity_id.clone(),
                                "USED_BY".to_string(),
                            ));
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

/// Resolve a relative import path to a qualified name
///
/// Given an importer's qualified_name and a relative import path,
/// returns the resolved qualified_name of the imported module.
///
/// Examples:
/// - importer: "utils.helpers", import: "./store" -> "utils.store"
/// - importer: "utils.helpers", import: "../core" -> "core"
/// - importer: "a.b.c", import: "../../x" -> "x"
fn resolve_import_path(importer_qname: &str, import_path: &str) -> Option<String> {
    // Non-relative imports (bare specifiers like "react") return None
    // They're typically external packages
    if !import_path.starts_with('.') {
        return None;
    }

    // Split importer qualified name into parts
    // e.g., "utils.helpers" -> ["utils", "helpers"]
    let mut parts: Vec<&str> = importer_qname.split('.').collect();

    // Remove the importer's own name (last segment) to get the directory
    // e.g., ["utils", "helpers"] -> ["utils"]
    if !parts.is_empty() {
        parts.pop();
    }

    // Process import path segments
    for segment in import_path.split('/') {
        match segment {
            "." | "" => {
                // Current directory, no change
            }
            ".." => {
                // Go up one level
                if !parts.is_empty() {
                    parts.pop();
                }
            }
            _ => {
                // Add this segment (strip file extension if present)
                let name = segment.rsplit('.').next_back().unwrap_or(segment);
                parts.push(name);
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("."))
    }
}

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

        // Build lookup maps for module resolution
        // 1. By qualified_name (e.g., "utils.helpers")
        // 2. By simple name (e.g., "helpers") for bare imports within the same package
        let module_map: HashMap<String, String> = modules
            .iter()
            .map(|m| (m.qualified_name.clone(), m.entity_id.clone()))
            .collect();

        let simple_name_map: HashMap<String, String> = modules
            .iter()
            .map(|m| (m.name.clone(), m.entity_id.clone()))
            .collect();

        let mut relationships = Vec::new();

        for module_entity in &modules {
            if let Some(imports_json) = module_entity.metadata.attributes.get("imports") {
                if let Ok(imports) = serde_json::from_str::<Vec<String>>(imports_json) {
                    for import_path in imports {
                        // Try to resolve the import
                        let imported_module_id = if import_path.starts_with('.') {
                            // Relative import: resolve based on importer's location
                            resolve_import_path(&module_entity.qualified_name, &import_path)
                                .and_then(|resolved| module_map.get(&resolved))
                        } else {
                            // Bare import: try simple name match (internal package import)
                            // External packages (react, lodash, etc.) won't match
                            simple_name_map.get(&import_path)
                        };

                        if let Some(imported_id) = imported_module_id {
                            // Forward edge: module -> imported_module
                            relationships.push((
                                module_entity.entity_id.clone(),
                                imported_id.clone(),
                                "IMPORTS".to_string(),
                            ));
                            // Reciprocal edge: imported_module -> module
                            relationships.push((
                                imported_id.clone(),
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

/// Resolver for containment (CONTAINS relationships)
///
/// Creates parent-child relationships based on entity.parent_scope.
/// Queries entity_metadata directly to build qualified_name -> entity_id map.
pub struct ContainsResolver;

#[async_trait]
impl RelationshipResolver for ContainsResolver {
    fn name(&self) -> &'static str {
        "containment"
    }

    async fn resolve(
        &self,
        postgres: &std::sync::Arc<dyn PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Vec<(String, String, String)>> {
        use std::collections::HashMap;

        // Get all entities for this repository
        let entities = postgres
            .get_all_entities(repository_id)
            .await
            .context("Failed to get all entities")?;

        // Build qualified_name -> entity_id map for parent lookup
        let qname_to_id: HashMap<&str, &str> = entities
            .iter()
            .map(|e| (e.qualified_name.as_str(), e.entity_id.as_str()))
            .collect();

        let mut relationships = Vec::new();

        for entity in &entities {
            if let Some(parent_scope) = &entity.parent_scope {
                // Look up parent by qualified_name
                if let Some(&parent_id) = qname_to_id.get(parent_scope.as_str()) {
                    // Forward edge: parent CONTAINS child
                    relationships.push((
                        parent_id.to_string(),
                        entity.entity_id.clone(),
                        "CONTAINS".to_string(),
                    ));
                    // Note: No reciprocal edge for CONTAINS - it's directional
                }
            }
        }

        Ok(relationships)
    }
}

// ============================================================================
// External Reference Resolution
// ============================================================================

/// External reference collected from entity attributes
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ExternalRef {
    /// Qualified name of the external reference
    qualified_name: String,
    /// Package/crate name (extracted from first segment)
    package: Option<String>,
}

impl ExternalRef {
    fn new(qualified_name: String) -> Self {
        // Extract package from first path segment
        let package = qualified_name
            .trim_start_matches("external::")
            .split("::")
            .next()
            .map(|s| s.to_string());

        Self {
            qualified_name,
            package,
        }
    }

    /// Generate a stable entity_id for this external reference
    fn entity_id(&self) -> String {
        format!(
            "external::{}",
            self.qualified_name.trim_start_matches("external::")
        )
    }
}

/// Resolve external references and create External stub nodes
///
/// This function:
/// 1. Collects all unresolved references from entity attributes
/// 2. Creates External stub nodes in Neo4j for those references
/// 3. Creates relationships from source entities to External nodes
///
/// External references are identified by:
/// - explicit "external::" prefix in resolved attributes
/// - references in implements_trait, extends, uses_types that don't match any entity
pub async fn resolve_external_references(
    postgres: &std::sync::Arc<dyn PostgresClientTrait>,
    neo4j: &dyn Neo4jClientTrait,
    repository_id: Uuid,
) -> Result<()> {
    use std::collections::{HashMap, HashSet};

    info!("Resolving external references...");

    // Get all entities for this repository
    let entities = postgres
        .get_all_entities(repository_id)
        .await
        .context("Failed to get all entities")?;

    if entities.is_empty() {
        return Ok(());
    }

    // Build set of known qualified names
    let known_names: HashSet<&str> = entities.iter().map(|e| e.qualified_name.as_str()).collect();

    // Also build a name -> qualified_name map for simple name lookups
    let name_to_qname: HashMap<&str, &str> = entities
        .iter()
        .map(|e| (e.name.as_str(), e.qualified_name.as_str()))
        .collect();

    // Collect all external references with their source relationships
    let mut external_refs: HashSet<ExternalRef> = HashSet::new();
    let mut relationships: Vec<(String, String, String)> = Vec::new();

    for entity in &entities {
        // Check implements_trait
        if let Some(trait_ref) = entity.metadata.attributes.get("implements_trait") {
            if is_external_ref(trait_ref, &known_names, &name_to_qname) {
                let ext_ref = ExternalRef::new(normalize_external_ref(trait_ref));
                let ext_id = ext_ref.entity_id();
                external_refs.insert(ext_ref);
                relationships.push((
                    entity.entity_id.clone(),
                    ext_id.clone(),
                    "IMPLEMENTS".to_string(),
                ));
                relationships.push((
                    ext_id,
                    entity.entity_id.clone(),
                    "IMPLEMENTED_BY".to_string(),
                ));
            }
        }

        // Check extends (for classes/interfaces)
        if let Some(extends_ref) = entity.metadata.attributes.get("extends") {
            if is_external_ref(extends_ref, &known_names, &name_to_qname) {
                let ext_ref = ExternalRef::new(normalize_external_ref(extends_ref));
                let ext_id = ext_ref.entity_id();
                external_refs.insert(ext_ref);
                relationships.push((
                    entity.entity_id.clone(),
                    ext_id.clone(),
                    "INHERITS_FROM".to_string(),
                ));
                relationships.push((ext_id, entity.entity_id.clone(), "HAS_SUBCLASS".to_string()));
            }
        }

        // Check uses_types (JSON array of type references)
        if let Some(uses_types_str) = entity.metadata.attributes.get("uses_types") {
            if let Ok(types) = serde_json::from_str::<Vec<String>>(uses_types_str) {
                for type_ref in types {
                    if is_external_ref(&type_ref, &known_names, &name_to_qname) {
                        let ext_ref = ExternalRef::new(normalize_external_ref(&type_ref));
                        let ext_id = ext_ref.entity_id();
                        external_refs.insert(ext_ref);
                        relationships.push((
                            entity.entity_id.clone(),
                            ext_id.clone(),
                            "USES".to_string(),
                        ));
                        relationships.push((
                            ext_id,
                            entity.entity_id.clone(),
                            "USED_BY".to_string(),
                        ));
                    }
                }
            }
        }

        // Check calls (JSON array of function calls)
        if let Some(calls_str) = entity.metadata.attributes.get("calls") {
            if let Ok(calls) = serde_json::from_str::<Vec<String>>(calls_str) {
                for call_ref in calls {
                    if is_external_ref(&call_ref, &known_names, &name_to_qname) {
                        let ext_ref = ExternalRef::new(normalize_external_ref(&call_ref));
                        let ext_id = ext_ref.entity_id();
                        external_refs.insert(ext_ref);
                        relationships.push((
                            entity.entity_id.clone(),
                            ext_id.clone(),
                            "CALLS".to_string(),
                        ));
                        relationships.push((
                            ext_id,
                            entity.entity_id.clone(),
                            "CALLED_BY".to_string(),
                        ));
                    }
                }
            }
        }
    }

    if external_refs.is_empty() {
        info!("No external references to resolve");
        return Ok(());
    }

    // Create External nodes
    let ext_nodes: Vec<(String, String, Option<String>)> = external_refs
        .iter()
        .map(|r| (r.entity_id(), r.qualified_name.clone(), r.package.clone()))
        .collect();

    neo4j
        .batch_create_external_nodes(&repository_id.to_string(), &ext_nodes)
        .await
        .context("Failed to create external nodes")?;

    info!("Created {} external nodes", ext_nodes.len());

    // Create relationships to external nodes
    neo4j
        .batch_create_relationships(&relationships)
        .await
        .context("Failed to create external relationships")?;

    info!(
        "Resolved {} external references ({} relationships)",
        external_refs.len(),
        relationships.len()
    );

    Ok(())
}

/// Check if a reference is external (not in the known entity set)
fn is_external_ref(
    ref_name: &str,
    known_names: &std::collections::HashSet<&str>,
    name_to_qname: &std::collections::HashMap<&str, &str>,
) -> bool {
    // Explicit external prefix
    if ref_name.starts_with("external::") || ref_name.starts_with("external.") {
        return true;
    }

    // Check if it matches any known qualified name
    if known_names.contains(ref_name) {
        return false;
    }

    // Strip generics before further checks
    let without_generics = ref_name.split('<').next().unwrap_or(ref_name);
    if known_names.contains(without_generics) {
        return false;
    }

    // Extract simple name using language-appropriate separator
    // Rust uses "::", JS/TS/Python use "."
    let simple_name = if without_generics.contains("::") {
        without_generics
            .rsplit("::")
            .next()
            .unwrap_or(without_generics)
    } else if without_generics.contains('.') {
        without_generics
            .rsplit('.')
            .next()
            .unwrap_or(without_generics)
    } else {
        without_generics
    };

    if name_to_qname.contains_key(simple_name) {
        return false;
    }

    // Assume it's external if we can't find it
    true
}

/// Normalize an external reference name
fn normalize_external_ref(ref_name: &str) -> String {
    // Strip crate:: prefix, keep external:: or add it
    let cleaned = ref_name
        .trim_start_matches("crate::")
        .split('<')
        .next()
        .unwrap_or(ref_name);

    if cleaned.starts_with("external::") {
        cleaned.to_string()
    } else {
        format!("external::{cleaned}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_import_path_current_dir() {
        // ./store relative to utils.helpers -> utils.store
        assert_eq!(
            resolve_import_path("utils.helpers", "./store"),
            Some("utils.store".to_string())
        );
    }

    #[test]
    fn test_resolve_import_path_parent_dir() {
        // ../core relative to utils.helpers -> core
        assert_eq!(
            resolve_import_path("utils.helpers", "../core"),
            Some("core".to_string())
        );
    }

    #[test]
    fn test_resolve_import_path_multiple_parents() {
        // ../../x relative to a.b.c -> x
        assert_eq!(
            resolve_import_path("a.b.c", "../../x"),
            Some("x".to_string())
        );
    }

    #[test]
    fn test_resolve_import_path_nested() {
        // ./sub/module relative to utils.helpers -> utils.sub.module
        assert_eq!(
            resolve_import_path("utils.helpers", "./sub/module"),
            Some("utils.sub.module".to_string())
        );
    }

    #[test]
    fn test_resolve_import_path_bare_specifier() {
        // Bare specifiers (external packages) return None
        assert_eq!(resolve_import_path("utils.helpers", "react"), None);
        assert_eq!(resolve_import_path("utils.helpers", "lodash"), None);
    }

    #[test]
    fn test_resolve_import_path_with_extension() {
        // Extensions should be stripped
        assert_eq!(
            resolve_import_path("utils.helpers", "./store.js"),
            Some("utils.store".to_string())
        );
    }

    #[test]
    fn test_resolve_import_path_root_level() {
        // Single segment module
        assert_eq!(
            resolve_import_path("index", "./utils"),
            Some("utils".to_string())
        );
    }

    #[test]
    fn test_is_external_ref_with_dot_separator() {
        let mut known_names = std::collections::HashSet::new();
        known_names.insert("utils.helpers");
        known_names.insert("core.module");

        let mut name_to_qname = std::collections::HashMap::new();
        name_to_qname.insert("helpers", "utils.helpers");
        name_to_qname.insert("module", "core.module");

        // Full qualified name match
        assert!(!is_external_ref(
            "utils.helpers",
            &known_names,
            &name_to_qname
        ));

        // Simple name match (via name_to_qname)
        assert!(!is_external_ref("helpers", &known_names, &name_to_qname));

        // Unknown reference is external
        assert!(is_external_ref(
            "unknown.thing",
            &known_names,
            &name_to_qname
        ));

        // Explicit external prefix
        assert!(is_external_ref(
            "external.react",
            &known_names,
            &name_to_qname
        ));
    }

    #[test]
    fn test_is_external_ref_with_rust_separator() {
        let mut known_names = std::collections::HashSet::new();
        known_names.insert("crate::utils::helpers");

        let mut name_to_qname = std::collections::HashMap::new();
        name_to_qname.insert("helpers", "crate::utils::helpers");

        // Full qualified name match
        assert!(!is_external_ref(
            "crate::utils::helpers",
            &known_names,
            &name_to_qname
        ));

        // Simple name match
        assert!(!is_external_ref("helpers", &known_names, &name_to_qname));

        // Explicit external prefix
        assert!(is_external_ref(
            "external::std::collections",
            &known_names,
            &name_to_qname
        ));
    }
}
