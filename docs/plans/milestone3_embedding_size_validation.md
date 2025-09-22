# Milestone 3: Embedding Size Validation Implementation Plan

## Overview

This plan addresses the remaining 20% of Milestone 3: End-to-End Indexing Pipeline. The goal is to handle code entities that exceed embedding model context windows by skipping them rather than using arbitrary truncation, ensuring semantic integrity of indexed content.

## Current State Analysis

The indexing pipeline is functional but uses a hardcoded 500-character truncation that doesn't respect model limits. The embedding provider has infrastructure for size validation (SequenceTooLong error, max_sequence_length method) but doesn't enforce limits.

### Key Discoveries:
- `EmbeddingProvider` trait at `crates/embeddings/src/provider.rs:19` returns `Vec<Vec<f32>>` without size validation
- `SequenceTooLong` error defined at `crates/embeddings/src/error.rs:21` but never used
- Arbitrary 500-char truncation at `crates/indexer/src/repository_indexer.rs:96-98`
- Storage requires equal-length arrays at `crates/storage/src/qdrant/client.rs:132`
- Model metadata including context window already fetched from HuggingFace

## Desired End State

After implementation:
- Large code entities exceeding model context windows are automatically detected and skipped
- No arbitrary truncation - entities maintain semantic integrity
- Clear statistics reporting how many entities were skipped due to size
- System continues to function efficiently with mixed small/large entities

### Verification:
- Run `cargo test --workspace` - all tests pass with updated signatures
- Index a repository with large files - see "skipped due to size" in statistics
- Query indexed data - confirm no truncated content present
- Check logs - see which specific entities were skipped

## What We're NOT Doing

- **Not implementing chunking** - Large entities will be skipped, not split
- **Not using token counting** - Will use character count approximation for simplicity
- **Not storing entities without embeddings** - Skipped entities won't be stored at all
- **Not modifying storage layer** - Storage continues to require embeddings for all entities
- **Not changing batch processing logic** - Existing 100-file batch size remains

## Implementation Approach

Use a three-phase approach: First modify the embeddings crate to return `Option` values for oversized inputs, then update the indexer to handle these None values by filtering before storage, finally validate with comprehensive tests.

## Phase 1: Embeddings Crate Modifications

### Overview
Modify the EmbeddingProvider trait to return `Option<Vec<f32>>` for each input text, allowing the provider to signal when text exceeds model limits.

### Changes Required:

#### 1. EmbeddingProvider Trait
**File**: `crates/embeddings/src/provider.rs`
**Changes**: Update trait signature to return Options

At line 19, change:
```rust
async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;
```
To:
```rust
async fn embed(&self, texts: Vec<String>) -> Result<Vec<Option<Vec<f32>>>>;
```

#### 2. EmbedAnythingProvider Implementation
**File**: `crates/embeddings/src/embed_anything_provider.rs`
**Changes**: Add size validation before embedding generation

At lines 135-218, modify the `embed` method to:
1. Check each text's length against `self.max_context` before line 172
2. For texts exceeding limit, add `None` to results instead of calling `embed_query`
3. For valid texts, wrap existing embedding in `Some()`
4. Return `Vec<Option<Vec<f32>>>` at line 215

#### 3. EmbeddingManager
**File**: `crates/embeddings/src/lib.rs`
**Changes**: Update manager's embed method

At lines 54-56, update signature to match new trait:
```rust
pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Option<Vec<f32>>>>
```

#### 4. Mock Providers
**Files**:
- `crates/embeddings/tests/local_embeddings_tests.rs` (line 46)
- `crates/indexer/tests/integration_test.rs` (line 17)
- `crates/indexer/src/repository_indexer.rs` (line 470)

**Changes**: Update all mock implementations to return `Vec<Option<Vec<f32>>>`

### Success Criteria:

#### Automated Verification:
- [ ] Embeddings crate compiles: `cargo build -p codesearch-embeddings`
- [ ] Embeddings tests pass: `cargo test -p codesearch-embeddings`
- [ ] Type checking passes: `cargo check --workspace`

#### Manual Verification:
- [ ] Test with text exceeding model limits returns `None`
- [ ] Test with valid text returns `Some(embedding)`
- [ ] Batch with mixed valid/invalid texts handled correctly

---

## Phase 2: Indexer Crate Updates

### Overview
Update the indexer to handle `Option` embeddings, filtering out entities without embeddings before storage.

