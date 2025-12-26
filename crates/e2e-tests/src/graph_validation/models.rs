//! Data models for graph validation

use codesearch_core::entities::EntityType;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};

/// Relationship types that can be validated.
/// Maps to ALLOWED_RELATIONSHIP_TYPES from storage crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationshipType {
    Calls,
    Uses,
    Contains,
    Implements,
    Imports,
    InheritsFrom,
    Associates,
    ExtendsInterface,
}

impl fmt::Display for RelationshipType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelationshipType::Calls => write!(f, "CALLS"),
            RelationshipType::Uses => write!(f, "USES"),
            RelationshipType::Contains => write!(f, "CONTAINS"),
            RelationshipType::Implements => write!(f, "IMPLEMENTS"),
            RelationshipType::Imports => write!(f, "IMPORTS"),
            RelationshipType::InheritsFrom => write!(f, "INHERITS_FROM"),
            RelationshipType::Associates => write!(f, "ASSOCIATES"),
            RelationshipType::ExtendsInterface => write!(f, "EXTENDS_INTERFACE"),
        }
    }
}

impl RelationshipType {
    /// Parse from Neo4j relationship type string
    pub fn from_neo4j_type(s: &str) -> Option<Self> {
        match s {
            "CALLS" => Some(RelationshipType::Calls),
            "USES" => Some(RelationshipType::Uses),
            "CONTAINS" => Some(RelationshipType::Contains),
            "IMPLEMENTS" => Some(RelationshipType::Implements),
            "IMPORTS" => Some(RelationshipType::Imports),
            "INHERITS_FROM" => Some(RelationshipType::InheritsFrom),
            "ASSOCIATES" => Some(RelationshipType::Associates),
            "EXTENDS_INTERFACE" => Some(RelationshipType::ExtendsInterface),
            _ => None,
        }
    }

    /// All relationship types for iteration
    pub fn all() -> &'static [RelationshipType] {
        &[
            RelationshipType::Calls,
            RelationshipType::Uses,
            RelationshipType::Contains,
            RelationshipType::Implements,
            RelationshipType::Imports,
            RelationshipType::InheritsFrom,
            RelationshipType::Associates,
            RelationshipType::ExtendsInterface,
        ]
    }
}

/// Reference to an entity (source or target of a relationship)
#[derive(Debug, Clone)]
pub struct EntityRef {
    qualified_name: String,
    entity_type: Option<EntityType>,
    file_path: Option<String>,
}

impl EntityRef {
    /// Create a new EntityRef with the given qualified name.
    ///
    /// # Panics
    /// Panics if `qualified_name` is empty after trimming whitespace.
    pub fn new(qualified_name: impl Into<String>) -> Self {
        let qualified_name = qualified_name.into();
        assert!(
            !qualified_name.trim().is_empty(),
            "EntityRef qualified_name must be non-empty"
        );
        Self {
            qualified_name,
            entity_type: None,
            file_path: None,
        }
    }

    /// Get the qualified name of this entity.
    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }

    /// Get the entity type, if known.
    pub fn entity_type(&self) -> Option<EntityType> {
        self.entity_type
    }

    /// Get the file path, if known.
    pub fn file_path(&self) -> Option<&str> {
        self.file_path.as_deref()
    }

    pub fn with_entity_type(mut self, entity_type: EntityType) -> Self {
        self.entity_type = Some(entity_type);
        self
    }

    pub fn with_file_path(mut self, file_path: impl Into<String>) -> Self {
        self.file_path = Some(file_path.into());
        self
    }
}

impl PartialEq for EntityRef {
    fn eq(&self, other: &Self) -> bool {
        // Strict matching on qualified_name only
        self.qualified_name == other.qualified_name
    }
}

impl Eq for EntityRef {}

impl Hash for EntityRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.qualified_name.hash(state);
    }
}

/// A relationship between two entities
#[derive(Debug, Clone)]
pub struct Relationship {
    pub source: EntityRef,
    pub target: EntityRef,
    pub relationship_type: RelationshipType,
}

