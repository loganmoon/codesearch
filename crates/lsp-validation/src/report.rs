//! Validation report generation

use crate::metrics::RelationshipMetrics;
use codesearch_core::entities::{Language, ReferenceType, SourceLocation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// A discrepancy between our extraction and LSP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discrepancy {
    /// File containing the reference
    pub file: PathBuf,
    /// Location of the reference in source
    pub location: SourceLocation,
    /// Type of reference
    pub ref_type: ReferenceType,
    /// The reference text as it appears in source
    pub source_text: String,
    /// What we resolved it to (if any)
    pub our_target: Option<String>,
    /// What LSP resolved it to (if any)
    pub lsp_target: Option<String>,
    /// Explanation of the discrepancy
    pub reason: String,
}

/// Full validation report for a codebase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    /// Name of the codebase being validated
    pub codebase_name: String,
    /// Language of the codebase
    pub language: Language,
    /// Metrics per reference type
    pub metrics: HashMap<ReferenceType, RelationshipMetrics>,
    /// List of discrepancies found
    pub discrepancies: Vec<Discrepancy>,
    /// Time taken for validation in seconds
    pub duration_secs: f64,
}

impl ValidationReport {
    /// Create a new empty report
    pub fn new(codebase_name: impl Into<String>, language: Language) -> Self {
        Self {
            codebase_name: codebase_name.into(),
            language,
            metrics: HashMap::new(),
            discrepancies: Vec::new(),
            duration_secs: 0.0,
        }
    }

    /// Get overall metrics across all reference types
    pub fn overall_metrics(&self) -> RelationshipMetrics {
        let mut overall = RelationshipMetrics::default();
        for metrics in self.metrics.values() {
            overall.merge(metrics);
        }
        overall
    }

    /// Print a summary of the report to stdout
    pub fn print_summary(&self) {
        println!();
        println!("{}", "=".repeat(80));
        println!(
            "LSP VALIDATION REPORT: {} ({:?})",
            self.codebase_name, self.language
        );
        println!("{}", "=".repeat(80));

        println!(
            "\n{:<15} {:>8} {:>8} {:>8} {:>10} {:>10} {:>10}",
            "Ref Type", "TP", "FP", "FN", "Module", "Precision", "Recall"
        );
        println!("{:-<90}", "");

        let mut sorted_types: Vec<_> = self.metrics.keys().collect();
        sorted_types.sort_by_key(|t| format!("{t:?}"));

        for ref_type in sorted_types {
            let m = &self.metrics[ref_type];
            println!(
                "{:<15} {:>8} {:>8} {:>8} {:>10} {:>9.1}% {:>9.1}%",
                format!("{ref_type:?}"),
                m.true_positives,
                m.false_positives,
                m.false_negatives,
                m.module_refs,
                m.precision() * 100.0,
                m.recall() * 100.0
            );
        }

        let overall = self.overall_metrics();
        println!("{:-<90}", "");
        println!(
            "{:<15} {:>8} {:>8} {:>8} {:>10} {:>9.1}% {:>9.1}%",
            "OVERALL",
            overall.true_positives,
            overall.false_positives,
            overall.false_negatives,
            overall.module_refs,
            overall.precision() * 100.0,
            overall.recall() * 100.0
        );

        println!("\nF1 Score: {:.1}%", overall.f1() * 100.0);
        println!("External Refs: {}", overall.external_refs);
        println!("Module Refs (not validated): {}", overall.module_refs);
        println!("LSP Errors: {}", overall.lsp_errors);
        println!("Duration: {:.2}s", self.duration_secs);

        if !self.discrepancies.is_empty() {
            println!("\nTop Discrepancies (first 10):");
            for d in self.discrepancies.iter().take(10) {
                println!(
                    "  {}:{}:{} - {:?}: {}",
                    d.file.display(),
                    d.location.start_line,
                    d.location.start_column,
                    d.ref_type,
                    d.reason
                );
            }
        }

        println!("{}", "=".repeat(80));
    }

    /// Serialize the report to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_report() {
        let report = ValidationReport::new("test-codebase", Language::TypeScript);
        assert_eq!(report.codebase_name, "test-codebase");
        assert_eq!(report.language, Language::TypeScript);
        assert!(report.metrics.is_empty());
        assert!(report.discrepancies.is_empty());
    }

    #[test]
    fn test_overall_metrics() {
        let mut report = ValidationReport::new("test", Language::TypeScript);

        report.metrics.insert(
            ReferenceType::Call,
            RelationshipMetrics {
                true_positives: 10,
                false_positives: 2,
                ..Default::default()
            },
        );
        report.metrics.insert(
            ReferenceType::TypeUsage,
            RelationshipMetrics {
                true_positives: 5,
                false_positives: 1,
                ..Default::default()
            },
        );

        let overall = report.overall_metrics();
        assert_eq!(overall.true_positives, 15);
        assert_eq!(overall.false_positives, 3);
    }
}
