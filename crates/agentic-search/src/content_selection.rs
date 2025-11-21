//! Intelligent content selection for reranking prompts
//!
//! PRIVATE MODULE - Not exported from crate

// Allow dead code until Phase 2 implementation
#![allow(dead_code)]

use codesearch_core::search_models::EntityResult;

#[derive(Debug, Clone, Copy)]
pub enum RerankStage {
    Worker,
    CrossWorker,
    FullContentFallback,
    QualityGate,
}

const SHORT_ENTITY_THRESHOLD: usize = 30;
const QUALITY_GATE_FULL_THRESHOLD: usize = 100;

pub fn select_content_for_reranking(entity: &EntityResult, stage: RerankStage) -> String {
    let mut content = format_signature_and_doc(entity);
    let line_count = entity.content.as_ref().map_or(0, |c| c.lines().count());

    match (stage, line_count) {
        (_, n) if n < SHORT_ENTITY_THRESHOLD => {
            if let Some(ref body) = entity.content {
                content.push_str("\n\n");
                content.push_str(body);
            }
        }
        (RerankStage::Worker, _) => {
            if let Some(ref body) = entity.content {
                content.push_str("\n\n");
                content.push_str(&stratified_sample(body, 15));
            }
        }
        (RerankStage::CrossWorker, _) => {
            if let Some(ref body) = entity.content {
                content.push_str("\n\n");
                content.push_str(&stratified_sample(body, 20));
            }
        }
        (RerankStage::FullContentFallback, _) => {
            if let Some(ref body) = entity.content {
                content.push_str("\n\n");
                content.push_str(body);
                content.push_str("\n// [FULL CONTENT PROVIDED]");
            }
        }
        (RerankStage::QualityGate, n) if n < QUALITY_GATE_FULL_THRESHOLD => {
            if let Some(ref body) = entity.content {
                content.push_str("\n\n");
                content.push_str(body);
            }
        }
        (RerankStage::QualityGate, _) => {
            if let Some(ref body) = entity.content {
                content.push_str("\n\n");
                content.push_str(&stratified_sample(body, 40));
            }
        }
    }

    content
}

