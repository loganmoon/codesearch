//! Configuration for agentic search

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct AgenticSearchConfig {
    pub api_key: Option<String>,
    pub orchestrator_model: String,
    pub worker_model: String,
    pub quality_gate: QualityGateConfig,
}

impl std::fmt::Debug for AgenticSearchConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgenticSearchConfig")
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("orchestrator_model", &self.orchestrator_model)
            .field("worker_model", &self.worker_model)
            .field("quality_gate", &self.quality_gate)
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
            worker_model: "claude-haiku-4-5".to_string(),
            quality_gate: QualityGateConfig::default(),
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
        assert_eq!(config.worker_model, "claude-haiku-4-5");
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
}
