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
//! - `implements_trait`: Trait being implemented by Rust impl blocks (for IMPLEMENTS)
//! - `implements`: Interfaces implemented by TS/JS classes (for IMPLEMENTS)
//! - `for_type`: Type that impl block is for (for ASSOCIATES)
//! - `extended_types`: Extended types - Rust trait bounds, TS interface extends (for EXTENDS_INTERFACE)
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
            .map(|e| (e.qualified_name.to_string(), e.entity_id.clone()))
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
    /// Returns a `ResolverOutput` containing:
    /// - `relationships`: Vec of (from_id, to_id, relationship_type) tuples
    /// - `external_refs`: HashSet of external references discovered during resolution
    async fn resolve(&self, cache: &EntityCache) -> Result<ResolverOutput>;
}

/// Generic function to resolve relationships using a resolver implementation
///
/// This function provides the common infrastructure for all relationship resolvers:
/// 1. Calls the resolver's `resolve()` method to extract relationships from cache
/// 2. Batch creates all relationships in Neo4j
/// 3. Accumulates external refs for batch creation later
/// 4. Logs progress and results
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
/// * `external_refs` - Accumulator for external refs (for batch creation later)
///
/// # Example
/// ```ignore
/// // Caller must select database and create cache first
/// neo4j.use_database(&db_name).await?;
/// let cache = EntityCache::new(&postgres, repository_id).await?;
/// let mut external_refs = HashSet::new();
/// let resolver = ContainsResolver;
/// resolve_relationships_generic(&cache, &neo4j, &resolver, &mut external_refs).await?;
/// ```
pub async fn resolve_relationships_generic(
    cache: &EntityCache,
    neo4j: &dyn Neo4jClientTrait,
    resolver: &dyn RelationshipResolver,
    external_refs: &mut std::collections::HashSet<ExternalRef>,
) -> Result<()> {
    info!("Resolving {} relationships...", resolver.name());

    // Resolve relationships using cached entity data
    let output = resolver.resolve(cache).await?;

    // Accumulate external refs for batch creation later
    external_refs.extend(output.external_refs);

    // Batch create all relationships
    if !output.relationships.is_empty() {
        neo4j
            .batch_create_relationships(&output.relationships)
            .await
            .with_context(|| format!("Failed to batch create {} relationships", resolver.name()))?;
    }

    info!(
        "Resolved {} {} relationships",
        output.relationships.len(),
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

    async fn resolve(&self, cache: &EntityCache) -> Result<ResolverOutput> {
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

        // CONTAINS has no external references
        Ok(ResolverOutput::relationships_only(relationships))
    }
}

// ============================================================================
// Resolver Output
// ============================================================================

/// Output from a relationship resolver containing both relationships and external refs
///
/// This unified output type allows resolvers to handle both internal and external
/// references in a single pass, eliminating the need for separate external resolution.
#[derive(Debug, Default)]
pub struct ResolverOutput {
    /// Relationships to create: (from_id, to_id, relationship_type)
    pub relationships: Vec<(String, String, String)>,
    /// External references discovered during resolution (deduplicated later)
    pub external_refs: std::collections::HashSet<ExternalRef>,
}

impl ResolverOutput {
    /// Create a new empty resolver output
    pub fn new() -> Self {
        Self::default()
    }

    /// Create resolver output with only relationships (no external refs)
    pub fn relationships_only(relationships: Vec<(String, String, String)>) -> Self {
        Self {
            relationships,
            external_refs: std::collections::HashSet::new(),
        }
    }
}

// ============================================================================
// External Reference Resolution
// ============================================================================

/// External reference collected from entity attributes
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExternalRef {
    /// Qualified name of the external reference
    pub qualified_name: String,
    /// Package/crate name (extracted from first segment)
    pub package: Option<String>,
}

impl ExternalRef {
    /// Create a new external reference with the given qualified name
    pub fn new(qualified_name: String) -> Self {
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
    pub fn entity_id(&self) -> String {
        format!(
            "external::{}",
            self.qualified_name.trim_start_matches("external::")
        )
    }
}

/// Normalize an external reference name
///
/// Strips `crate::` prefix, removes generic parameters, and ensures
/// the result starts with `external::`.
pub fn normalize_external_ref(ref_name: &str) -> String {
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
