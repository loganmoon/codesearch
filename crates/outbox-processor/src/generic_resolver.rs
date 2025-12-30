//! Generic relationship resolver using typed relationship definitions
//!
//! This module provides a configurable resolver that can handle any relationship type
//! by using the `RelationshipDef` and `LookupStrategy` types from core. This replaces
//! the need for separate resolver implementations per relationship type.
//!
//! # Architecture
//!
//! The `GenericResolver` is parameterized by a `RelationshipDef` which configures:
//! - Source and target entity types
//! - Forward and reciprocal relationship names
//! - Lookup strategies to try (in order)
//!
//! Relationship data is extracted from the typed `EntityRelationshipData` field
//! on each entity, eliminating JSON parsing at resolution time.

#![deny(warnings)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]

use async_trait::async_trait;
use codesearch_core::entities::{CodeEntity, SourceReference};
use codesearch_core::error::Result;
use codesearch_core::resolution::{LookupStrategy, RelationshipDef};
use std::collections::HashMap;
use tracing::{debug, trace, warn};

use crate::neo4j_relationship_resolver::{EntityCache, RelationshipResolver};

/// Lookup maps for resolving entity references
///
/// Contains multiple lookup strategies for finding target entities
/// during relationship resolution.
struct TargetLookupMaps {
    /// Qualified name -> entity_id
    qname: HashMap<String, String>,
    /// Path entity identifier -> entity_id
    path_id: HashMap<String, String>,
    /// Call alias -> entity_id
    call_alias: HashMap<String, String>,
    /// Simple name -> entity_id (only for unique names)
    unique_simple_name: HashMap<String, String>,
    /// Simple name -> all entity_ids (for uniqueness check)
    all_simple_names: HashMap<String, Vec<String>>,
}

/// A relationship reference extracted from an entity
#[derive(Debug, Clone)]
pub struct RelationshipRef {
    /// Target reference (qualified name or partial reference to resolve)
    pub target: String,
}

impl From<&SourceReference> for RelationshipRef {
    fn from(sr: &SourceReference) -> Self {
        Self {
            target: sr.target.clone(),
        }
    }
}

impl From<&String> for RelationshipRef {
    fn from(s: &String) -> Self {
        Self { target: s.clone() }
    }
}

/// Trait for extracting relationship references from entities
///
/// Implementations handle the mapping from specific relationship kinds
/// to the appropriate field in `EntityRelationshipData`.
pub trait ReferenceExtractor: Send + Sync {
    /// Extract references for this relationship type from the entity
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<RelationshipRef>;
}

/// Extractor for CALLS relationships
pub struct CallsExtractor;

impl ReferenceExtractor for CallsExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<RelationshipRef> {
        entity
            .relationships
            .calls
            .iter()
            .map(RelationshipRef::from)
            .collect()
    }
}

/// Extractor for USES relationships
pub struct UsesExtractor;

impl ReferenceExtractor for UsesExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<RelationshipRef> {
        entity
            .relationships
            .uses_types
            .iter()
            .map(RelationshipRef::from)
            .collect()
    }
}

/// Extractor for IMPLEMENTS relationships
pub struct ImplementsExtractor;

impl ReferenceExtractor for ImplementsExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<RelationshipRef> {
        entity
            .relationships
            .implements_trait
            .as_ref()
            .map(|t| vec![RelationshipRef::from(t)])
            .unwrap_or_default()
    }
}

/// Extractor for ASSOCIATES relationships
pub struct AssociatesExtractor;

impl ReferenceExtractor for AssociatesExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<RelationshipRef> {
        entity
            .relationships
            .for_type
            .as_ref()
            .map(|t| vec![RelationshipRef::from(t)])
            .unwrap_or_default()
    }
}

/// Extractor for EXTENDS (supertraits) relationships
pub struct SupertraitsExtractor;

impl ReferenceExtractor for SupertraitsExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<RelationshipRef> {
        entity
            .relationships
            .supertraits
            .iter()
            .filter(|s| !s.starts_with('\'')) // Skip lifetimes
            .map(RelationshipRef::from)
            .collect()
    }
}

/// Extractor for INHERITS (class inheritance) relationships
pub struct InheritsExtractor;

impl ReferenceExtractor for InheritsExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<RelationshipRef> {
        entity
            .relationships
            .extends
            .iter()
            .map(RelationshipRef::from)
            .collect()
    }
}

/// Extractor for IMPORTS relationships
pub struct ImportsExtractor;

