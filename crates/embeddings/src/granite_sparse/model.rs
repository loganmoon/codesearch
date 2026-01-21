//! SPLADE-style sparse embedding model using Candle
//!
//! Implements the Granite sparse embedding model architecture:
//! - RoBERTa-like encoder (6 layers, 12 heads)
//! - MLM head for token-level predictions
//! - SPLADE pooling: log(1 + ReLU(x)) with max pooling across sequence

use super::config::GraniteSparseConfig;
use candle_core::{DType, Device, Module, Result, Tensor};
use candle_nn::{
    embedding, layer_norm, linear, Activation, Embedding, LayerNorm, Linear, VarBuilder,
};

/// Layer normalization with configurable epsilon
fn layer_norm_config(hidden_size: usize, eps: f64, vb: VarBuilder) -> Result<LayerNorm> {
    layer_norm(
        hidden_size,
        candle_nn::LayerNormConfig {
            eps,
            ..Default::default()
        },
        vb,
    )
}

/// RoBERTa-style embeddings layer
pub struct GraniteEmbeddings {
    word_embeddings: Embedding,
    position_embeddings: Embedding,
    token_type_embeddings: Embedding,
    layer_norm: LayerNorm,
}

impl GraniteEmbeddings {
    pub fn new(config: &GraniteSparseConfig, vb: VarBuilder) -> Result<Self> {
        let word_embeddings = embedding(
            config.vocab_size,
            config.hidden_size,
            vb.pp("word_embeddings"),
        )?;
        let position_embeddings = embedding(
            config.max_position_embeddings,
            config.hidden_size,
            vb.pp("position_embeddings"),
        )?;
        let token_type_embeddings = embedding(
            config.type_vocab_size,
            config.hidden_size,
            vb.pp("token_type_embeddings"),
        )?;
        let layer_norm = layer_norm_config(
            config.hidden_size,
            config.layer_norm_eps,
            vb.pp("LayerNorm"),
        )?;

        Ok(Self {
            word_embeddings,
            position_embeddings,
            token_type_embeddings,
            layer_norm,
        })
    }

    pub fn forward(&self, input_ids: &Tensor, token_type_ids: Option<&Tensor>) -> Result<Tensor> {
        let seq_len = input_ids.dim(1)?;
        let device = input_ids.device();

        // Word embeddings
        let word_embeds = self.word_embeddings.forward(input_ids)?;

        // Position IDs: 0, 1, 2, ..., seq_len-1
        let position_ids = Tensor::arange(0u32, seq_len as u32, device)?
            .unsqueeze(0)?
            .expand((input_ids.dim(0)?, seq_len))?;
        let position_embeds = self.position_embeddings.forward(&position_ids)?;

        // Token type embeddings (usually all zeros for single-sentence tasks)
        let token_type_embeds = match token_type_ids {
            Some(ids) => self.token_type_embeddings.forward(ids)?,
            None => {
                let zeros = Tensor::zeros((input_ids.dim(0)?, seq_len), DType::U32, device)?;
                self.token_type_embeddings.forward(&zeros)?
            }
        };

        // Sum all embeddings and apply layer norm
        let embeddings = (word_embeds + position_embeds + token_type_embeds)?;
        self.layer_norm.forward(&embeddings)
    }
}

/// Self-attention layer
pub struct GraniteSelfAttention {
    query: Linear,
    key: Linear,
    value: Linear,
    num_attention_heads: usize,
    head_dim: usize,
}

impl GraniteSelfAttention {
    pub fn new(config: &GraniteSparseConfig, vb: VarBuilder) -> Result<Self> {
        let hidden_size = config.hidden_size;
        let query = linear(hidden_size, hidden_size, vb.pp("query"))?;
        let key = linear(hidden_size, hidden_size, vb.pp("key"))?;
        let value = linear(hidden_size, hidden_size, vb.pp("value"))?;

        Ok(Self {
            query,
            key,
            value,
            num_attention_heads: config.num_attention_heads,
            head_dim: config.head_dim(),
        })
    }

    pub fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let (batch_size, seq_len, _) = hidden_states.dims3()?;

        // Project to Q, K, V
        let query = self.query.forward(hidden_states)?;
        let key = self.key.forward(hidden_states)?;
        let value = self.value.forward(hidden_states)?;

