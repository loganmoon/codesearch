//! Validation metrics for precision, recall, and F1 score

use serde::{Deserialize, Serialize};

/// Metrics for a relationship type
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationshipMetrics {
    /// References we extracted that LSP confirms (correct extractions)
    pub true_positives: usize,
    /// References we extracted but LSP disagrees (incorrect extractions)
    pub false_positives: usize,
    /// References LSP found but we missed (from findReferences, if used)
    pub false_negatives: usize,
    /// External references (stdlib, deps) - not penalized
    pub external_refs: usize,
    /// Module-level targets that can't be validated via LSP find_references
    pub module_refs: usize,
    /// LSP errors that prevented validation
    pub lsp_errors: usize,
}

impl RelationshipMetrics {
    /// Calculate precision: TP / (TP + FP)
    ///
    /// Precision measures: of the references we extracted, how many are correct?
    pub fn precision(&self) -> f64 {
        let total_positive = self.true_positives + self.false_positives;
        if total_positive == 0 {
            1.0 // No predictions = vacuously correct
        } else {
            self.true_positives as f64 / total_positive as f64
        }
    }

    /// Calculate recall: TP / (TP + FN)
    ///
    /// Recall measures: of the references that exist, how many did we find?
    pub fn recall(&self) -> f64 {
        let total_actual = self.true_positives + self.false_negatives;
        if total_actual == 0 {
            1.0 // No actual positives = vacuously complete
        } else {
            self.true_positives as f64 / total_actual as f64
        }
    }

    /// Calculate F1 score: harmonic mean of precision and recall
    pub fn f1(&self) -> f64 {
        let p = self.precision();
        let r = self.recall();
        if p + r == 0.0 {
            0.0
        } else {
            2.0 * p * r / (p + r)
        }
    }

    /// Total validated references (not counting external or errors)
    pub fn total_validated(&self) -> usize {
        self.true_positives + self.false_positives + self.false_negatives
    }

    /// Merge another metrics instance into this one
    pub fn merge(&mut self, other: &RelationshipMetrics) {
        self.true_positives += other.true_positives;
        self.false_positives += other.false_positives;
        self.false_negatives += other.false_negatives;
        self.external_refs += other.external_refs;
        self.module_refs += other.module_refs;
        self.lsp_errors += other.lsp_errors;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_precision_all_correct() {
        let m = RelationshipMetrics {
            true_positives: 10,
            false_positives: 0,
            ..Default::default()
        };
        assert!((m.precision() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_precision_half_correct() {
        let m = RelationshipMetrics {
            true_positives: 5,
            false_positives: 5,
            ..Default::default()
        };
        assert!((m.precision() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_recall_all_found() {
        let m = RelationshipMetrics {
            true_positives: 10,
            false_negatives: 0,
            ..Default::default()
        };
        assert!((m.recall() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_recall_half_found() {
        let m = RelationshipMetrics {
            true_positives: 5,
            false_negatives: 5,
            ..Default::default()
        };
        assert!((m.recall() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_f1_perfect() {
        let m = RelationshipMetrics {
            true_positives: 10,
            false_positives: 0,
            false_negatives: 0,
            ..Default::default()
        };
        assert!((m.f1() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_f1_balanced() {
        let m = RelationshipMetrics {
            true_positives: 5,
            false_positives: 5,
            false_negatives: 5,
            ..Default::default()
        };
        // precision = 5/10 = 0.5, recall = 5/10 = 0.5
        // F1 = 2 * 0.5 * 0.5 / (0.5 + 0.5) = 0.5
        assert!((m.f1() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_merge() {
        let mut m1 = RelationshipMetrics {
            true_positives: 5,
            false_positives: 2,
            false_negatives: 1,
            external_refs: 3,
            module_refs: 0,
            lsp_errors: 0,
        };
        let m2 = RelationshipMetrics {
            true_positives: 3,
            false_positives: 1,
            false_negatives: 2,
            external_refs: 2,
            module_refs: 1,
            lsp_errors: 1,
        };
        m1.merge(&m2);

        assert_eq!(m1.true_positives, 8);
        assert_eq!(m1.false_positives, 3);
        assert_eq!(m1.false_negatives, 3);
        assert_eq!(m1.external_refs, 5);
        assert_eq!(m1.lsp_errors, 1);
    }
}