impl ReferenceExtractor for ImportsExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<RelationshipRef> {
        entity
            .relationships
            .imports
            .iter()
            .map(RelationshipRef::from)
            .collect()
    }
}

/// Generic relationship resolver that uses typed relationship definitions
///
/// This resolver can handle any relationship type by being configured with:
/// 1. A `RelationshipDef` that specifies source/target types and lookup strategies
/// 2. A `ReferenceExtractor` that knows how to extract references from entities
pub struct GenericResolver {
    /// The relationship definition (source/target types, forward/reciprocal names, strategies)
    def: &'static RelationshipDef,
    /// Extractor for getting references from entities
    extractor: Box<dyn ReferenceExtractor>,
}

impl GenericResolver {
    /// Create a new generic resolver with the given definition and extractor
    pub fn new(def: &'static RelationshipDef, extractor: Box<dyn ReferenceExtractor>) -> Self {
        Self { def, extractor }
    }

    /// Build lookup maps for the target entity types
    ///
    /// Returns maps based on the lookup strategies configured in the relationship definition.
    fn build_target_maps(&self, cache: &EntityCache) -> TargetLookupMaps {
        let mut qname_map = HashMap::new();
        let mut path_id_map = HashMap::new();
        let mut call_alias_map = HashMap::new();
        let mut simple_name_map = HashMap::new();
        let mut simple_name_counts: HashMap<String, Vec<String>> = HashMap::new();

        // Collect all target entities based on target_types
        let target_entities: Vec<&CodeEntity> = cache
            .all()
            .iter()
            .filter(|e| self.def.target_types.contains(&e.entity_type))
            .collect();

        for entity in target_entities {
            // Always build qname map
            qname_map.insert(entity.qualified_name.clone(), entity.entity_id.clone());

            // Build path_id map if PathEntityIdentifier strategy is used
            if self
                .def
                .lookup_strategies
                .contains(&LookupStrategy::PathEntityIdentifier)
            {
                if let Some(path_id) = &entity.path_entity_identifier {
                    path_id_map.insert(path_id.clone(), entity.entity_id.clone());
                }
            }

            // Build call_alias map if CallAliases strategy is used
            if self
                .def
                .lookup_strategies
                .contains(&LookupStrategy::CallAliases)
            {
                for alias in &entity.relationships.call_aliases {
                    call_alias_map.insert(alias.clone(), entity.entity_id.clone());
                }
            }

            // Track simple names for SimpleName/UniqueSimpleName strategies
            if self
                .def
                .lookup_strategies
                .contains(&LookupStrategy::SimpleName)
                || self
                    .def
                    .lookup_strategies
                    .contains(&LookupStrategy::UniqueSimpleName)
            {
                simple_name_counts
                    .entry(entity.name.clone())
                    .or_default()
                    .push(entity.entity_id.clone());
            }
        }

        // Build simple_name_map: only include entries with exactly one entity for UniqueSimpleName
        for (name, ids) in &simple_name_counts {
            if ids.len() == 1 {
                simple_name_map.insert(name.clone(), ids[0].clone());
            }
        }

        TargetLookupMaps {
            qname: qname_map,
            path_id: path_id_map,
            call_alias: call_alias_map,
            unique_simple_name: simple_name_map,
            all_simple_names: simple_name_counts,
        }
    }

    /// Resolve a reference using the configured lookup strategies
    fn resolve_reference(&self, reference: &str, maps: &TargetLookupMaps) -> Option<String> {
        // Strip generics for lookup
        let base_ref = reference.split('<').next().unwrap_or(reference).trim();

        for strategy in self.def.lookup_strategies {
            match strategy {
                LookupStrategy::QualifiedName => {
                    if let Some(id) = maps.qname.get(base_ref) {
                        trace!("  [QualifiedName] resolved {} -> {}", reference, id);
                        return Some(id.clone());
                    }
                }
                LookupStrategy::PathEntityIdentifier => {
                    if let Some(id) = maps.path_id.get(base_ref) {
                        trace!("  [PathEntityIdentifier] resolved {} -> {}", reference, id);
                        return Some(id.clone());
                    }
                }
                LookupStrategy::CallAliases => {
                    if let Some(id) = maps.call_alias.get(base_ref) {
                        trace!("  [CallAliases] resolved {} -> {}", reference, id);
                        return Some(id.clone());
                    }
                }
                LookupStrategy::UniqueSimpleName => {
                    // Extract simple name (last segment)
                    let simple_name = base_ref
                        .rsplit("::")
                        .next()
                        .or_else(|| base_ref.rsplit('.').next())
                        .unwrap_or(base_ref);

                    if let Some(id) = maps.unique_simple_name.get(simple_name) {
                        trace!("  [UniqueSimpleName] resolved {} -> {}", reference, id);
                        return Some(id.clone());
                    }
                }
                LookupStrategy::SimpleName => {
                    // Extract simple name (last segment)
                    let simple_name = base_ref
                        .rsplit("::")
                        .next()
                        .or_else(|| base_ref.rsplit('.').next())
                        .unwrap_or(base_ref);

                    // First match wins (may be ambiguous)
                    if let Some(ids) = maps.all_simple_names.get(simple_name) {
                        if !ids.is_empty() {
                            if ids.len() > 1 {
                                warn!(
                                    "  [SimpleName] ambiguous match for {}: {} candidates",
                                    reference,
                                    ids.len()
                                );
                            }
                            trace!("  [SimpleName] resolved {} -> {}", reference, &ids[0]);
                            return Some(ids[0].clone());
                        }
                    }
                }
            }
        }

        trace!("  [UNRESOLVED] {}", reference);
        None
    }
}