impl Relationship {
    pub fn new(
        source: EntityRef,
        target: EntityRef,
        relationship_type: RelationshipType,
    ) -> Self {
        Self {
            source,
            target,
            relationship_type,
        }
    }

    /// Create a canonical key for hashing/comparison
    pub fn to_key(&self) -> RelationshipKey {
        RelationshipKey {
            source_qname: self.source.qualified_name().to_owned(),
            target_qname: self.target.qualified_name().to_owned(),
            relationship_type: self.relationship_type,
        }
    }
}

/// Hashable key for relationship comparison
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RelationshipKey {
    pub source_qname: String,
    pub target_qname: String,
    pub relationship_type: RelationshipType,
}

impl fmt::Display for RelationshipKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} --[{}]--> {}",
            self.source_qname, self.relationship_type, self.target_qname
        )
    }
}

/// Precision/recall metrics
///
/// Metrics can only be constructed via `Metrics::calculate()` to ensure
/// internal consistency (e.g., f1_score matches precision/recall).
#[derive(Debug, Clone)]
pub struct Metrics {
    precision: f64,
    recall: f64,
    f1_score: f64,
    true_positive_count: usize,
    false_positive_count: usize,
    false_negative_count: usize,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::calculate(0, 0, 0)
    }
}

impl Metrics {
    /// Calculate metrics from counts
    pub fn calculate(tp: usize, fp: usize, fn_count: usize) -> Self {
        let precision = if tp + fp > 0 {
            tp as f64 / (tp + fp) as f64
        } else {
            0.0
        };

        let recall = if tp + fn_count > 0 {
            tp as f64 / (tp + fn_count) as f64
        } else {
            0.0
        };

        let f1_score = if precision + recall > 0.0 {
            2.0 * (precision * recall) / (precision + recall)
        } else {
            0.0
        };

        Self {
            precision,
            recall,
            f1_score,
            true_positive_count: tp,
            false_positive_count: fp,
            false_negative_count: fn_count,
        }
    }

    /// Precision: proportion of extracted relationships that are correct
    pub fn precision(&self) -> f64 {
        self.precision
    }

    /// Recall: proportion of ground truth relationships that were found
    pub fn recall(&self) -> f64 {
        self.recall
    }

    /// F1 score: harmonic mean of precision and recall
    pub fn f1_score(&self) -> f64 {
        self.f1_score
    }

    /// Number of correctly extracted relationships
    pub fn true_positive_count(&self) -> usize {
        self.true_positive_count
    }

    /// Number of incorrectly extracted relationships (not in ground truth)
    pub fn false_positive_count(&self) -> usize {
        self.false_positive_count
    }

    /// Number of missed relationships (in ground truth but not extracted)
    pub fn false_negative_count(&self) -> usize {
        self.false_negative_count
    }
}

/// Result of comparing two graphs
#[derive(Debug)]
pub struct ComparisonResult {
    /// Overall metrics
    pub metrics: Metrics,
    /// Metrics broken down by relationship type
    pub metrics_by_type: HashMap<RelationshipType, Metrics>,
    /// Relationships found in both graphs (true positives)
    pub true_positives: Vec<Relationship>,
    /// Relationships in extracted but not in ground truth (false positives)
    pub false_positives: Vec<Relationship>,
    /// Relationships in ground truth but not in extracted (false negatives)
    pub false_negatives: Vec<Relationship>,
}

