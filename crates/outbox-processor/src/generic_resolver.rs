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
use codesearch_core::entities::{CodeEntity, RelationshipType};
use codesearch_core::error::Result;
use codesearch_core::resolution::{LookupStrategy, RelationshipDef};
use std::collections::HashMap;
use tracing::{debug, trace, warn};

use crate::neo4j_relationship_resolver::{EntityCache, RelationshipResolver};

/// A reference extracted from an entity, with pre-computed simple name.
#[derive(Debug, Clone)]
struct ExtractedRef {
    /// The fully qualified target reference
    target: String,
    /// Pre-computed simple name (last path segment)
    simple_name: String,
}

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

/// Trait for extracting relationship target references from entities
///
/// Implementations handle the mapping from specific relationship kinds
/// to the appropriate field in `EntityRelationshipData`.
trait ReferenceExtractor: Send + Sync {
    /// Extract target references for this relationship type from the entity.
    /// Returns ExtractedRef structs with pre-computed simple names.
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<ExtractedRef>;
}

/// Extractor for CALLS relationships
struct CallsExtractor;

impl ReferenceExtractor for CallsExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<ExtractedRef> {
        entity
            .relationships
            .calls
            .iter()
            .map(|sr| ExtractedRef {
                target: sr.target().to_string(),
                simple_name: sr.simple_name().to_string(),
            })
            .collect()
    }
}

/// Extractor for USES relationships
struct UsesExtractor;

impl ReferenceExtractor for UsesExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<ExtractedRef> {
        entity
            .relationships
            .uses_types
            .iter()
            .map(|sr| ExtractedRef {
                target: sr.target().to_string(),
                simple_name: sr.simple_name().to_string(),
            })
            .collect()
    }
}

/// Extractor for IMPLEMENTS relationships
/// Handles both Rust impl blocks (implements_trait) and TypeScript classes (implements)
struct ImplementsExtractor;

impl ReferenceExtractor for ImplementsExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<ExtractedRef> {
        let mut refs = Vec::new();

        // Rust impl blocks: implements_trait (single trait)
        if let Some(src_ref) = entity.relationships.implements_trait.as_ref() {
            refs.push(ExtractedRef {
                target: src_ref.target().to_string(),
                simple_name: src_ref.simple_name().to_string(),
            });
        }

        // TypeScript/JavaScript classes: implements (multiple interfaces)
        for src_ref in &entity.relationships.implements {
            refs.push(ExtractedRef {
                target: src_ref.target().to_string(),
                simple_name: src_ref.simple_name().to_string(),
            });
        }

        refs
    }
}

/// Extractor for ASSOCIATES relationships
struct AssociatesExtractor;

impl ReferenceExtractor for AssociatesExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<ExtractedRef> {
        entity
            .relationships
            .for_type
            .as_ref()
            .map(|src_ref| {
                vec![ExtractedRef {
                    target: src_ref.target().to_string(),
                    simple_name: src_ref.simple_name().to_string(),
                }]
            })
            .unwrap_or_default()
    }
}

/// Extractor for EXTENDS (supertraits) relationships
struct SupertraitsExtractor;

impl ReferenceExtractor for SupertraitsExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<ExtractedRef> {
        // NOTE: Lifetimes are now excluded at extraction time (tree-sitter query),
        // so no filtering is needed here
        entity
            .relationships
            .supertraits
            .iter()
            .map(|src_ref| ExtractedRef {
                target: src_ref.target().to_string(),
                simple_name: src_ref.simple_name().to_string(),
            })
            .collect()
    }
}

/// Extractor for INHERITS (class inheritance) relationships
struct InheritsExtractor;

impl ReferenceExtractor for InheritsExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<ExtractedRef> {
        entity
            .relationships
            .extends
            .iter()
            .map(|src_ref| ExtractedRef {
                target: src_ref.target().to_string(),
                simple_name: src_ref.simple_name().to_string(),
            })
            .collect()
    }
}

/// Extractor for IMPORTS relationships
struct ImportsExtractor;

impl ReferenceExtractor for ImportsExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<ExtractedRef> {
        entity
            .relationships
            .imports
            .iter()
            .map(|src_ref| ExtractedRef {
                target: src_ref.target().to_string(),
                simple_name: src_ref.simple_name().to_string(),
            })
            .collect()
    }
}

/// Extractor for REEXPORTS relationships
struct ReexportsExtractor;