        // Reshape for multi-head attention: (batch, seq, heads, head_dim)
        // Note: contiguous() is required after transpose for matmul compatibility
        let query = query
            .reshape((batch_size, seq_len, self.num_attention_heads, self.head_dim))?
            .transpose(1, 2)?
            .contiguous()?; // (batch, heads, seq, head_dim)
        let key = key
            .reshape((batch_size, seq_len, self.num_attention_heads, self.head_dim))?
            .transpose(1, 2)?
            .contiguous()?;
        let value = value
            .reshape((batch_size, seq_len, self.num_attention_heads, self.head_dim))?
            .transpose(1, 2)?
            .contiguous()?;

        // Scaled dot-product attention
        let scale = (self.head_dim as f64).sqrt();
        let key_t = key.transpose(2, 3)?.contiguous()?;
        let attention_scores = (query.matmul(&key_t)? / scale)?;

        // Apply attention mask if provided
        let attention_scores = match attention_mask {
            Some(mask) => {
                // Mask shape: (batch, seq) -> (batch, 1, 1, seq) for broadcasting
                let mask = mask.unsqueeze(1)?.unsqueeze(1)?;
                // Convert to f32 and create bias: 1 (valid) -> 0, 0 (padding) -> -10000
                let mask_f32 = mask.to_dtype(attention_scores.dtype())?;
                let mask_bias = ((1.0 - mask_f32)? * (-10000.0f64))?;
                // Use broadcast_add for automatic shape broadcasting
                attention_scores.broadcast_add(&mask_bias)?
            }
            None => attention_scores,
        };

        // Softmax
        let attention_probs = candle_nn::ops::softmax(&attention_scores, 3)?;

        // Apply attention to values
        let context = attention_probs.matmul(&value)?;

        // Reshape back: (batch, heads, seq, head_dim) -> (batch, seq, hidden)
        // contiguous() required after transpose for reshape compatibility
        context.transpose(1, 2)?.contiguous()?.reshape((
            batch_size,
            seq_len,
            self.num_attention_heads * self.head_dim,
        ))
    }
}

/// Self-attention output projection with residual connection
pub struct GraniteSelfOutput {
    dense: Linear,
    layer_norm: LayerNorm,
}

impl GraniteSelfOutput {
    pub fn new(config: &GraniteSparseConfig, vb: VarBuilder) -> Result<Self> {
        let dense = linear(config.hidden_size, config.hidden_size, vb.pp("dense"))?;
        let layer_norm = layer_norm_config(
            config.hidden_size,
            config.layer_norm_eps,
            vb.pp("LayerNorm"),
        )?;

        Ok(Self { dense, layer_norm })
    }

    pub fn forward(&self, hidden_states: &Tensor, input_tensor: &Tensor) -> Result<Tensor> {
        let hidden_states = self.dense.forward(hidden_states)?;
        self.layer_norm.forward(&(hidden_states + input_tensor)?)
    }
}

/// Combined attention layer
pub struct GraniteAttention {
    self_attention: GraniteSelfAttention,
    output: GraniteSelfOutput,
}

impl GraniteAttention {
    pub fn new(config: &GraniteSparseConfig, vb: VarBuilder) -> Result<Self> {
        let self_attention = GraniteSelfAttention::new(config, vb.pp("self"))?;
        let output = GraniteSelfOutput::new(config, vb.pp("output"))?;

        Ok(Self {
            self_attention,
            output,
        })
    }

    pub fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let self_output = self.self_attention.forward(hidden_states, attention_mask)?;
        self.output.forward(&self_output, hidden_states)
    }
}

/// Feed-forward intermediate layer
pub struct GraniteIntermediate {
    dense: Linear,
    activation: Activation,
}

impl GraniteIntermediate {
    pub fn new(config: &GraniteSparseConfig, vb: VarBuilder) -> Result<Self> {
        let dense = linear(config.hidden_size, config.intermediate_size, vb.pp("dense"))?;
        let activation = match config.hidden_act.as_str() {
            "gelu" => Activation::Gelu,
            "relu" => Activation::Relu,
            _ => Activation::Gelu, // Default to GELU
        };

        Ok(Self { dense, activation })
    }

