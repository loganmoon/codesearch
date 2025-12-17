//! HuggingFace tokenizer wrapper for Granite sparse model

use codesearch_core::error::{Error, Result};
use std::path::Path;
use tokenizers::Tokenizer;

/// Wrapper around HuggingFace tokenizer for Granite model
pub struct GraniteTokenizer {
    tokenizer: Tokenizer,
}

/// Tokenized output
#[derive(Clone)]
pub struct TokenizedInput {
    /// Token IDs
    pub input_ids: Vec<u32>,
    /// Attention mask (1 for real tokens, 0 for padding)
    pub attention_mask: Vec<u32>,
}

impl GraniteTokenizer {
    /// Load tokenizer from a file path
    pub fn from_file(tokenizer_path: &Path) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(|e| {
            Error::embedding(format!(
                "Failed to load tokenizer from {}: {e}",
                tokenizer_path.display()
            ))
        })?;

        Ok(Self { tokenizer })
    }

    /// Encode a single text into token IDs and attention mask
    pub fn encode(&self, text: &str, max_length: usize) -> Result<TokenizedInput> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| Error::embedding(format!("Tokenization failed: {e}")))?;

        let mut input_ids: Vec<u32> = encoding.get_ids().to_vec();
        let mut attention_mask: Vec<u32> = encoding.get_attention_mask().to_vec();

        // Truncate if necessary
        if input_ids.len() > max_length {
            input_ids.truncate(max_length);
            attention_mask.truncate(max_length);
        }

        Ok(TokenizedInput {
            input_ids,
            attention_mask,
        })
    }

    /// Encode multiple texts with padding to the same length
    pub fn encode_batch(&self, texts: &[&str], max_length: usize) -> Result<Vec<TokenizedInput>> {
        let mut results = Vec::with_capacity(texts.len());

        for text in texts {
            results.push(self.encode(text, max_length)?);
        }

        // Find max length in batch for padding
        let max_batch_len = results
            .iter()
            .map(|t| t.input_ids.len())
            .max()
            .unwrap_or(0)
            .min(max_length);

        // Pad all sequences to the same length
        let pad_token_id = self.pad_token_id();
        for result in &mut results {
            while result.input_ids.len() < max_batch_len {
                result.input_ids.push(pad_token_id);
                result.attention_mask.push(0);
            }
        }

        Ok(results)
    }

    /// Get the padding token ID
    pub fn pad_token_id(&self) -> u32 {
        // RoBERTa uses <pad> token with ID 1
        self.tokenizer.token_to_id("<pad>").unwrap_or(1)
    }

    /// Get the vocabulary size
    #[allow(dead_code)]
    pub fn vocab_size(&self) -> usize {
        self.tokenizer.get_vocab_size(true)
    }

    /// Decode token IDs back to text
    #[allow(dead_code)]
    pub fn decode(&self, ids: &[u32], skip_special_tokens: bool) -> Result<String> {
        self.tokenizer
            .decode(ids, skip_special_tokens)
            .map_err(|e| Error::embedding(format!("Decoding failed: {e}")))
    }

    /// Get token text for a given ID
    #[allow(dead_code)]
    pub fn id_to_token(&self, id: u32) -> Option<String> {
        self.tokenizer.id_to_token(id)
    }
}

#[cfg(test)]
mod tests {
    // Tests require downloading the tokenizer, so we skip them in CI
    // These tests serve as documentation for the expected behavior
}