#[async_trait]
impl RelationshipResolver for GenericResolver {
    fn name(&self) -> &'static str {
        self.def.name
    }

    async fn resolve(&self, cache: &EntityCache) -> Result<Vec<(String, String, String)>> {
        // Build target lookup maps
        let maps = self.build_target_maps(cache);

        debug!(
            "GenericResolver[{}]: built maps - qname={}, path_id={}, call_alias={}, unique_simple={}",
            self.def.name,
            maps.qname.len(),
            maps.path_id.len(),
            maps.call_alias.len(),
            maps.unique_simple_name.len()
        );

        // Collect source entities
        let source_entities: Vec<&CodeEntity> = cache
            .all()
            .iter()
            .filter(|e| self.def.source_types.contains(&e.entity_type))
            .collect();

        debug!(
            "GenericResolver[{}]: processing {} source entities",
            self.def.name,
            source_entities.len()
        );

        let mut relationships = Vec::new();

        for source in source_entities {
            let refs = self.extractor.extract_refs(source);
            if refs.is_empty() {
                continue;
            }

            trace!(
                "GenericResolver[{}]: {} has {} references",
                self.def.name,
                source.qualified_name,
                refs.len()
            );

            for rel_ref in refs {
                if let Some(target_id) = self.resolve_reference(&rel_ref.target, &maps) {
                    // Skip self-references
                    if target_id == source.entity_id {
                        continue;
                    }

                    // Forward edge
                    relationships.push((
                        source.entity_id.clone(),
                        target_id.clone(),
                        self.def.forward_rel.to_string(),
                    ));

                    // Reciprocal edge (if defined)
                    if let Some(reciprocal) = self.def.reciprocal_rel {
                        relationships.push((
                            target_id,
                            source.entity_id.clone(),
                            reciprocal.to_string(),
                        ));
                    }
                } else {
                    debug!(
                        "GenericResolver[{}]: unresolved reference {} -> {}",
                        self.def.name, source.qualified_name, rel_ref.target
                    );
                }
            }
        }

        Ok(relationships)
    }
}

/// Create a GenericResolver for CALLS relationships
pub fn calls_resolver() -> GenericResolver {
    GenericResolver::new(
        &codesearch_core::resolution::definitions::CALLS,
        Box::new(CallsExtractor),
    )
}

/// Create a GenericResolver for USES relationships
pub fn uses_resolver() -> GenericResolver {
    GenericResolver::new(
        &codesearch_core::resolution::definitions::USES,
        Box::new(UsesExtractor),
    )
}

/// Create a GenericResolver for IMPLEMENTS relationships
pub fn implements_resolver() -> GenericResolver {
    GenericResolver::new(
        &codesearch_core::resolution::definitions::IMPLEMENTS,
        Box::new(ImplementsExtractor),
    )
}

/// Create a GenericResolver for ASSOCIATES relationships
pub fn associates_resolver() -> GenericResolver {
    GenericResolver::new(
        &codesearch_core::resolution::definitions::ASSOCIATES,
        Box::new(AssociatesExtractor),
    )
}

/// Create a GenericResolver for EXTENDS (supertraits) relationships
pub fn extends_resolver() -> GenericResolver {
    GenericResolver::new(
        &codesearch_core::resolution::definitions::EXTENDS,
        Box::new(SupertraitsExtractor),
    )
}

