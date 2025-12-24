//! Graph comparison logic for computing precision/recall metrics

use std::collections::{HashMap, HashSet};

use super::models::{ComparisonResult, Metrics, Relationship, RelationshipKey, RelationshipType};

/// Compare extracted relationships against ground truth.
///
/// Uses strict matching on (source.qualified_name, target.qualified_name, relationship_type).
/// No fuzzy matching is performed.
pub fn compare(ground_truth: &[Relationship], extracted: &[Relationship]) -> ComparisonResult {
    // Build lookup sets
    let ground_truth_keys: HashSet<RelationshipKey> =
        ground_truth.iter().map(|r| r.to_key()).collect();
    let extracted_keys: HashSet<RelationshipKey> =
        extracted.iter().map(|r| r.to_key()).collect();

    // Build maps for retrieving original relationships
    let ground_truth_map: HashMap<RelationshipKey, &Relationship> =
        ground_truth.iter().map(|r| (r.to_key(), r)).collect();
    let extracted_map: HashMap<RelationshipKey, &Relationship> =
        extracted.iter().map(|r| (r.to_key(), r)).collect();

    // Calculate sets
    let true_positive_keys: HashSet<_> = ground_truth_keys
        .intersection(&extracted_keys)
        .cloned()
        .collect();
    let false_positive_keys: HashSet<_> = extracted_keys
        .difference(&ground_truth_keys)
        .cloned()
        .collect();
    let false_negative_keys: HashSet<_> = ground_truth_keys
        .difference(&extracted_keys)
        .cloned()
        .collect();

    // Convert back to relationships
    let true_positives: Vec<Relationship> = true_positive_keys
        .iter()
        .filter_map(|k| extracted_map.get(k).map(|r| (*r).clone()))
        .collect();
    let false_positives: Vec<Relationship> = false_positive_keys
        .iter()
        .filter_map(|k| extracted_map.get(k).map(|r| (*r).clone()))
        .collect();
    let false_negatives: Vec<Relationship> = false_negative_keys
        .iter()
        .filter_map(|k| ground_truth_map.get(k).map(|r| (*r).clone()))
        .collect();

    // Overall metrics
    let metrics = Metrics::calculate(
        true_positives.len(),
        false_positives.len(),
        false_negatives.len(),
    );

    // Per-type metrics
    let metrics_by_type = calculate_metrics_by_type(
        &true_positive_keys,
        &false_positive_keys,
        &false_negative_keys,
    );

    ComparisonResult {
        metrics,
        metrics_by_type,
        true_positives,
        false_positives,
        false_negatives,
    }
}

/// Calculate metrics broken down by relationship type
fn calculate_metrics_by_type(
    true_positives: &HashSet<RelationshipKey>,
    false_positives: &HashSet<RelationshipKey>,
    false_negatives: &HashSet<RelationshipKey>,
) -> HashMap<RelationshipType, Metrics> {
    let mut result = HashMap::new();

    for rel_type in RelationshipType::all() {
        let tp = true_positives
            .iter()
            .filter(|k| k.relationship_type == *rel_type)
            .count();
        let fp = false_positives
            .iter()
            .filter(|k| k.relationship_type == *rel_type)
            .count();
        let fn_count = false_negatives
            .iter()
            .filter(|k| k.relationship_type == *rel_type)
            .count();

        result.insert(*rel_type, Metrics::calculate(tp, fp, fn_count));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_validation::models::EntityRef;

    #[test]
    fn test_identical_graphs() {
        let rel = Relationship::new(
            EntityRef::new("mod::foo"),
            EntityRef::new("mod::bar"),
            RelationshipType::Calls,
        );

        let ground_truth = vec![rel.clone()];
        let extracted = vec![rel];

        let result = compare(&ground_truth, &extracted);

        assert_eq!(result.metrics.precision, 1.0);
        assert_eq!(result.metrics.recall, 1.0);
        assert_eq!(result.true_positives.len(), 1);
        assert!(result.false_positives.is_empty());
        assert!(result.false_negatives.is_empty());
    }

    #[test]
    fn test_partial_overlap() {
        let rel_a_b = Relationship::new(
            EntityRef::new("a"),
            EntityRef::new("b"),
            RelationshipType::Calls,
        );
        let rel_b_c = Relationship::new(
            EntityRef::new("b"),
            EntityRef::new("c"),
            RelationshipType::Calls,
        );
        let rel_c_d = Relationship::new(
            EntityRef::new("c"),
            EntityRef::new("d"),
            RelationshipType::Calls,
        );

        // Ground truth: A->B, B->C
        let ground_truth = vec![rel_a_b.clone(), rel_b_c];
        // Extracted: A->B, C->D
        let extracted = vec![rel_a_b, rel_c_d];

        let result = compare(&ground_truth, &extracted);

        // TP=1 (A->B), FP=1 (C->D), FN=1 (B->C)
        assert_eq!(result.true_positives.len(), 1);
        assert_eq!(result.false_positives.len(), 1);
        assert_eq!(result.false_negatives.len(), 1);
        assert!((result.metrics.precision - 0.5).abs() < 0.01);
        assert!((result.metrics.recall - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_empty_extracted() {
        let rel = Relationship::new(
            EntityRef::new("a"),
            EntityRef::new("b"),
            RelationshipType::Calls,
        );

        let ground_truth = vec![rel];
        let extracted = vec![];

        let result = compare(&ground_truth, &extracted);

        assert_eq!(result.metrics.precision, 0.0);
        assert_eq!(result.metrics.recall, 0.0);
        assert!(result.true_positives.is_empty());
        assert!(result.false_positives.is_empty());
        assert_eq!(result.false_negatives.len(), 1);
    }

    #[test]
    fn test_strict_matching_no_fuzzy() {
        // Different qualified names should NOT match
        let gt_rel = Relationship::new(
            EntityRef::new("my_mod::caller"),
            EntityRef::new("my_mod::callee"),
            RelationshipType::Calls,
        );
        let ext_rel = Relationship::new(
            EntityRef::new("caller"), // Different qualified name
            EntityRef::new("callee"),
            RelationshipType::Calls,
        );

        let ground_truth = vec![gt_rel];
        let extracted = vec![ext_rel];

        let result = compare(&ground_truth, &extracted);

        // Should NOT match due to strict matching
        assert!(result.true_positives.is_empty());
        assert_eq!(result.false_positives.len(), 1);
        assert_eq!(result.false_negatives.len(), 1);
    }
}