    pub fn forward(&self, hidden_states: &Tensor) -> Result<Tensor> {
        let hidden_states = self.dense.forward(hidden_states)?;
        self.activation.forward(&hidden_states)
    }
}

/// Feed-forward output layer with residual connection
pub struct GraniteOutput {
    dense: Linear,
    layer_norm: LayerNorm,
}

impl GraniteOutput {
    pub fn new(config: &GraniteSparseConfig, vb: VarBuilder) -> Result<Self> {
        let dense = linear(config.intermediate_size, config.hidden_size, vb.pp("dense"))?;
        let layer_norm = layer_norm_config(
            config.hidden_size,
            config.layer_norm_eps,
            vb.pp("LayerNorm"),
        )?;

        Ok(Self { dense, layer_norm })
    }

    pub fn forward(&self, hidden_states: &Tensor, input_tensor: &Tensor) -> Result<Tensor> {
        let hidden_states = self.dense.forward(hidden_states)?;
        self.layer_norm.forward(&(hidden_states + input_tensor)?)
    }
}

/// Single transformer layer
pub struct GraniteLayer {
    attention: GraniteAttention,
    intermediate: GraniteIntermediate,
    output: GraniteOutput,
}

impl GraniteLayer {
    pub fn new(config: &GraniteSparseConfig, vb: VarBuilder) -> Result<Self> {
        let attention = GraniteAttention::new(config, vb.pp("attention"))?;
        let intermediate = GraniteIntermediate::new(config, vb.pp("intermediate"))?;
        let output = GraniteOutput::new(config, vb.pp("output"))?;

        Ok(Self {
            attention,
            intermediate,
            output,
        })
    }

    pub fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let attention_output = self.attention.forward(hidden_states, attention_mask)?;
        let intermediate_output = self.intermediate.forward(&attention_output)?;
        self.output.forward(&intermediate_output, &attention_output)
    }
}

/// Transformer encoder stack
pub struct GraniteEncoder {
    layers: Vec<GraniteLayer>,
}

impl GraniteEncoder {
    pub fn new(config: &GraniteSparseConfig, vb: VarBuilder) -> Result<Self> {
        let mut layers = Vec::with_capacity(config.num_hidden_layers);
        for i in 0..config.num_hidden_layers {
            layers.push(GraniteLayer::new(config, vb.pp(format!("layer.{i}")))?);
        }

        Ok(Self { layers })
    }

    pub fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let mut hidden_states = hidden_states.clone();
        for layer in &self.layers {
            hidden_states = layer.forward(&hidden_states, attention_mask)?;
        }
        Ok(hidden_states)
    }
}

/// MLM prediction head for SPLADE
///
/// Note: In RoBERTa, the decoder.weight is tied to the word embeddings.
/// We handle this by accepting the word embeddings tensor directly.
pub struct GraniteLMHead {
    dense: Linear,
    layer_norm: LayerNorm,
    decoder_weight: Tensor,
    decoder_bias: Tensor,
}

impl GraniteLMHead {
    pub fn new(
        config: &GraniteSparseConfig,
        vb: VarBuilder,
        word_embeddings_weight: Tensor,
    ) -> Result<Self> {
        let dense = linear(config.hidden_size, config.hidden_size, vb.pp("dense"))?;
        let layer_norm = layer_norm_config(
            config.hidden_size,
            config.layer_norm_eps,
            vb.pp("layer_norm"),
        )?;

        // In RoBERTa, lm_head.decoder.weight is tied to roberta.embeddings.word_embeddings.weight
        // The bias is stored separately
        let decoder_bias = vb.get(config.vocab_size, "bias")?;

        Ok(Self {
            dense,
            layer_norm,
            decoder_weight: word_embeddings_weight,
            decoder_bias,
        })
    }

    pub fn forward(&self, hidden_states: &Tensor) -> Result<Tensor> {
        let hidden_states = self.dense.forward(hidden_states)?;
        let hidden_states = Activation::Gelu.forward(&hidden_states)?;
        let hidden_states = self.layer_norm.forward(&hidden_states)?;

        // Manual linear: x @ W^T + b
        // decoder_weight shape: (vocab_size, hidden_size)
        // hidden_states shape: (batch, seq, hidden_size)
        // We need: (batch, seq, hidden_size) @ (hidden_size, vocab_size) = (batch, seq, vocab_size)
        // Use broadcast_matmul to handle 3D × 2D broadcasting correctly
        let output = hidden_states.broadcast_matmul(&self.decoder_weight.t()?)?;
        output.broadcast_add(&self.decoder_bias)
    }
}