/// Create a GenericResolver for INHERITS relationships
pub fn inherits_resolver() -> GenericResolver {
    GenericResolver::new(
        &codesearch_core::resolution::definitions::INHERITS,
        Box::new(InheritsExtractor),
    )
}

/// Create a GenericResolver for IMPORTS relationships
pub fn imports_resolver() -> GenericResolver {
    GenericResolver::new(
        &codesearch_core::resolution::definitions::IMPORTS,
        Box::new(ImportsExtractor),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesearch_core::entities::{
        CodeEntityBuilder, EntityMetadata, EntityRelationshipData, EntityType, Language,
        SourceLocation, SourceReference,
    };
    use std::path::PathBuf;

    fn make_test_entity(
        entity_type: EntityType,
        name: &str,
        qualified_name: &str,
        relationships: EntityRelationshipData,
    ) -> CodeEntity {
        CodeEntityBuilder::default()
            .entity_id(format!("id_{}", name))
            .repository_id("repo_1".to_string())
            .name(name.to_string())
            .qualified_name(qualified_name.to_string())
            .entity_type(entity_type)
            .file_path(PathBuf::from("/test.rs"))
            .location(SourceLocation {
                start_line: 1,
                end_line: 10,
                start_column: 0,
                end_column: 0,
            })
            .content("test content".to_string())
            .language(Language::Rust)
            .metadata(EntityMetadata::default())
            .relationships(relationships)
            .build()
            .ok()
            .unwrap()
    }

    #[test]
    fn test_calls_extractor() {
        let extractor = CallsExtractor;
        let entity = make_test_entity(
            EntityType::Function,
            "foo",
            "crate::foo",
            EntityRelationshipData {
                calls: vec![
                    SourceReference::new(
                        "crate::bar".to_string(),
                        SourceLocation {
                            start_line: 5,
                            end_line: 5,
                            start_column: 0,
                            end_column: 10,
                        },
                        codesearch_core::ReferenceType::Call,
                    ),
                    SourceReference::new(
                        "crate::baz".to_string(),
                        SourceLocation {
                            start_line: 6,
                            end_line: 6,
                            start_column: 0,
                            end_column: 10,
                        },
                        codesearch_core::ReferenceType::Call,
                    ),
                ],
                ..Default::default()
            },
        );

        let refs = extractor.extract_refs(&entity);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].target, "crate::bar");
        assert_eq!(refs[1].target, "crate::baz");
    }

    #[test]
    fn test_uses_extractor() {
        let extractor = UsesExtractor;
        let entity = make_test_entity(
            EntityType::Function,
            "foo",
            "crate::foo",
            EntityRelationshipData {
                uses_types: vec![SourceReference::new(
                    "crate::MyStruct".to_string(),
                    SourceLocation {
                        start_line: 2,
                        end_line: 2,
                        start_column: 0,
                        end_column: 10,
                    },
                    codesearch_core::ReferenceType::TypeUsage,
                )],
                ..Default::default()
            },
        );

        let refs = extractor.extract_refs(&entity);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target, "crate::MyStruct");
    }

    #[test]
    fn test_implements_extractor() {
        let extractor = ImplementsExtractor;
        let entity = make_test_entity(
            EntityType::Impl,
            "impl",
            "crate::impl_MyTrait_for_MyStruct",
            EntityRelationshipData {
                implements_trait: Some("crate::MyTrait".to_string()),
                ..Default::default()
            },
        );

        let refs = extractor.extract_refs(&entity);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target, "crate::MyTrait");
    }

    #[test]
    fn test_supertraits_extractor_skips_lifetimes() {
        let extractor = SupertraitsExtractor;
        let entity = make_test_entity(
            EntityType::Trait,
            "MyTrait",
            "crate::MyTrait",
            EntityRelationshipData {
                supertraits: vec![
                    "'static".to_string(),
                    "crate::BaseTrait".to_string(),
                    "'a".to_string(),
                ],
                ..Default::default()
            },
        );

        let refs = extractor.extract_refs(&entity);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target, "crate::BaseTrait");
    }

    #[test]
    fn test_imports_extractor() {
        let extractor = ImportsExtractor;
        let entity = make_test_entity(
            EntityType::Module,
            "mod",
            "crate::mod",
            EntityRelationshipData {
                imports: vec!["crate::other::Thing".to_string()],
                ..Default::default()
            },
        );

        let refs = extractor.extract_refs(&entity);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target, "crate::other::Thing");
    }
}
