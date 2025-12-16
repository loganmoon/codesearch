//! Configuration for agentic search

use codesearch_core::config::{RerankingConfig, RerankingRequestConfig};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct AgenticSearchConfig {
    pub api_key: Option<String>,
    pub orchestrator_model: String,
    pub quality_gate: QualityGateConfig,
    /// Full reranking config for creating internal reranker (used for final synthesis)
    pub reranking: Option<RerankingConfig>,
    /// Request-level reranking overrides (passed to semantic search workers)
    pub reranking_request: Option<RerankingRequestConfig>,
    /// Number of candidates to fetch for reranking (default 100)
    pub semantic_candidates: usize,
}

impl std::fmt::Debug for AgenticSearchConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgenticSearchConfig")
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("orchestrator_model", &self.orchestrator_model)
            .field("quality_gate", &self.quality_gate)
            .field("reranking", &self.reranking.is_some())
            .field("reranking_request", &self.reranking_request)
            .field("semantic_candidates", &self.semantic_candidates)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityGateConfig {
    pub min_top5_avg_score: f32,
    pub min_entity_type_diversity: usize,
    pub min_file_path_diversity: usize,
    pub enabled: bool,
}

impl Default for AgenticSearchConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            orchestrator_model: "claude-sonnet-4-5".to_string(),
            quality_gate: QualityGateConfig::default(),
            reranking: None,
            reranking_request: None,
            semantic_candidates: 100,
        }
    }
}

impl Default for QualityGateConfig {
    fn default() -> Self {
        Self {
            min_top5_avg_score: 0.85,
            min_entity_type_diversity: 3,
            min_file_path_diversity: 5,
            enabled: true,
        }
    }
}

impl AgenticSearchConfig {
    /// Create config from application-level reranking configuration.
    ///
    /// This factory method eliminates duplication when constructing `AgenticSearchConfig`
    /// from the global `RerankingConfig` in different entry points (CLI, server, MCP).
    pub fn from_reranking_config(reranking: &RerankingConfig) -> Self {
        Self {
            reranking: if reranking.enabled {
                Some(reranking.clone())
            } else {
                None
            },
            reranking_request: if reranking.enabled {
                Some(RerankingRequestConfig {
                    enabled: Some(true),
                    candidates: Some(reranking.candidates),
                    top_k: Some(reranking.top_k),
                })
            } else {
                None
            },
            semantic_candidates: reranking.candidates,
            ..Default::default()
        }
    }

    pub fn resolve_api_key(&self) -> Option<String> {
        self.api_key
            .clone()
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.quality_gate.min_top5_avg_score < 0.0 || self.quality_gate.min_top5_avg_score > 1.0
        {
            return Err("min_top5_avg_score must be between 0.0 and 1.0".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation_score_range() {
        let config = AgenticSearchConfig {
            quality_gate: QualityGateConfig {
                min_top5_avg_score: 1.5,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_defaults() {
        let config = AgenticSearchConfig::default();
        assert_eq!(config.orchestrator_model, "claude-sonnet-4-5");
        assert_eq!(config.semantic_candidates, 100);
        assert!(config.reranking.is_none());
        assert!(config.reranking_request.is_none());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_api_key_resolution() {
        std::env::set_var("ANTHROPIC_API_KEY", "test-key");
        let config = AgenticSearchConfig::default();
        assert_eq!(config.resolve_api_key(), Some("test-key".to_string()));
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    #[test]
    fn test_debug_redacts_api_key() {
        let config = AgenticSearchConfig {
            api_key: Some("secret-api-key-12345".to_string()),
            ..Default::default()
        };
        let debug_output = format!("{config:?}");
        assert!(!debug_output.contains("secret-api-key-12345"));
        assert!(debug_output.contains("[REDACTED]"));
    }

    #[test]
    fn test_from_reranking_config_enabled() {
        let reranking = RerankingConfig {
            enabled: true,
            provider: "jina".to_string(),
            model: "jina-reranker-v2".to_string(),
            candidates: 50,
            top_k: 10,
            ..Default::default()
        };

        let config = AgenticSearchConfig::from_reranking_config(&reranking);

        // Verify reranking config is set
        assert!(config.reranking.is_some());
        let rerank = config.reranking.as_ref().unwrap();
        assert!(rerank.enabled);
        assert_eq!(rerank.candidates, 50);

        // Verify request config is set
        assert!(config.reranking_request.is_some());
        let req = config.reranking_request.as_ref().unwrap();
        assert_eq!(req.enabled, Some(true));
        assert_eq!(req.candidates, Some(50));
        assert_eq!(req.top_k, Some(10));

        // Verify semantic_candidates matches
        assert_eq!(config.semantic_candidates, 50);
    }

    #[test]
    fn test_from_reranking_config_disabled() {
        let reranking = RerankingConfig {
            enabled: false,
            candidates: 100,
            top_k: 20,
            ..Default::default()
        };

        let config = AgenticSearchConfig::from_reranking_config(&reranking);

        // Verify reranking is not set when disabled
        assert!(config.reranking.is_none());
        assert!(config.reranking_request.is_none());

        // semantic_candidates still reflects the reranking config value
        assert_eq!(config.semantic_candidates, 100);
    }
}