impl ComparisonResult {
    /// Print a summary of the validation results to stdout
    pub fn print_summary(&self) {
        println!("=== Graph Validation Results ===");
        println!();
        println!("Overall Metrics:");
        println!("  Precision: {:.2}%", self.metrics.precision() * 100.0);
        println!("  Recall:    {:.2}%", self.metrics.recall() * 100.0);
        println!("  F1 Score:  {:.2}%", self.metrics.f1_score() * 100.0);
        println!();
        println!("Counts:");
        println!("  True Positives:  {}", self.true_positives.len());
        println!("  False Positives: {}", self.false_positives.len());
        println!("  False Negatives: {}", self.false_negatives.len());
        println!();
        println!("By Relationship Type:");

        for rel_type in RelationshipType::all() {
            if let Some(metrics) = self.metrics_by_type.get(rel_type) {
                let total = metrics.true_positive_count()
                    + metrics.false_positive_count()
                    + metrics.false_negative_count();
                if total > 0 {
                    println!(
                        "  {}: P={:.1}%, R={:.1}%, F1={:.1}% (TP={}, FP={}, FN={})",
                        rel_type,
                        metrics.precision() * 100.0,
                        metrics.recall() * 100.0,
                        metrics.f1_score() * 100.0,
                        metrics.true_positive_count(),
                        metrics.false_positive_count(),
                        metrics.false_negative_count(),
                    );
                }
            }
        }
    }
}

/// Check if a qualified name contains external references or test types.
///
/// Returns true if the name should be filtered out from IMPORTS comparison.
fn should_filter_import_name(name: &str) -> bool {
    // Filter out external references
    if name.contains("external::") {
        return true;
    }

    // Filter out test types (TestError, test fixtures, etc.)
    let test_patterns = ["TestError", "test_", "Test::", "tests::"];
    for pattern in test_patterns {
        if name.contains(pattern) {
            return true;
        }
    }

    false
}

/// Clean up a qualified name for module extraction.
///
/// Handles impl blocks like "anyhow::error::impl external" or
/// "anyhow::backtrace::capture::impl anyhow::backtrace::capture"
fn clean_qualified_name(qualified_name: &str) -> &str {
    // Handle "impl X" patterns - find "impl " and take everything before it
    if let Some(impl_pos) = qualified_name.find("::impl ") {
        return &qualified_name[..impl_pos];
    }

    // Handle trait impl blocks like "<Type as Trait>::method"
    if qualified_name.starts_with('<') {
        // Try to extract the first type before " as " or ">"
        let inner = qualified_name.trim_start_matches('<');
        let type_part = inner
            .split(" as ")
            .next()
            .unwrap_or(inner)
            .split('>')
            .next()
            .unwrap_or(inner);

        // If the type itself contains external::, return as-is for filtering
        if type_part.contains("external::") {
            return qualified_name;
        }

        return type_part;
    }

    qualified_name
}

/// Extract the module path from a qualified name using entity type.
///
/// For SCIP comparison, we need to map entities to their containing module:
/// - Module/Package entities: use qualified_name directly (it IS a module)
/// - Other entities: extract parent module (drop the last segment)
///
/// Then limit to max 2 segments for SCIP's module-level granularity:
/// - `anyhow::error` -> `anyhow::error`
/// - `anyhow::backtrace::capture` -> `anyhow::backtrace`
pub fn extract_module_from_entity(entity: &EntityRef) -> String {
    let qualified_name = entity.qualified_name();

    // Handle special cases
    if qualified_name.is_empty() {
        return qualified_name.to_string();
    }

    // Clean up impl blocks first
    let cleaned = clean_qualified_name(qualified_name);

    // Skip entities with "external::" - these are unresolved references
    if cleaned.contains("external::") {
        return qualified_name.to_string();
    }

    let segments: Vec<&str> = cleaned.split("::").collect();

    // Determine how many segments to keep based on entity type
    let module_segments: Vec<&str> = match entity.entity_type() {
        // Module and Package ARE modules - use all segments (up to limit)
        Some(EntityType::Module) | Some(EntityType::Package) => {
            segments.iter().take(2).copied().collect()
        }
        // For all other types, drop the last segment (the entity name itself)
        // to get the parent module
        _ => {
            if segments.len() <= 1 {
                // Top-level entity, return package name
                segments.clone()
            } else {
                // Drop the last segment (entity name), keep up to 2 module segments
                segments[..segments.len() - 1].iter().take(2).copied().collect()
            }
        }
    };

    if module_segments.is_empty() {
        return segments.first().map(|s| s.to_string()).unwrap_or_default();
    }

    module_segments.join("::")
}

