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
    /// Fully qualified name of the entity
    pub qualified_name: String,
    /// Type of the entity (function, struct, etc.)
    pub entity_type: Option<EntityType>,
    /// File path (optional, for additional context)
    pub file_path: Option<String>,
}

impl EntityRef {
    pub fn new(qualified_name: impl Into<String>) -> Self {
        Self {
            qualified_name: qualified_name.into(),
            entity_type: None,
            file_path: None,
        }
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
            source_qname: self.source.qualified_name.clone(),
            target_qname: self.target.qualified_name.clone(),
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
#[derive(Debug, Clone, Default)]
pub struct Metrics {
    pub precision: f64,
    pub recall: f64,
    pub f1_score: f64,
    pub true_positive_count: usize,
    pub false_positive_count: usize,
    pub false_negative_count: usize,
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
        println!("  Precision: {:.2}%", self.metrics.precision * 100.0);
        println!("  Recall:    {:.2}%", self.metrics.recall * 100.0);
        println!("  F1 Score:  {:.2}%", self.metrics.f1_score * 100.0);
        println!();
        println!("Counts:");
        println!("  True Positives:  {}", self.true_positives.len());
        println!("  False Positives: {}", self.false_positives.len());
        println!("  False Negatives: {}", self.false_negatives.len());
        println!();
        println!("By Relationship Type:");

        for rel_type in RelationshipType::all() {
            if let Some(metrics) = self.metrics_by_type.get(rel_type) {
                if metrics.true_positive_count + metrics.false_positive_count + metrics.false_negative_count > 0 {
                    println!(
                        "  {}: P={:.1}%, R={:.1}%, F1={:.1}% (TP={}, FP={}, FN={})",
                        rel_type,
                        metrics.precision * 100.0,
                        metrics.recall * 100.0,
                        metrics.f1_score * 100.0,
                        metrics.true_positive_count,
                        metrics.false_positive_count,
                        metrics.false_negative_count,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_calculate_all_zeros() {
        let m = Metrics::calculate(0, 0, 0);
        assert_eq!(m.precision, 0.0);
        assert_eq!(m.recall, 0.0);
        assert_eq!(m.f1_score, 0.0);
        assert_eq!(m.true_positive_count, 0);
        assert_eq!(m.false_positive_count, 0);
        assert_eq!(m.false_negative_count, 0);
    }

    #[test]
    fn test_metrics_calculate_perfect_precision() {
        // All extracted are correct, but we missed some
        let m = Metrics::calculate(10, 0, 5);
        assert_eq!(m.precision, 1.0);
        assert!((m.recall - 0.666).abs() < 0.01);
        assert_eq!(m.true_positive_count, 10);
        assert_eq!(m.false_positive_count, 0);
        assert_eq!(m.false_negative_count, 5);
    }

    #[test]
    fn test_metrics_calculate_perfect_recall() {
        // We found everything, but also some extra
        let m = Metrics::calculate(10, 5, 0);
        assert!((m.precision - 0.666).abs() < 0.01);
        assert_eq!(m.recall, 1.0);
    }

    #[test]
    fn test_metrics_calculate_perfect_f1() {
        // Perfect precision and recall
        let m = Metrics::calculate(10, 0, 0);
        assert_eq!(m.precision, 1.0);
        assert_eq!(m.recall, 1.0);
        assert_eq!(m.f1_score, 1.0);
    }
}
