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
use codesearch_core::entities::CodeEntity;
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

        Ok(Self {
            entities,
            qname_to_id,
        })
    }

    /// Get all entities
    pub fn all(&self) -> &[CodeEntity] {
        &self.entities
    }

    /// Get qualified_name -> entity_id lookup map
    pub fn qname_map(&self) -> &HashMap<String, String> {
        &self.qname_to_id
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
/// External references are identified by the `is_external` flag on SourceReference fields.
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

    // Collect all external references with their source relationships
    let mut external_refs: HashSet<ExternalRef> = HashSet::new();
    let mut relationships: Vec<(String, String, String)> = Vec::new();

    for entity in entities {
        // Check implements_trait (typed SourceReference with is_external flag)
        if let Some(ref trait_ref) = entity.relationships.implements_trait {
            if trait_ref.is_external() {
                let ext_ref = ExternalRef::new(normalize_external_ref(trait_ref.target()));
                let ext_id = ext_ref.entity_id();
                external_refs.insert(ext_ref);
                relationships.push((entity.entity_id.clone(), ext_id, "IMPLEMENTS".to_string()));
            }
        }

        // Check for_type (for Associates relationships on impl blocks)
        if let Some(ref for_type_ref) = entity.relationships.for_type {
            if for_type_ref.is_external() {
                let ext_ref = ExternalRef::new(normalize_external_ref(for_type_ref.target()));
                let ext_id = ext_ref.entity_id();
                external_refs.insert(ext_ref);
                relationships.push((entity.entity_id.clone(), ext_id, "ASSOCIATES".to_string()));
            }
        }

        // Check extends (for classes/interfaces - typed SourceReference)
        for extend_ref in &entity.relationships.extends {
            if extend_ref.is_external() {
                let ext_ref = ExternalRef::new(normalize_external_ref(extend_ref.target()));
                let ext_id = ext_ref.entity_id();
                external_refs.insert(ext_ref);
                relationships.push((
                    entity.entity_id.clone(),
                    ext_id,
                    "INHERITS_FROM".to_string(),
                ));
            }
        }

        // Check supertraits (for Rust traits - typed SourceReference)
        for supertrait_ref in &entity.relationships.supertraits {
            if supertrait_ref.is_external() {
                let ext_ref = ExternalRef::new(normalize_external_ref(supertrait_ref.target()));
                let ext_id = ext_ref.entity_id();
                external_refs.insert(ext_ref);
                relationships.push((
                    entity.entity_id.clone(),
                    ext_id,
                    "EXTENDS_INTERFACE".to_string(),
                ));
            }
        }

        // Check uses_types (typed SourceReference with is_external flag)
        for type_ref in &entity.relationships.uses_types {
            if type_ref.is_external() {
                let ext_ref = ExternalRef::new(normalize_external_ref(type_ref.target()));
                let ext_id = ext_ref.entity_id();
                external_refs.insert(ext_ref);
                relationships.push((entity.entity_id.clone(), ext_id, "USES".to_string()));
            }
        }

        // Check calls (typed SourceReference with is_external flag)
        for call_ref in &entity.relationships.calls {
            if call_ref.is_external() {
                let ext_ref = ExternalRef::new(normalize_external_ref(call_ref.target()));
                let ext_id = ext_ref.entity_id();
                external_refs.insert(ext_ref);
                relationships.push((entity.entity_id.clone(), ext_id, "CALLS".to_string()));
            }
        }

        // Check imports (typed SourceReference with is_external flag)
        for import_ref in &entity.relationships.imports {
            if import_ref.is_external() {
                let ext_ref = ExternalRef::new(normalize_external_ref(import_ref.target()));
                let ext_id = ext_ref.entity_id();
                external_refs.insert(ext_ref);
                relationships.push((entity.entity_id.clone(), ext_id, "IMPORTS".to_string()));
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
    fn test_normalize_external_ref() {
        // Strip crate:: prefix
        assert_eq!(
            normalize_external_ref("crate::utils::helpers"),
            "external::utils::helpers"
        );

        // Keep external:: prefix
        assert_eq!(
            normalize_external_ref("external::std::collections"),
            "external::std::collections"
        );

        // Add external:: prefix to bare refs
        assert_eq!(
            normalize_external_ref("std::collections::HashMap"),
            "external::std::collections::HashMap"
        );

        // Strip generic parameters
        assert_eq!(normalize_external_ref("Vec<T>"), "external::Vec");
    }
}