impl ReferenceExtractor for ReexportsExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<ExtractedRef> {
        entity
            .relationships
            .reexports
            .iter()
            .map(|src_ref| ExtractedRef {
                target: src_ref.target().to_string(),
                simple_name: src_ref.simple_name().to_string(),
            })
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
    /// Create a new generic resolver with the given definition and extractor.
    ///
    /// This is private to ensure correct pairing of definitions and extractors.
    /// Use the factory functions (e.g., `calls_resolver()`) instead.
    fn new(def: &'static RelationshipDef, extractor: Box<dyn ReferenceExtractor>) -> Self {
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
    fn resolve_reference(&self, ext_ref: &ExtractedRef, maps: &TargetLookupMaps) -> Option<String> {
        // References are already normalized at extraction time:
        // - Turbofish generics stripped via tree-sitter query
        // - UFCS syntax preserved (starts with '<')
        // - Qualified names resolved via import maps
        // - simple_name pre-computed at extraction time
        let target = ext_ref.target.trim();

        for strategy in self.def.lookup_strategies {
            match strategy {
                LookupStrategy::QualifiedName => {
                    if let Some(id) = maps.qname.get(target) {
                        trace!("  [QualifiedName] resolved {} -> {}", target, id);
                        return Some(id.clone());
                    }
                }
                LookupStrategy::PathEntityIdentifier => {
                    if let Some(id) = maps.path_id.get(target) {
                        trace!("  [PathEntityIdentifier] resolved {} -> {}", target, id);
                        return Some(id.clone());
                    }
                }
                LookupStrategy::CallAliases => {
                    if let Some(id) = maps.call_alias.get(target) {
                        trace!("  [CallAliases] resolved {} -> {}", target, id);
                        return Some(id.clone());
                    }
                }
                LookupStrategy::UniqueSimpleName => {
                    // Use pre-computed simple_name from ExtractedRef
                    if let Some(id) = maps.unique_simple_name.get(&ext_ref.simple_name) {
                        trace!("  [UniqueSimpleName] resolved {} -> {}", target, id);
                        return Some(id.clone());
                    }
                }
                LookupStrategy::SimpleName => {
                    // Use pre-computed simple_name from ExtractedRef
                    if let Some(ids) = maps.all_simple_names.get(&ext_ref.simple_name) {
                        if !ids.is_empty() {
                            if ids.len() > 1 {
                                warn!(
                                    "  [SimpleName] ambiguous match for {}: {} candidates",
                                    target,
                                    ids.len()
                                );
                            }
                            trace!("  [SimpleName] resolved {} -> {}", target, &ids[0]);
                            return Some(ids[0].clone());
                        }
                    }
                }
            }
        }

        trace!("  [UNRESOLVED] {}", target);
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

            for ext_ref in refs {
                if let Some(target_id) = self.resolve_reference(&ext_ref, &maps) {
                    // Skip self-references (except for CALLS - recursive functions are valid)
                    if target_id == source.entity_id
                        && self.def.forward_rel != RelationshipType::Calls
                    {
                        continue;
                    }

                    // Forward edge
                    relationships.push((
                        source.entity_id.clone(),
                        target_id,
                        self.def.forward_rel.to_string(),
                    ));
                } else {
                    debug!(
                        "GenericResolver[{}]: unresolved reference {} -> {}",
                        self.def.name, source.qualified_name, ext_ref.target
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

/// Create a GenericResolver for REEXPORTS relationships
pub fn reexports_resolver() -> GenericResolver {
    GenericResolver::new(
        &codesearch_core::resolution::definitions::REEXPORTS,
        Box::new(ReexportsExtractor),
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
                    SourceReference::builder()
                        .target("crate::bar")
                        .simple_name("bar")
                        .is_external(false)
                        .location(SourceLocation {
                            start_line: 5,
                            end_line: 5,
                            start_column: 0,
                            end_column: 10,
                        })
                        .ref_type(codesearch_core::ReferenceType::Call)
                        .build()
                        .unwrap(),
                    SourceReference::builder()
                        .target("crate::baz")
                        .simple_name("baz")
                        .is_external(false)
                        .location(SourceLocation {
                            start_line: 6,
                            end_line: 6,
                            start_column: 0,
                            end_column: 10,
                        })
                        .ref_type(codesearch_core::ReferenceType::Call)
                        .build()
                        .unwrap(),
                ],
                ..Default::default()
            },
        );

        let refs = extractor.extract_refs(&entity);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].target, "crate::bar");
        assert_eq!(refs[0].simple_name, "bar");
        assert_eq!(refs[1].target, "crate::baz");
        assert_eq!(refs[1].simple_name, "baz");
    }

    #[test]
    fn test_uses_extractor() {
        let extractor = UsesExtractor;
        let entity = make_test_entity(
            EntityType::Function,
            "foo",
            "crate::foo",
            EntityRelationshipData {
                uses_types: vec![SourceReference::builder()
                    .target("crate::MyStruct")
                    .simple_name("MyStruct")
                    .is_external(false)
                    .location(SourceLocation {
                        start_line: 2,
                        end_line: 2,
                        start_column: 0,
                        end_column: 10,
                    })
                    .ref_type(codesearch_core::ReferenceType::TypeUsage)
                    .build()
                    .unwrap()],
                ..Default::default()
            },
        );

        let refs = extractor.extract_refs(&entity);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target, "crate::MyStruct");
        assert_eq!(refs[0].simple_name, "MyStruct");
    }

