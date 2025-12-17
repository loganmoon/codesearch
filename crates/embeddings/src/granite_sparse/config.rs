//! Granite sparse model configuration

use codesearch_core::error::{Error, Result};
use serde::Deserialize;
use std::path::Path;

/// Configuration for the Granite sparse embedding model
#[derive(Debug, Clone, Deserialize)]
pub struct GraniteSparseConfig {
    /// Vocabulary size (default: 50265 for Granite)
    #[serde(default = "default_vocab_size")]
    pub vocab_size: usize,

    /// Hidden layer size (default: 384)
    #[serde(default = "default_hidden_size")]
    pub hidden_size: usize,

    /// Number of transformer layers (default: 6)
    #[serde(default = "default_num_hidden_layers")]
    pub num_hidden_layers: usize,

    /// Number of attention heads (default: 12)
    #[serde(default = "default_num_attention_heads")]
    pub num_attention_heads: usize,

    /// Intermediate (feedforward) layer size (default: 1536)
    #[serde(default = "default_intermediate_size")]
    pub intermediate_size: usize,

    /// Maximum sequence length (default: 512)
    #[serde(default = "default_max_position_embeddings")]
    pub max_position_embeddings: usize,

    /// Hidden activation function (default: "gelu")
    #[serde(default = "default_hidden_act")]
    pub hidden_act: String,

    /// Layer normalization epsilon (default: 1e-5)
    #[serde(default = "default_layer_norm_eps")]
    pub layer_norm_eps: f64,

    /// Attention dropout probability
    #[serde(default)]
    pub attention_probs_dropout_prob: f64,

    /// Hidden layer dropout probability
    #[serde(default)]
    pub hidden_dropout_prob: f64,

    /// Padding token ID (default: 1)
    #[serde(default = "default_pad_token_id")]
    pub pad_token_id: usize,

    /// Type vocabulary size (default: 1)
    #[serde(default = "default_type_vocab_size")]
    pub type_vocab_size: usize,
}

fn default_vocab_size() -> usize {
    50265
}

fn default_hidden_size() -> usize {
    384
}

fn default_num_hidden_layers() -> usize {
    6
}

fn default_num_attention_heads() -> usize {
    12
}

fn default_intermediate_size() -> usize {
    1536
}

fn default_max_position_embeddings() -> usize {
    512
}

fn default_hidden_act() -> String {
    "gelu".to_string()
}

fn default_layer_norm_eps() -> f64 {
    1e-5
}

fn default_pad_token_id() -> usize {
    1
}

fn default_type_vocab_size() -> usize {
    2 // Match actual Granite model
}

impl Default for GraniteSparseConfig {
    fn default() -> Self {
        Self {
            vocab_size: default_vocab_size(),
            hidden_size: default_hidden_size(),
            num_hidden_layers: default_num_hidden_layers(),
            num_attention_heads: default_num_attention_heads(),
            intermediate_size: default_intermediate_size(),
            max_position_embeddings: default_max_position_embeddings(),
            hidden_act: default_hidden_act(),
            layer_norm_eps: default_layer_norm_eps(),
            attention_probs_dropout_prob: 0.1,
            hidden_dropout_prob: 0.1,
            pad_token_id: default_pad_token_id(),
            type_vocab_size: default_type_vocab_size(),
        }
    }
}

impl GraniteSparseConfig {
    /// Load configuration from a JSON file
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            Error::embedding(format!(
                "Failed to read config file {}: {e}",
                path.display()
            ))
        })?;

        serde_json::from_str(&content).map_err(|e| {
            Error::embedding(format!(
                "Failed to parse config file {}: {e}",
                path.display()
            ))
        })
    }

    /// Calculate the attention head dimension
    pub fn head_dim(&self) -> usize {
        self.hidden_size / self.num_attention_heads
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = GraniteSparseConfig::default();
        assert_eq!(config.vocab_size, 50265);
        assert_eq!(config.hidden_size, 384);
        assert_eq!(config.num_hidden_layers, 6);
        assert_eq!(config.num_attention_heads, 12);
        assert_eq!(config.intermediate_size, 1536);
        assert_eq!(config.head_dim(), 32);
    }

    #[test]
    fn test_config_from_json() {
        let json = r#"{
            "vocab_size": 50265,
            "hidden_size": 384,
            "num_hidden_layers": 6,
            "num_attention_heads": 12,
            "intermediate_size": 1536,
            "max_position_embeddings": 512,
            "hidden_act": "gelu",
            "layer_norm_eps": 1e-5
        }"#;

        let config: GraniteSparseConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.vocab_size, 50265);
        assert_eq!(config.hidden_size, 384);
    }
}
