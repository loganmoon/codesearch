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
use codesearch_core::entities::{CodeEntity, EntityType, SourceReference};
use codesearch_core::error::Result;
use codesearch_languages::rust::import_resolution::parse_trait_impl_short_form;
use codesearch_storage::{Neo4jClientTrait, PostgresClientTrait};
use std::collections::HashMap;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

// ============================================================================
// Entity Cache
// ============================================================================

/// Cache for entity data during relationship resolution
///
/// Fetches all entities once at the start of resolution and provides
/// typed accessors for filtering by entity type. This eliminates
/// redundant database queries across multiple resolvers.
pub struct EntityCache {
    /// All entities for the repository
    entities: Vec<CodeEntity>,
    /// Pre-built lookup: qualified_name -> entity_id (semantic, package-relative)
    qname_to_id: HashMap<String, String>,
    /// Pre-built lookup: path_entity_identifier -> entity_id (file-path-based)
    path_id_to_id: HashMap<String, String>,
    /// Pre-built lookup: simple name -> entity_id (for fallback)
    name_to_id: HashMap<String, String>,
}

impl EntityCache {
    /// Create a new cache by fetching all entities from PostgreSQL
    pub async fn new(
        postgres: &std::sync::Arc<dyn PostgresClientTrait>,
        repository_id: Uuid,
    ) -> Result<Self> {
        let entities = postgres
            .get_all_entities(repository_id)
            .await
            .context("Failed to fetch entities for cache")?;

        let qname_to_id: HashMap<String, String> = entities
            .iter()
            .map(|e| (e.qualified_name.clone(), e.entity_id.clone()))
            .collect();

        // Build path_entity_identifier map (only for entities that have it)
        let path_id_to_id: HashMap<String, String> = entities
            .iter()
            .filter_map(|e| {
                e.path_entity_identifier
                    .as_ref()
                    .map(|pid| (pid.clone(), e.entity_id.clone()))
            })
            .collect();

        let name_to_id: HashMap<String, String> = entities
            .iter()
            .map(|e| (e.name.clone(), e.entity_id.clone()))
            .collect();

        Ok(Self {
            entities,
            qname_to_id,
            path_id_to_id,
            name_to_id,
        })
    }

    /// Get all entities
    pub fn all(&self) -> &[CodeEntity] {
        &self.entities
    }

    /// Get entities filtered by type
    pub fn by_type(&self, entity_type: EntityType) -> Vec<&CodeEntity> {
        self.entities
            .iter()
            .filter(|e| e.entity_type == entity_type)
            .collect()
    }

    /// Get all type entities (Struct, Enum, Class, Interface, Trait, TypeAlias)
    pub fn all_types(&self) -> Vec<&CodeEntity> {
        self.entities
            .iter()
            .filter(|e| {
                matches!(
                    e.entity_type,
                    EntityType::Struct
                        | EntityType::Enum
                        | EntityType::Class
                        | EntityType::Interface
                        | EntityType::Trait
                        | EntityType::TypeAlias
                )
            })
            .collect()
    }

    /// Get qualified_name -> entity_id lookup map
    pub fn qname_map(&self) -> &HashMap<String, String> {
        &self.qname_to_id
    }

    /// Get path_entity_identifier -> entity_id lookup map
    ///
    /// For file-path-based lookups (useful for import resolution)
    pub fn path_id_map(&self) -> &HashMap<String, String> {
        &self.path_id_to_id
    }

    /// Get simple name -> entity_id lookup map (use with caution - collisions possible)
    pub fn name_map(&self) -> &HashMap<String, String> {
        &self.name_to_id
    }

    /// Resolve a reference using multiple fallback strategies
    ///
    /// For import/file-based resolution:
    /// 1. Try path_entity_identifier map first
    /// 2. Fall back to qualified_name map
    /// 3. Fall back to simple name map
    pub fn resolve_path_reference(&self, reference: &str) -> Option<&String> {
        self.path_id_to_id
            .get(reference)
            .or_else(|| self.qname_to_id.get(reference))
            .or_else(|| self.name_to_id.get(reference))
    }