/// Legacy function for extracting module when entity type is unknown.
/// Uses CamelCase heuristic as fallback.
fn is_type_segment(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }
    segment.chars().next().unwrap_or('a').is_ascii_uppercase()
}

/// Extract module from qualified name without entity type info.
/// Used as fallback when entity type is not available.
pub fn extract_module_from_qualified_name(qualified_name: &str) -> String {
    if qualified_name.is_empty() {
        return qualified_name.to_string();
    }

    let cleaned = clean_qualified_name(qualified_name);
    if cleaned.contains("external::") {
        return qualified_name.to_string();
    }

    let segments: Vec<&str> = cleaned.split("::").collect();

    // Use CamelCase heuristic: stop at first uppercase segment
    let mut module_segments = Vec::new();
    for segment in &segments {
        if is_type_segment(segment) {
            break;
        }
        module_segments.push(*segment);
        if module_segments.len() >= 2 {
            break;
        }
    }

    if module_segments.is_empty() {
        return segments.first().map(|s| s.to_string()).unwrap_or_default();
    }

    module_segments.join("::")
}

/// Aggregate IMPORTS relationships to module level.
///
/// SCIP tracks module→module imports, while codesearch tracks entity→entity.
/// This function converts entity-level IMPORTS to module-level for comparison.
///
/// Uses entity type information when available:
/// - Module/Package: use qualified_name as the module
/// - Other types: extract parent module by dropping the last segment
///
/// Filtering applied:
/// 1. Filter out sources/targets containing `external::`
/// 2. Filter out imports targeting test types (TestError, etc.)
/// 3. Handle impl qualified names - strip the impl portion
/// 4. Aggregate to top-level modules (max 2 segments)
/// 5. Deduplicate module→module pairs
pub fn aggregate_imports_to_module_level(relationships: &[Relationship]) -> Vec<Relationship> {
    use std::collections::HashSet;

    let mut module_imports: HashSet<(String, String)> = HashSet::new();
    let mut result = Vec::new();

    for rel in relationships {
        if rel.relationship_type == RelationshipType::Imports {
            let source_qname = rel.source.qualified_name();
            let target_qname = rel.target.qualified_name();

            // Filter out external references and test types
            if should_filter_import_name(source_qname) || should_filter_import_name(target_qname) {
                continue;
            }

            // Use entity type-aware extraction when available, fall back to heuristic
            let source_module = if rel.source.entity_type().is_some() {
                extract_module_from_entity(&rel.source)
            } else {
                extract_module_from_qualified_name(source_qname)
            };

            let target_module = if rel.target.entity_type().is_some() {
                extract_module_from_entity(&rel.target)
            } else {
                extract_module_from_qualified_name(target_qname)
            };

            // Skip if extraction resulted in external:: (from impl blocks for external types)
            if source_module.contains("external::") || target_module.contains("external::") {
                continue;
            }

            // Skip self-imports
            if source_module == target_module {
                continue;
            }

            // Deduplicate
            if module_imports.insert((source_module.clone(), target_module.clone())) {
                result.push(Relationship::new(
                    EntityRef::new(source_module),
                    EntityRef::new(target_module),
                    RelationshipType::Imports,
                ));
            }
        } else {
            // Keep other relationship types as-is
            result.push(rel.clone());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_module_from_entity_with_types() {
        // Function entity - drops the function name to get parent module
        let func = EntityRef::new("anyhow::error::object_boxed").with_entity_type(EntityType::Function);
        assert_eq!(extract_module_from_entity(&func), "anyhow::error");

        // Struct entity - drops the struct name to get parent module
        let strct = EntityRef::new("anyhow::error::ErrorImpl").with_entity_type(EntityType::Struct);
        assert_eq!(extract_module_from_entity(&strct), "anyhow::error");

        // Method entity - drops the method name
        let method = EntityRef::new("anyhow::chain::Chain::new").with_entity_type(EntityType::Method);
        assert_eq!(extract_module_from_entity(&method), "anyhow::chain");

        // Module entity - keeps module path (up to 2 segments)
        let module = EntityRef::new("anyhow::error").with_entity_type(EntityType::Module);
        assert_eq!(extract_module_from_entity(&module), "anyhow::error");

        // Deeply nested module - limited to 2 segments
        let deep_mod = EntityRef::new("anyhow::backtrace::capture").with_entity_type(EntityType::Module);
        assert_eq!(extract_module_from_entity(&deep_mod), "anyhow::backtrace");

        // Top-level type - returns package only
        let top_type = EntityRef::new("anyhow::Error").with_entity_type(EntityType::Struct);
        assert_eq!(extract_module_from_entity(&top_type), "anyhow");
    }

    #[test]
    fn test_extract_module_from_qualified_name_heuristic() {
        // Simple module/package
        assert_eq!(extract_module_from_qualified_name("anyhow"), "anyhow");

        // Type at top level - stop at type (CamelCase), return package only
        assert_eq!(extract_module_from_qualified_name("anyhow::Error"), "anyhow");

        // Type at top level with method
        assert_eq!(extract_module_from_qualified_name("anyhow::Error::new"), "anyhow");

        // Nested type - stop at type
        assert_eq!(
            extract_module_from_qualified_name("anyhow::error::ErrorImpl"),
            "anyhow::error"
        );

        // Type with nested method
        assert_eq!(
            extract_module_from_qualified_name("anyhow::chain::Chain::new"),
            "anyhow::chain"
        );

        // All lowercase - limited to 2 segments
        assert_eq!(
            extract_module_from_qualified_name("anyhow::error::object_boxed"),
            "anyhow::error"
        );

        // Deeply nested modules (all lowercase) - limited to 2 segments
        assert_eq!(
            extract_module_from_qualified_name("anyhow::backtrace::capture::output_filename"),
            "anyhow::backtrace"
        );
    }

    #[test]
    fn test_extract_module_handles_impl_blocks() {
        // Impl block patterns should strip the impl portion
        assert_eq!(
            extract_module_from_qualified_name("anyhow::error::impl external"),
            "anyhow::error"
        );

        assert_eq!(
            extract_module_from_qualified_name("anyhow::backtrace::capture::impl anyhow"),
            "anyhow::backtrace"
        );
    }

    #[test]
    fn test_is_type_segment() {
        // Types are CamelCase
        assert!(is_type_segment("Error"));
        assert!(is_type_segment("ErrorImpl"));
        assert!(is_type_segment("Chain"));
        assert!(is_type_segment("Result"));

        // Modules are snake_case
        assert!(!is_type_segment("error"));
        assert!(!is_type_segment("chain"));
        assert!(!is_type_segment("backtrace"));
        assert!(!is_type_segment("object_boxed"));

        // Edge cases
        assert!(!is_type_segment(""));
        assert!(!is_type_segment("anyhow"));
    }

    #[test]
    fn test_should_filter_import_name() {
        // External references should be filtered
        assert!(should_filter_import_name("external::ErrorImpl"));
        assert!(should_filter_import_name("anyhow::<external::T as Foo>"));

        // Test types should be filtered
        assert!(should_filter_import_name("TestError"));
        assert!(should_filter_import_name("anyhow::test_foo"));
        assert!(should_filter_import_name("anyhow::tests::helper"));

        // Normal names should not be filtered
        assert!(!should_filter_import_name("anyhow::error"));
        assert!(!should_filter_import_name("anyhow::Error"));
    }

    #[test]
    fn test_aggregate_imports_to_module_level() {
        let imports = vec![
            Relationship::new(
                EntityRef::new("anyhow::fmt::Indented"),
                EntityRef::new("anyhow::chain::Chain"),
                RelationshipType::Imports,
            ),
            Relationship::new(
                EntityRef::new("anyhow::fmt::format_err"),
                EntityRef::new("anyhow::chain::ChainState"),
                RelationshipType::Imports,
            ),
            // This should be deduplicated (same module pair)
            Relationship::new(
                EntityRef::new("anyhow::fmt::another"),
                EntityRef::new("anyhow::chain::Other"),
                RelationshipType::Imports,
            ),
        ];

        let aggregated = aggregate_imports_to_module_level(&imports);

        // Should have 1 unique module->module import (anyhow::fmt -> anyhow::chain)
        let import_count = aggregated
            .iter()
            .filter(|r| r.relationship_type == RelationshipType::Imports)
            .count();
        assert_eq!(import_count, 1);

        let import = aggregated
            .iter()
            .find(|r| r.relationship_type == RelationshipType::Imports)
            .unwrap();
        assert_eq!(import.source.qualified_name(), "anyhow::fmt");
        assert_eq!(import.target.qualified_name(), "anyhow::chain");
    }

    #[test]
    fn test_aggregate_filters_external_and_test() {
        let imports = vec![
            // Should be filtered - external in source
            Relationship::new(
                EntityRef::new("anyhow::<external::T as Foo>::method"),
                EntityRef::new("anyhow::error::Error"),
                RelationshipType::Imports,
            ),
            // Should be filtered - external in target
            Relationship::new(
                EntityRef::new("anyhow::error::Error"),
                EntityRef::new("external::TestError"),
                RelationshipType::Imports,
            ),
            // Should be filtered - test type
            Relationship::new(
                EntityRef::new("anyhow::fmt::tests::helper"),
                EntityRef::new("anyhow::chain::Chain"),
                RelationshipType::Imports,
            ),
            // Should be kept
            Relationship::new(
                EntityRef::new("anyhow::context::private::Sealed"),
                EntityRef::new("anyhow::error::Error"),
                RelationshipType::Imports,
            ),
        ];

        let aggregated = aggregate_imports_to_module_level(&imports);

        let import_count = aggregated
            .iter()
            .filter(|r| r.relationship_type == RelationshipType::Imports)
            .count();
        assert_eq!(import_count, 1);

        let import = aggregated
            .iter()
            .find(|r| r.relationship_type == RelationshipType::Imports)
            .unwrap();
        assert_eq!(import.source.qualified_name(), "anyhow::context");
        assert_eq!(import.target.qualified_name(), "anyhow::error");
    }

    #[test]
    fn test_metrics_calculate_all_zeros() {
        let m = Metrics::calculate(0, 0, 0);
        assert_eq!(m.precision(), 0.0);
        assert_eq!(m.recall(), 0.0);
        assert_eq!(m.f1_score(), 0.0);
        assert_eq!(m.true_positive_count(), 0);
        assert_eq!(m.false_positive_count(), 0);
        assert_eq!(m.false_negative_count(), 0);
    }

    #[test]
    fn test_metrics_calculate_perfect_precision() {
        // All extracted are correct, but we missed some
        let m = Metrics::calculate(10, 0, 5);
        assert_eq!(m.precision(), 1.0);
        assert!((m.recall() - 0.666).abs() < 0.01);
        assert_eq!(m.true_positive_count(), 10);
        assert_eq!(m.false_positive_count(), 0);
        assert_eq!(m.false_negative_count(), 5);
    }

    #[test]
    fn test_metrics_calculate_perfect_recall() {
        // We found everything, but also some extra
        let m = Metrics::calculate(10, 5, 0);
        assert!((m.precision() - 0.666).abs() < 0.01);
        assert_eq!(m.recall(), 1.0);
    }

    #[test]
    fn test_metrics_calculate_perfect_f1() {
        // Perfect precision and recall
        let m = Metrics::calculate(10, 0, 0);
        assert_eq!(m.precision(), 1.0);
        assert_eq!(m.recall(), 1.0);
        assert_eq!(m.f1_score(), 1.0);
    }
}