fn format_signature_and_doc(entity: &EntityResult) -> String {
    let mut content = String::new();

    if let Some(ref sig) = entity.signature {
        // Format function signature manually
        let params = sig
            .parameters
            .iter()
            .map(|(name, ty)| {
                if let Some(t) = ty {
                    format!("{name}: {t}")
                } else {
                    name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");

        let return_type = sig
            .return_type
            .as_ref()
            .map(|t| format!(" -> {t}"))
            .unwrap_or_default();

        let async_prefix = if sig.is_async { "async " } else { "" };

        let generics = if sig.generics.is_empty() {
            String::new()
        } else {
            format!("<{}>", sig.generics.join(", "))
        };

        content.push_str(&format!(
            "{}fn {}{}({}){}\n",
            async_prefix, entity.name, generics, params, return_type
        ));
    } else {
        content.push_str(&format!("{}\n", entity.qualified_name));
    }

    if let Some(ref doc) = entity.documentation_summary {
        let truncated = if doc.len() > 200 {
            format!("{}...", &doc[..200])
        } else {
            doc.clone()
        };
        content.push_str(&format!("// {truncated}\n"));
    }

    content
}

pub fn stratified_sample(content: &str, max_lines: usize) -> String {
    let lines: Vec<_> = content.lines().collect();
    let n = lines.len();

    if n <= max_lines {
        return content.to_string();
    }

    let num_segments = 3;
    let lines_per_segment = max_lines / num_segments;

    // CRITICAL FIX: Guard against division by zero when max_lines < num_segments
    if lines_per_segment == 0 {
        // Fallback: just take first max_lines
        return lines
            .iter()
            .take(max_lines)
            .enumerate()
            .map(|(idx, line)| format!("{:4} | {}", idx + 1, line))
            .collect::<Vec<_>>()
            .join("\n");
    }

    let segment_size = n / num_segments;
    let mut sampled = Vec::new();

    for segment_idx in 0..num_segments {
        let segment_start = segment_idx * segment_size;
        let segment_end = if segment_idx == num_segments - 1 {
            n
        } else {
            (segment_idx + 1) * segment_size
        };

        // This is now safe because lines_per_segment > 0
        let step = (segment_end - segment_start) / lines_per_segment;

        for i in 0..lines_per_segment {
            let line_idx = segment_start + i * step;
            if line_idx < segment_end {
                sampled.push((line_idx, lines[line_idx]));
            }
        }
    }

    sampled
        .iter()
        .map(|(num, line)| format!("{:4} | {}", num + 1, line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn calculate_confidence(results: &[EntityResult]) -> f32 {
    if results.len() < 5 {
        return 0.0;
    }

    let top_5_scores: Vec<f32> = results[..5].iter().map(|r| r.score).collect();
    let avg_score = top_5_scores.iter().sum::<f32>() / 5.0;

    let score_variance = top_5_scores
        .iter()
        .map(|s| (s - avg_score).powi(2))
        .sum::<f32>()
        / 5.0;

    if avg_score > 0.8 && score_variance < 0.05 {
        0.95
    } else if avg_score > 0.7 && score_variance < 0.1 {
        0.75
    } else if avg_score < 0.5 || score_variance > 0.2 {
        0.3
    } else {
        0.6
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stratified_sample_coverage() {
        let content = (1..=60)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = stratified_sample(&content, 15);
        let lines: Vec<_> = result.lines().collect();

        // Should sample from beginning, middle, and end
        assert!(lines.len() >= 14 && lines.len() <= 15);

        // First line should be from the beginning
        assert!(lines[0].contains("line 1") || lines[0].contains("line 2"));

        // Last line should be from the last segment (lines 40-59)
        // With 3 segments and 60 lines: segment 2 is lines 40-59
        // Sampling 5 lines per segment with step=4: 40, 44, 48, 52, 56
        let last = lines.last().unwrap();
        assert!(last.contains("line 56") || last.contains("line 57") || last.contains("line 52"));
    }

    #[test]
    fn test_stratified_sample_low_max_lines() {
        // CRITICAL: Test division by zero guard
        let content = (1..=60)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");

        // max_lines < num_segments (3) should not panic
        let result = stratified_sample(&content, 2);
        let lines: Vec<_> = result.lines().collect();

        assert_eq!(lines.len(), 2, "Should return exactly 2 lines");
        assert!(lines[0].contains("line 1"));
        assert!(lines[1].contains("line 2"));
    }

    #[test]
    fn test_stratified_sample_single_line() {
        let content = (1..=60)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = stratified_sample(&content, 1);
        let lines: Vec<_> = result.lines().collect();

        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("line 1"));
    }

    #[test]
    fn test_stratified_sample_short_content() {
        let content = "line 1\nline 2\nline 3";
        let result = stratified_sample(content, 15);
        assert_eq!(result, content);
    }

    #[test]
    fn test_confidence_high() {
        use codesearch_core::entities::{EntityType, Language, SourceLocation, Visibility};
        use codesearch_core::search_models::EntityResult;
        use uuid::Uuid;

        let results = vec![
            EntityResult {
                entity_id: "1".to_string(),
                repository_id: Uuid::new_v4(),
                qualified_name: "test".to_string(),
                name: "test".to_string(),
                entity_type: EntityType::Function,
                language: Language::Rust,
                file_path: "test.rs".to_string(),
                location: SourceLocation {
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 0,
                },
                content: None,
                signature: None,
                documentation_summary: None,
                visibility: Visibility::Public,
                score: 0.85,
                reranked: false,
                reasoning: None,
            },
            EntityResult {
                entity_id: "2".to_string(),
                repository_id: Uuid::new_v4(),
                qualified_name: "test".to_string(),
                name: "test".to_string(),
                entity_type: EntityType::Function,
                language: Language::Rust,
                file_path: "test.rs".to_string(),
                location: SourceLocation {
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 0,
                },
                content: None,
                signature: None,
                documentation_summary: None,
                visibility: Visibility::Public,
                score: 0.83,
                reranked: false,
                reasoning: None,
            },
            EntityResult {
                entity_id: "3".to_string(),
                repository_id: Uuid::new_v4(),
                qualified_name: "test".to_string(),
                name: "test".to_string(),
                entity_type: EntityType::Function,
                language: Language::Rust,
                file_path: "test.rs".to_string(),
                location: SourceLocation {
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 0,
                },
                content: None,
                signature: None,
                documentation_summary: None,
                visibility: Visibility::Public,
                score: 0.84,
                reranked: false,
                reasoning: None,
            },
            EntityResult {
                entity_id: "4".to_string(),
                repository_id: Uuid::new_v4(),
                qualified_name: "test".to_string(),
                name: "test".to_string(),
                entity_type: EntityType::Function,
                language: Language::Rust,
                file_path: "test.rs".to_string(),
                location: SourceLocation {
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 0,
                },
                content: None,
                signature: None,
                documentation_summary: None,
                visibility: Visibility::Public,
                score: 0.82,
                reranked: false,
                reasoning: None,
            },
            EntityResult {
                entity_id: "5".to_string(),
                repository_id: Uuid::new_v4(),
                qualified_name: "test".to_string(),
                name: "test".to_string(),
                entity_type: EntityType::Function,
                language: Language::Rust,
                file_path: "test.rs".to_string(),
                location: SourceLocation {
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 0,
                },
                content: None,
                signature: None,
                documentation_summary: None,
                visibility: Visibility::Public,
                score: 0.86,
                reranked: false,
                reasoning: None,
            },
        ];

        assert!(calculate_confidence(&results) > 0.9);
    }

    #[test]
    fn test_confidence_low() {
        use codesearch_core::entities::{EntityType, Language, SourceLocation, Visibility};
        use codesearch_core::search_models::EntityResult;
        use uuid::Uuid;

        let results = vec![
            EntityResult {
                entity_id: "1".to_string(),
                repository_id: Uuid::new_v4(),
                qualified_name: "test".to_string(),
                name: "test".to_string(),
                entity_type: EntityType::Function,
                language: Language::Rust,
                file_path: "test.rs".to_string(),
                location: SourceLocation {
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 0,
                },
                content: None,
                signature: None,
                documentation_summary: None,
                visibility: Visibility::Public,
                score: 0.3,
                reranked: false,
                reasoning: None,
            },
            EntityResult {
                entity_id: "2".to_string(),
                repository_id: Uuid::new_v4(),
                qualified_name: "test".to_string(),
                name: "test".to_string(),
                entity_type: EntityType::Function,
                language: Language::Rust,
                file_path: "test.rs".to_string(),
                location: SourceLocation {
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 0,
                },
                content: None,
                signature: None,
                documentation_summary: None,
                visibility: Visibility::Public,
                score: 0.4,
                reranked: false,
                reasoning: None,
            },
            EntityResult {
                entity_id: "3".to_string(),
                repository_id: Uuid::new_v4(),
                qualified_name: "test".to_string(),
                name: "test".to_string(),
                entity_type: EntityType::Function,
                language: Language::Rust,
                file_path: "test.rs".to_string(),
                location: SourceLocation {
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 0,
                },
                content: None,
                signature: None,
                documentation_summary: None,
                visibility: Visibility::Public,
                score: 0.5,
                reranked: false,
                reasoning: None,
            },
            EntityResult {
                entity_id: "4".to_string(),
                repository_id: Uuid::new_v4(),
                qualified_name: "test".to_string(),
                name: "test".to_string(),
                entity_type: EntityType::Function,
                language: Language::Rust,
                file_path: "test.rs".to_string(),
                location: SourceLocation {
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 0,
                },
                content: None,
                signature: None,
                documentation_summary: None,
                visibility: Visibility::Public,
                score: 0.2,
                reranked: false,
                reasoning: None,
            },
            EntityResult {
                entity_id: "5".to_string(),
                repository_id: Uuid::new_v4(),
                qualified_name: "test".to_string(),
                name: "test".to_string(),
                entity_type: EntityType::Function,
                language: Language::Rust,
                file_path: "test.rs".to_string(),
                location: SourceLocation {
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 0,
                },
                content: None,
                signature: None,
                documentation_summary: None,
                visibility: Visibility::Public,
                score: 0.6,
                reranked: false,
                reasoning: None,
            },
        ];

        assert!(calculate_confidence(&results) < 0.5);
    }
}