    #[test]
    fn test_implements_extractor() {
        let extractor = ImplementsExtractor;
        let entity = make_test_entity(
            EntityType::Impl,
            "impl",
            "crate::impl_MyTrait_for_MyStruct",
            EntityRelationshipData {
                implements_trait: Some(
                    SourceReference::builder()
                        .target("crate::MyTrait")
                        .simple_name("MyTrait")
                        .is_external(false)
                        .location(SourceLocation::default())
                        .ref_type(codesearch_core::ReferenceType::Extends)
                        .build()
                        .unwrap(),
                ),
                ..Default::default()
            },
        );

        let refs = extractor.extract_refs(&entity);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target, "crate::MyTrait");
        assert_eq!(refs[0].simple_name, "MyTrait");
    }

    #[test]
    fn test_supertraits_extractor() {
        // NOTE: Lifetimes are now excluded at extraction time (tree-sitter query
        // in type_handlers.rs), so they won't appear in supertraits at all.
        // This test verifies the extractor works correctly with clean data.
        let extractor = SupertraitsExtractor;
        let entity = make_test_entity(
            EntityType::Trait,
            "MyTrait",
            "crate::MyTrait",
            EntityRelationshipData {
                supertraits: vec![
                    SourceReference::builder()
                        .target("crate::BaseTrait")
                        .simple_name("BaseTrait")
                        .is_external(false)
                        .location(SourceLocation::default())
                        .ref_type(codesearch_core::ReferenceType::Extends)
                        .build()
                        .unwrap(),
                    SourceReference::builder()
                        .target("crate::OtherTrait")
                        .simple_name("OtherTrait")
                        .is_external(false)
                        .location(SourceLocation::default())
                        .ref_type(codesearch_core::ReferenceType::Extends)
                        .build()
                        .unwrap(),
                ],
                ..Default::default()
            },
        );

        let refs = extractor.extract_refs(&entity);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].target, "crate::BaseTrait");
        assert_eq!(refs[0].simple_name, "BaseTrait");
        assert_eq!(refs[1].target, "crate::OtherTrait");
        assert_eq!(refs[1].simple_name, "OtherTrait");
    }

    #[test]
    fn test_imports_extractor() {
        let extractor = ImportsExtractor;
        let entity = make_test_entity(
            EntityType::Module,
            "mod",
            "crate::mod",
            EntityRelationshipData {
                imports: vec![SourceReference::builder()
                    .target("crate::other::Thing")
                    .simple_name("Thing")
                    .is_external(false)
                    .location(SourceLocation::default())
                    .ref_type(codesearch_core::ReferenceType::Import)
                    .build()
                    .unwrap()],
                ..Default::default()
            },
        );

        let refs = extractor.extract_refs(&entity);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target, "crate::other::Thing");
        assert_eq!(refs[0].simple_name, "Thing");
    }

    #[test]
    fn test_calls_extractor_includes_recursive_self_calls() {
        let extractor = CallsExtractor;
        // A recursive function that calls itself
        let entity = make_test_entity(
            EntityType::Function,
            "factorial",
            "crate::factorial",
            EntityRelationshipData {
                calls: vec![SourceReference::builder()
                    .target("crate::factorial") // Self-call
                    .simple_name("factorial")
                    .is_external(false)
                    .location(SourceLocation {
                        start_line: 5,
                        end_line: 5,
                        start_column: 0,
                        end_column: 10,
                    })
                    .ref_type(codesearch_core::ReferenceType::Call)
                    .build()
                    .unwrap()],
                ..Default::default()
            },
        );

        let refs = extractor.extract_refs(&entity);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target, "crate::factorial");
        assert_eq!(refs[0].simple_name, "factorial");
        // The extractor should return the self-call; filtering happens in GenericResolver
        // which now allows self-references for CALLS relationships
    }
}
