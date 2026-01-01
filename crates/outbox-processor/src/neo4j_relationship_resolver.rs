//! Neo4j relationship resolution framework
//!
//! This module provides the infrastructure for creating relationship edges in Neo4j.
//! The primary implementation uses `GenericResolver` (see `generic_resolver` module)
//! with typed `EntityRelationshipData` from each entity.
//!
//! # Architecture
//!
//! Relationship information is stored in the typed `EntityRelationshipData` struct:
//! - `calls`: Function/method call references (for CALLS)
//! - `uses_types`: Type usage references (for USES)
//! - `implements_trait`: Trait being implemented (for IMPLEMENTS)
//! - `for_type`: Type that impl block is for (for ASSOCIATES)
//! - `supertraits`: Trait supertraits (for EXTENDS_INTERFACE)
//! - `extends`: Parent class names (for INHERITS_FROM)
//! - `imports`: Imported module names (for IMPORTS)
//!
//! Structural relationships use entity fields directly:
//! - `parent_scope`: Qualified name of containing entity (for CONTAINS)
//!
//! Resolution is triggered once when indexing completes (drain mode) and queries
//! entity_metadata to build lookup maps and create Neo4j edges.
//!
//! # Core Components
//!
//! - `EntityCache`: Caches all entities and provides lookup maps
//! - `RelationshipResolver`: Trait for implementing resolvers
//! - `ContainsResolver`: CONTAINS relationships (structural, uses parent_scope)
//! - `GenericResolver`: Configurable resolver for all other relationship types
//!   (see `generic_resolver` module for factory functions like `calls_resolver()`)

use anyhow::Context;
use async_trait::async_trait;
use codesearch_core::entities::{CodeEntity, EntityType, SourceReference};
use codesearch_core::error::Result;
use codesearch_storage::{Neo4jClientTrait, PostgresClientTrait};
use std::collections::HashMap;
use tracing::info;
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
/// let resolver = ContainsResolver;
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
                relationships.push((entity.entity_id.clone(), ext_id, "IMPLEMENTS".to_string()));
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
                    ext_id,
                    "INHERITS_FROM".to_string(),
                ));
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
                        relationships.push((entity.entity_id.clone(), ext_id, "USES".to_string()));
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
                        relationships.push((entity.entity_id.clone(), ext_id, "CALLS".to_string()));
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
                            ext_id,
                            "IMPORTS".to_string(),
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