/// Complete Granite SPLADE model
pub struct GraniteSparseModel {
    embeddings: GraniteEmbeddings,
    encoder: GraniteEncoder,
    lm_head: GraniteLMHead,
    config: GraniteSparseConfig,
}

impl GraniteSparseModel {
    /// Load model from safetensors file
    pub fn load(
        config: GraniteSparseConfig,
        model_path: &std::path::Path,
        device: &Device,
    ) -> Result<Self> {
        let model_data = std::fs::read(model_path).map_err(|e| {
            candle_core::Error::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to read model file {}: {e}", model_path.display()),
            ))
        })?;
        let vb = VarBuilder::from_buffered_safetensors(model_data, DType::F32, device)?;

        Self::new(&config, vb)
    }

    /// Create model from VarBuilder
    pub fn new(config: &GraniteSparseConfig, vb: VarBuilder) -> Result<Self> {
        // Load word embeddings weight first - it's needed for both embeddings and LM head (weight tying)
        let word_embeddings_weight = vb.get(
            (config.vocab_size, config.hidden_size),
            "roberta.embeddings.word_embeddings.weight",
        )?;

        let embeddings = GraniteEmbeddings::new(config, vb.pp("roberta.embeddings"))?;
        let encoder = GraniteEncoder::new(config, vb.pp("roberta.encoder"))?;
        let lm_head = GraniteLMHead::new(config, vb.pp("lm_head"), word_embeddings_weight.clone())?;

        Ok(Self {
            embeddings,
            encoder,
            lm_head,
            config: config.clone(),
        })
    }

    /// Forward pass through encoder
    pub fn forward(&self, input_ids: &Tensor, attention_mask: Option<&Tensor>) -> Result<Tensor> {
        let embeddings = self.embeddings.forward(input_ids, None)?;
        let encoder_output = self.encoder.forward(&embeddings, attention_mask)?;
        self.lm_head.forward(&encoder_output)
    }

    /// Generate sparse embeddings using SPLADE pooling
    ///
    /// SPLADE pooling: log(1 + ReLU(x)) with max pooling across sequence
    /// Returns sparse vector as Vec<(token_id, weight)>
    pub fn embed_sparse(
        &self,
        input_ids: &Tensor,
        attention_mask: &Tensor,
        top_k: usize,
    ) -> Result<Vec<Vec<(u32, f32)>>> {
        // Get MLM logits: (batch, seq, vocab)
        let logits = self.forward(input_ids, Some(attention_mask))?;

        // SPLADE activation: log(1 + ReLU(x))
        let relu_logits = logits.relu()?;
        let splade_scores = (relu_logits + 1.0)?.log()?;

        // Apply attention mask to zero out padding tokens
        // Expand mask: (batch, seq) -> (batch, seq, 1) for broadcasting
        let mask = attention_mask.unsqueeze(2)?;
        let mask = mask.to_dtype(splade_scores.dtype())?;
        // Use broadcast_mul for proper (batch, seq, vocab) * (batch, seq, 1) broadcasting
        let masked_scores = splade_scores.broadcast_mul(&mask)?;

        // Max pool across sequence dimension: (batch, seq, vocab) -> (batch, vocab)
        let pooled = masked_scores.max(1)?;

        // Convert to sparse vectors
        let batch_size = pooled.dim(0)?;
        let pooled_vec = pooled.to_vec2::<f32>()?;

        let results: Vec<Vec<(u32, f32)>> = pooled_vec
            .iter()
            .take(batch_size)
            .map(|scores| {
                // Collect non-zero scores with their indices
                let mut sparse: Vec<(u32, f32)> = scores
                    .iter()
                    .enumerate()
                    .filter(|(_, &score)| score > 0.0)
                    .map(|(idx, &score)| (idx as u32, score))
                    .collect();

                // Sort by score descending and take top-k
                sparse.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                sparse.truncate(top_k);
                sparse
            })
            .collect();

        Ok(results)
    }

    /// Get the model configuration
    pub fn config(&self) -> &GraniteSparseConfig {
        &self.config
    }
}