### Changes Required:

#### 1. Remove Truncation
**File**: `crates/indexer/src/repository_indexer.rs`
**Changes**: Remove arbitrary 500-char limit

At lines 96-98, remove the truncation logic:
```rust
// Delete the 500-char truncation, use content directly
let truncated = content.clone();
```

#### 2. Handle Option Embeddings
**File**: `crates/indexer/src/repository_indexer.rs`
**Changes**: Process Option embeddings in batch processing

At lines 247-257 in `process_batch`:
1. After calling `embed()` at line 250-254, receive `Vec<Option<Vec<f32>>>`
2. Zip entities with Option embeddings
3. Filter to keep only `(entity, Some(embedding))` pairs
4. Collect separate vectors of entities and unwrapped embeddings
5. Pass filtered vectors to `bulk_load_entities` at line 257

#### 3. Update Statistics
**File**: `crates/indexer/src/repository_indexer.rs`
**Changes**: Track skipped entities

Add to `IndexStats` struct (around line 30):
- `entities_skipped_size: usize` field
- Method to increment skipped count
- Include in statistics reporting at lines 187-191

### Success Criteria:

#### Automated Verification:
- [ ] Indexer compiles: `cargo build -p codesearch-indexer`
- [ ] Indexer tests pass: `cargo test -p codesearch-indexer`
- [ ] Integration tests pass: `cargo test --test integration_test`

#### Manual Verification:
- [ ] Large entities are skipped, not truncated
- [ ] Statistics show correct skipped entity count
- [ ] Only entities with embeddings reach storage

---

## Phase 3: Integration Testing

### Overview
Validate the complete pipeline handles mixed small/large entities correctly.

### Changes Required:

#### 1. Add Size Validation Test
**File**: `crates/embeddings/tests/local_embeddings_tests.rs`
**Changes**: New test for size limits

Add test function:
```rust
#[tokio::test]
async fn test_embedding_size_limits() {
    // Create provider with known context limit
    // Test text under limit returns Some
    // Test text over limit returns None
    // Test batch with mixed sizes
}
```

#### 2. Update Integration Tests
**File**: `crates/indexer/tests/integration_test.rs`
**Changes**: Test skipped entity handling

Add assertions to verify:
- Entities with None embeddings don't reach storage
- Statistics correctly count skipped entities
- Pipeline continues after encountering oversized entities

#### 3. End-to-End Validation
**File**: New test file or in existing E2E tests
**Changes**: Complete pipeline test

Create test that:
1. Creates test repository with mixed file sizes
2. Runs full indexing pipeline
3. Verifies storage contains only embeddable entities
4. Checks statistics for skipped counts

### Success Criteria:

#### Automated Verification:
- [ ] All workspace tests pass: `cargo test --workspace`
- [ ] Clippy passes: `cargo clippy --workspace`
- [ ] Format check passes: `cargo fmt --check`

#### Manual Verification:
- [ ] Index real repository with large files
- [ ] Verify "skipped due to size" appears in output
- [ ] Search results don't contain truncated content
- [ ] Performance acceptable with many skipped entities

---

## Testing Strategy

### Unit Tests:
- Embedding provider size validation with exact context window boundary
- Option handling in embedding manager
- Filtering logic in indexer batch processing
- Statistics tracking for skipped entities

### Integration Tests:
- Pipeline with all small entities (no skipping)
- Pipeline with all large entities (all skipped)
- Pipeline with mixed sizes (partial skipping)
- Error recovery when some embeddings fail

### Manual Testing Steps:
1. Run `cargo run -- init` in test repository
2. Add file with 10,000+ character function
3. Run `cargo run -- index`
4. Verify output shows "X entities skipped due to size"
5. Run `cargo run -- search "large_function"`
6. Confirm no results for skipped entity

## Performance Considerations

- Character counting is O(n) but faster than tokenization
- Filtering before storage reduces Qdrant operations
- Skipped entities still consume extraction time but not embedding time
- Consider logging at INFO level only for first few skips, then DEBUG

## Migration Notes

No data migration needed as this only affects new indexing operations. Existing indexed data remains unchanged. Re-indexing will apply new size limits.

## References

- Embeddings trait: `crates/embeddings/src/provider.rs:19`
- Indexer batch processing: `crates/indexer/src/repository_indexer.rs:247-257`