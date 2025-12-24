//! Report generation for graph validation results

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;

use super::models::{ComparisonResult, RelationshipType};

/// Write a detailed validation report to the logs directory.
///
/// Creates a timestamped log file at `{workspace_root}/logs/graph_validation_{repo_name}_{timestamp}.log`.
/// Returns the path to the created log file.
pub fn write_report(result: &ComparisonResult, repo_name: &str) -> Result<PathBuf> {
    let log_dir = get_workspace_root()?.join("logs");
    fs::create_dir_all(&log_dir).context("Failed to create logs directory")?;

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let log_path = log_dir.join(format!("graph_validation_{repo_name}_{timestamp}.log"));

    let mut file = File::create(&log_path)
        .with_context(|| format!("Failed to create log file: {}", log_path.display()))?;

    write_report_content(&mut file, result, repo_name)?;

    Ok(log_path)
}

/// Write the report content to a writer
fn write_report_content<W: Write>(
    writer: &mut W,
    result: &ComparisonResult,
    repo_name: &str,
) -> Result<()> {
    writeln!(writer, "=== Graph Validation Report ===")?;
    writeln!(writer, "Repository: {repo_name}")?;
    writeln!(writer, "Timestamp: {}", Utc::now().format("%Y-%m-%d %H:%M:%S UTC"))?;
    writeln!(writer)?;

    // Overall metrics
    writeln!(writer, "=== Overall Metrics ===")?;
    writeln!(writer, "Precision: {:.2}%", result.metrics.precision * 100.0)?;
    writeln!(writer, "Recall:    {:.2}%", result.metrics.recall * 100.0)?;
    writeln!(writer, "F1 Score:  {:.2}%", result.metrics.f1_score * 100.0)?;
    writeln!(writer)?;

    writeln!(writer, "Counts:")?;
    writeln!(writer, "  True Positives:  {}", result.true_positives.len())?;
    writeln!(writer, "  False Positives: {}", result.false_positives.len())?;
    writeln!(writer, "  False Negatives: {}", result.false_negatives.len())?;
    writeln!(writer)?;

    // Per-type metrics
    writeln!(writer, "=== Metrics by Relationship Type ===")?;
    for rel_type in RelationshipType::all() {
        if let Some(metrics) = result.metrics_by_type.get(rel_type) {
            let total = metrics.true_positive_count
                + metrics.false_positive_count
                + metrics.false_negative_count;
            if total > 0 {
                writeln!(
                    writer,
                    "{}: P={:.1}%, R={:.1}%, F1={:.1}% (TP={}, FP={}, FN={})",
                    rel_type,
                    metrics.precision * 100.0,
                    metrics.recall * 100.0,
                    metrics.f1_score * 100.0,
                    metrics.true_positive_count,
                    metrics.false_positive_count,
                    metrics.false_negative_count,
                )?;
            }
        }
    }
    writeln!(writer)?;

    // False positives
    writeln!(
        writer,
        "=== False Positives ({}) - In extracted but not in ground truth ===",
        result.false_positives.len()
    )?;
    for rel in &result.false_positives {
        writeln!(
            writer,
            "  {} --[{}]--> {}",
            rel.source.qualified_name, rel.relationship_type, rel.target.qualified_name
        )?;
    }
    writeln!(writer)?;

    // False negatives
    writeln!(
        writer,
        "=== False Negatives ({}) - In ground truth but not extracted ===",
        result.false_negatives.len()
    )?;
    for rel in &result.false_negatives {
        writeln!(
            writer,
            "  {} --[{}]--> {}",
            rel.source.qualified_name, rel.relationship_type, rel.target.qualified_name
        )?;
    }
    writeln!(writer)?;

    // True positives (summary)
    writeln!(
        writer,
        "=== True Positives ({}) - Correctly extracted ===",
        result.true_positives.len()
    )?;
    // Only show first 50 to keep log manageable
    for rel in result.true_positives.iter().take(50) {
        writeln!(
            writer,
            "  {} --[{}]--> {}",
            rel.source.qualified_name, rel.relationship_type, rel.target.qualified_name
        )?;
    }
    if result.true_positives.len() > 50 {
        writeln!(
            writer,
            "  ... and {} more",
            result.true_positives.len() - 50
        )?;
    }

    Ok(())
}

/// Get the workspace root directory
fn get_workspace_root() -> Result<PathBuf> {
    // CARGO_MANIFEST_DIR points to crates/e2e-tests
    // Go up two levels to get workspace root
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Fallback: use current file's location
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        });

    let workspace_root = manifest_dir
        .parent() // crates/
        .and_then(|p| p.parent()) // workspace root
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| manifest_dir.clone());

    Ok(workspace_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_validation::models::{EntityRef, Metrics, Relationship, RelationshipType};
    use std::collections::HashMap;

    #[test]
    fn test_write_report_content() {
        let result = ComparisonResult {
            metrics: Metrics::calculate(5, 2, 3),
            metrics_by_type: {
                let mut m = HashMap::new();
                m.insert(RelationshipType::Calls, Metrics::calculate(3, 1, 2));
                m.insert(RelationshipType::Uses, Metrics::calculate(2, 1, 1));
                m
            },
            true_positives: vec![Relationship::new(
                EntityRef::new("a"),
                EntityRef::new("b"),
                RelationshipType::Calls,
            )],
            false_positives: vec![Relationship::new(
                EntityRef::new("x"),
                EntityRef::new("y"),
                RelationshipType::Uses,
            )],
            false_negatives: vec![Relationship::new(
                EntityRef::new("p"),
                EntityRef::new("q"),
                RelationshipType::Contains,
            )],
        };

        let mut output = Vec::new();
        write_report_content(&mut output, &result, "test-repo").unwrap();

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("Graph Validation Report"));
        assert!(output_str.contains("test-repo"));
        assert!(output_str.contains("Precision"));
        assert!(output_str.contains("False Positives"));
        assert!(output_str.contains("False Negatives"));
    }
}