    /// Resolve a reference using semantic matching
    ///
    /// For semantic lookups (traits, types, etc.):
    /// 1. Try qualified_name map first
    /// 2. Fall back to path_entity_identifier map
    /// 3. Fall back to simple name map
    pub fn resolve_semantic_reference(&self, reference: &str) -> Option<&String> {
        self.qname_to_id
            .get(reference)
            .or_else(|| self.path_id_to_id.get(reference))
            .or_else(|| self.name_to_id.get(reference))
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

/// Trait for resolving specific relationship types between entities
///
/// Each implementation uses the pre-populated EntityCache to build lookup maps
/// and extract relationships based on entity metadata attributes.
///
/// The cache is populated once at the start of resolution, eliminating
/// redundant database queries across resolvers.
#[async_trait]
pub trait RelationshipResolver: Send + Sync {
    /// Name of this resolver (for logging)
    fn name(&self) -> &'static str;

    /// Extract relationships using cached entity data
    ///
    /// Returns Vec<(from_id, to_id, relationship_type)>
    async fn resolve(&self, cache: &EntityCache) -> Result<Vec<(String, String, String)>>;
}

/// Generic function to resolve relationships using a resolver implementation
///
/// This function provides the common infrastructure for all relationship resolvers:
/// 1. Calls the resolver's `resolve()` method to extract relationships from cache
/// 2. Batch creates all relationships in Neo4j
/// 3. Logs progress and results
///
/// # Prerequisites
/// The caller MUST ensure the Neo4j database is already selected via `use_database()`
/// before calling this function. This is typically done once per repository in
/// `resolve_pending_relationships()`.
///
/// # Arguments
/// * `cache` - Pre-populated entity cache for the repository
/// * `neo4j` - Neo4j client for creating relationships (must have database already selected)
/// * `resolver` - Implementation of the RelationshipResolver trait
///
/// # Example
/// ```ignore
/// // Caller must select database and create cache first
/// neo4j.use_database(&db_name).await?;
/// let cache = EntityCache::new(&postgres, repository_id).await?;
/// let resolver = TraitImplResolver;
/// resolve_relationships_generic(&cache, &neo4j, &resolver).await?;
/// ```
pub async fn resolve_relationships_generic(
    cache: &EntityCache,
    neo4j: &dyn Neo4jClientTrait,
    resolver: &dyn RelationshipResolver,
) -> Result<()> {
    info!("Resolving {} relationships...", resolver.name());

    // Resolve relationships using cached entity data
    let relationships = resolver.resolve(cache).await?;

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

    async fn resolve(&self, cache: &EntityCache) -> Result<Vec<(String, String, String)>> {
        // Get entities from cache
        let impls = cache.by_type(EntityType::Impl);
        let traits = cache.by_type(EntityType::Trait);
        let structs = cache.by_type(EntityType::Struct);
        let enums = cache.by_type(EntityType::Enum);
        let interfaces = cache.by_type(EntityType::Interface);

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

        // EXTENDS_INTERFACE relationships for Rust trait supertraits
        // E.g., `trait Extended: Base {}` creates Extended -> Base relationship
        // Uses pre-resolved supertraits attribute which contains qualified supertrait names
        for trait_entity in &traits {
            if let Some(supertraits_json) = trait_entity.metadata.attributes.get("supertraits") {
                let supertraits: Vec<String> = match serde_json::from_str(supertraits_json) {
                    Ok(t) => t,
                    Err(e) => {
                        warn!(
                            "Failed to parse 'supertraits' JSON for trait {}: {}",
                            trait_entity.entity_id, e
                        );
                        continue;
                    }
                };
                for supertrait in supertraits {
                    // Skip lifetimes (start with ')
                    if supertrait.starts_with('\'') {
                        continue;
                    }
                    // Strip generics from supertrait name
                    let base_trait = supertrait.split('<').next().unwrap_or(&supertrait).trim();
                    if let Some(supertrait_id) = trait_map.get(base_trait) {
                        // Forward edge: trait -> supertrait
                        relationships.push((
                            trait_entity.entity_id.clone(),
                            supertrait_id.clone(),
                            "EXTENDS_INTERFACE".to_string(),
                        ));
                        // Reciprocal edge: supertrait -> trait
                        relationships.push((
                            supertrait_id.clone(),
                            trait_entity.entity_id.clone(),
                            "EXTENDED_BY".to_string(),
                        ));
                    }
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

    async fn resolve(&self, cache: &EntityCache) -> Result<Vec<(String, String, String)>> {
        let classes = cache.by_type(EntityType::Class);

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
                    match serde_json::from_str::<Vec<String>>(bases_json) {
                        Ok(bases) => bases,
                        Err(e) => {
                            warn!(
                                "Failed to parse 'bases' JSON for entity {}: {}",
                                class_entity.entity_id, e
                            );
                            continue;
                        }
                    }
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

    async fn resolve(&self, cache: &EntityCache) -> Result<Vec<(String, String, String)>> {
        let structs = cache.by_type(EntityType::Struct);
        let functions = cache.by_type(EntityType::Function);
        let methods = cache.by_type(EntityType::Method);
        let all_types = cache.all_types();

        // Build type lookup map (qualified_name -> entity_id) for correct resolution
        let type_map: HashMap<String, String> = all_types
            .iter()
            .map(|t| (t.qualified_name.clone(), t.entity_id.clone()))
            .collect();

        let mut relationships = Vec::new();

        // Process struct field types using pre-resolved uses_types attribute
        for struct_entity in structs {
            if let Some(uses_types_json) = struct_entity.metadata.attributes.get("uses_types") {
                let types: Vec<String> = match serde_json::from_str(uses_types_json) {
                    Ok(t) => t,
                    Err(e) => {
                        warn!(
                            "Failed to parse 'uses_types' JSON for struct {}: {}",
                            struct_entity.entity_id, e
                        );
                        continue;
                    }
                };
                for type_name in types {
                    // Strip generics and get the base type name
                    let base_type = type_name.split('<').next().unwrap_or(&type_name).trim();

                    if let Some(type_id) = type_map.get(base_type) {
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

        // Process enum variant types using pre-resolved uses_types attribute
        let enums = cache.by_type(EntityType::Enum);
        for enum_entity in enums {
            if let Some(uses_types_json) = enum_entity.metadata.attributes.get("uses_types") {
                let types: Vec<String> = match serde_json::from_str(uses_types_json) {
                    Ok(t) => t,
                    Err(e) => {
                        warn!(
                            "Failed to parse 'uses_types' JSON for enum {}: {}",
                            enum_entity.entity_id, e
                        );
                        continue;
                    }
                };
                for type_name in types {
                    // Strip generics and get the base type name
                    let base_type = type_name.split('<').next().unwrap_or(&type_name).trim();

                    if let Some(type_id) = type_map.get(base_type) {
                        // Forward edge: enum -> type
                        relationships.push((
                            enum_entity.entity_id.clone(),
                            type_id.clone(),
                            "USES".to_string(),
                        ));
                        // Reciprocal edge: type -> enum
                        relationships.push((
                            type_id.clone(),
                            enum_entity.entity_id.clone(),
                            "USED_BY".to_string(),
                        ));
                    }
                }
            }
        }

        // Process function and method uses_types
        let callables: Vec<_> = functions.into_iter().chain(methods).collect();
        for callable in callables {
            if let Some(uses_types_json) = callable.metadata.attributes.get("uses_types") {
                let types = match serde_json::from_str::<Vec<SourceReference>>(uses_types_json) {
                    Ok(t) => t,
                    Err(e) => {
                        warn!(
                            "Failed to parse 'uses_types' JSON for entity {}: {}",
                            callable.entity_id, e
                        );
                        continue;
                    }
                };
                for type_ref in types {
                    // Strip generics and get the base type name
                    let type_name = type_ref
                        .target
                        .split('<')
                        .next()
                        .unwrap_or(&type_ref.target)
                        .trim();

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

    async fn resolve(&self, cache: &EntityCache) -> Result<Vec<(String, String, String)>> {
        let functions = cache.by_type(EntityType::Function);
        let methods = cache.by_type(EntityType::Method);

        let all_callables: Vec<_> = functions.into_iter().chain(methods).collect();

        // Build lookup map using only qualified_name for correct resolution
        // Using simple name would cause collisions between functions with the same name
        // in different modules
        let callable_map: HashMap<String, String> = all_callables
            .iter()
            .map(|c| (c.qualified_name.clone(), c.entity_id.clone()))
            .collect();

        // Build secondary lookup for trait impl methods
        // Maps "TypeFQN::method" -> entity_id for methods like "<TypeFQN as TraitFQN>::method"
        // This allows resolution of calls like `value.method()` where the method is from a trait impl
        let trait_impl_map: HashMap<String, String> = all_callables
            .iter()
            .filter_map(|c| {
                // Parse trait impl method names like "<test_crate::IntProducer as test_crate::Producer>::produce"
                parse_trait_impl_short_form(&c.qualified_name)
                    .map(|short_form| (short_form, c.entity_id.clone()))
            })
            .collect();

        // Build simple name map for re-export fallback resolution
        // For each simple name, store the entity_id if there's only one match (to avoid collisions)
        let mut simple_name_counts: HashMap<String, usize> = HashMap::new();
        let mut simple_name_map: HashMap<String, String> = HashMap::new();
        for c in &all_callables {
            *simple_name_counts.entry(c.name.clone()).or_insert(0) += 1;
            simple_name_map.insert(c.name.clone(), c.entity_id.clone());
        }
        // Remove entries with multiple matches (ambiguous)
        simple_name_map.retain(|name, _| simple_name_counts.get(name) == Some(&1));

        let mut relationships = Vec::new();

        // DEBUG: Log callable_map contents
        debug!(
            "CallGraphResolver: callable_map has {} entries",
            callable_map.len()
        );
        for (qname, _) in callable_map.iter() {
            trace!("  callable: {}", qname);
        }

        for caller in all_callables {
            if let Some(calls_json) = caller.metadata.attributes.get("calls") {
                debug!(
                    "CallGraphResolver: {} has calls attribute: {}",
                    caller.qualified_name, calls_json
                );
                let calls = match serde_json::from_str::<Vec<SourceReference>>(calls_json) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(
                            "Failed to parse 'calls' JSON for entity {}: {}",
                            caller.entity_id, e
                        );
                        continue;
                    }
                };
                for call_ref in calls {
                    trace!(
                        "  call target: {} (in callable_map: {}, in trait_impl_map: {})",
                        call_ref.target,
                        callable_map.contains_key(&call_ref.target),
                        trait_impl_map.contains_key(&call_ref.target)
                    );
                    // Try direct match first
                    let callee_id = callable_map
                        .get(&call_ref.target)
                        // Fall back to trait impl short form lookup
                        .or_else(|| trait_impl_map.get(&call_ref.target))
                        // Fall back to simple name lookup for re-exports
                        // Extract the last segment of the path as the simple name
                        .or_else(|| {
                            let simple_name = call_ref.target.rsplit("::").next()?;
                            simple_name_map.get(simple_name)
                        });

                    if let Some(callee_id) = callee_id {
                        trace!("  -> resolved to callee_id: {}", callee_id);
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
                    } else {
                        trace!("  -> NOT FOUND in any lookup map");
                    }
                }
            }
        }

        Ok(relationships)
    }
}

/// Resolver for imports (IMPORTS relationships)
///
/// Creates import relationships from any entity with an `imports` attribute
/// to the entities they import. External imports are handled separately
/// by resolve_external_references.
pub struct ImportsResolver;

#[async_trait]
impl RelationshipResolver for ImportsResolver {
    fn name(&self) -> &'static str {
        "imports"
    }

    async fn resolve(&self, cache: &EntityCache) -> Result<Vec<(String, String, String)>> {
        // Build lookup map by qualified_name for all entities
        let qname_map: HashMap<&str, &str> = cache
            .all()
            .iter()
            .map(|e| (e.qualified_name.as_str(), e.entity_id.as_str()))
            .collect();

        // Also build a simple name map for fallback resolution
        let simple_name_map: HashMap<&str, &str> = cache
            .all()
            .iter()
            .map(|e| (e.name.as_str(), e.entity_id.as_str()))
            .collect();

        let mut relationships = Vec::new();

        // Process all entities that have imports
        for entity in cache.all() {
            if let Some(imports_json) = entity.metadata.attributes.get("imports") {
                let imports = match serde_json::from_str::<Vec<String>>(imports_json) {
                    Ok(i) => i,
                    Err(e) => {
                        warn!(
                            "Failed to parse 'imports' JSON for entity {}: {}",
                            entity.entity_id, e
                        );
                        continue;
                    }
                };

                for import_path in imports {
                    // Try to resolve the import:
                    // 1. Exact qualified name match
                    // 2. Simple name fallback (for internal package imports)
                    let imported_id = qname_map.get(import_path.as_str()).or_else(|| {
                        // Try matching just the last segment (simple name)
                        let simple_name = import_path
                            .rsplit("::")
                            .next()
                            .or_else(|| import_path.rsplit('.').next())
                            .unwrap_or(&import_path);
                        simple_name_map.get(simple_name)
                    });

                    if let Some(imported_id) = imported_id {
                        // Skip self-imports
                        if *imported_id == entity.entity_id {
                            continue;
                        }

                        // Forward edge: entity -> imported_entity
                        relationships.push((
                            entity.entity_id.clone(),
                            (*imported_id).to_string(),
                            "IMPORTS".to_string(),
                        ));
                        // Reciprocal edge: imported_entity -> entity
                        relationships.push((
                            (*imported_id).to_string(),
                            entity.entity_id.clone(),
                            "IMPORTED_BY".to_string(),
                        ));
                    }
                    // Note: Unresolved imports (external) are handled by resolve_external_references
                }
            }
        }

        Ok(relationships)
    }
}

/// Resolver for containment (CONTAINS relationships)
///
/// Creates parent-child relationships based on entity.parent_scope.
/// Uses pre-built qualified_name -> entity_id map from cache.
pub struct ContainsResolver;

#[async_trait]
impl RelationshipResolver for ContainsResolver {
    fn name(&self) -> &'static str {
        "containment"
    }

    async fn resolve(&self, cache: &EntityCache) -> Result<Vec<(String, String, String)>> {
        let entities = cache.all();
        let qname_to_id = cache.qname_map();

        let mut relationships = Vec::new();

        for entity in entities {
            if let Some(parent_scope) = &entity.parent_scope {
                // Look up parent by qualified_name
                if let Some(parent_id) = qname_to_id.get(parent_scope) {
                    // Forward edge: parent CONTAINS child
                    relationships.push((
                        parent_id.clone(),
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
    cache: &EntityCache,
    neo4j: &dyn Neo4jClientTrait,
    repository_id: Uuid,
) -> Result<()> {
    use std::collections::HashSet;

    info!("Resolving external references...");

    if cache.is_empty() {
        return Ok(());
    }

    let entities = cache.all();

    // Build set of known qualified names from cache's pre-built map
    let known_names: HashSet<&str> = cache.qname_map().keys().map(|s| s.as_str()).collect();

    // Use cache's name map for simple name lookups
    let name_to_qname: HashMap<&str, &str> = cache
        .name_map()
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    // Collect all external references with their source relationships
    let mut external_refs: HashSet<ExternalRef> = HashSet::new();
    let mut relationships: Vec<(String, String, String)> = Vec::new();

    for entity in entities {
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

        // Check uses_types (JSON array of SourceReference)
        if let Some(uses_types_str) = entity.metadata.attributes.get("uses_types") {
            if let Ok(types) = serde_json::from_str::<Vec<SourceReference>>(uses_types_str) {
                for type_ref in types {
                    if is_external_ref(&type_ref.target, &known_names, &name_to_qname) {
                        let ext_ref = ExternalRef::new(normalize_external_ref(&type_ref.target));
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

        // Check calls (JSON array of SourceReference)
        if let Some(calls_str) = entity.metadata.attributes.get("calls") {
            if let Ok(calls) = serde_json::from_str::<Vec<SourceReference>>(calls_str) {
                for call_ref in calls {
                    if is_external_ref(&call_ref.target, &known_names, &name_to_qname) {
                        let ext_ref = ExternalRef::new(normalize_external_ref(&call_ref.target));
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

        // Check imports (JSON array of import paths)
        if let Some(imports_str) = entity.metadata.attributes.get("imports") {
            if let Ok(imports) = serde_json::from_str::<Vec<String>>(imports_str) {
                for import_ref in imports {
                    if is_external_ref(&import_ref, &known_names, &name_to_qname) {
                        let ext_ref = ExternalRef::new(normalize_external_ref(&import_ref));
                        let ext_id = ext_ref.entity_id();
                        external_refs.insert(ext_ref);
                        relationships.push((
                            entity.entity_id.clone(),
                            ext_id.clone(),
                            "IMPORTS".to_string(),
                        ));
                        relationships.push((
                            ext_id,
                            entity.entity_id.clone(),
                            "IMPORTED_BY".to_string(),
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

    // Note: resolve_relative_import tests are in codesearch_languages::common::import_map
    // These tests verify the external reference detection logic

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
