//! Configuration for agentic search

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticSearchConfig {
    pub api_key: Option<String>,
    pub orchestrator_model: String,
    pub worker_model: String,
    pub max_workers: usize,
    pub timeout_secs: u64,
    pub quality_gate: QualityGateConfig,
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
            max_workers: 5,
            timeout_secs: 120,
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
        if self.max_workers == 0 {
            return Err("max_workers must be greater than 0".to_string());
        }
        if self.max_workers > 10 {
            return Err("max_workers cannot exceed 10".to_string());
        }
        if self.timeout_secs == 0 {
            return Err("timeout_secs must be greater than 0".to_string());
        }
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
    fn test_config_validation_workers_zero() {
        let config = AgenticSearchConfig {
            max_workers: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_workers_too_high() {
        let config = AgenticSearchConfig {
            max_workers: 11,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

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
        assert_eq!(config.max_workers, 5);
        assert_eq!(config.orchestrator_model, "claude-sonnet-4-5");
        assert_eq!(config.worker_model, "claude-haiku-4-5");
        assert_eq!(config.timeout_secs, 120);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_api_key_resolution() {
        std::env::set_var("ANTHROPIC_API_KEY", "test-key");
        let config = AgenticSearchConfig::default();
        assert_eq!(config.resolve_api_key(), Some("test-key".to_string()));
        std::env::remove_var("ANTHROPIC_API_KEY");
    }
}
