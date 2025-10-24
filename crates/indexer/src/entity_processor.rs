//! Shared entity processing logic
//!
//! This module provides common functions for entity extraction, embedding generation,
//! and storage that are used by both full repository indexing and incremental file updates.

use crate::common::{path_to_str, ResultExt};
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use codesearch_embeddings::EmbeddingManager;
use codesearch_languages::create_extractor;
use codesearch_storage::{EmbeddingCacheEntry, OutboxOperation, PostgresClientTrait, TargetStore};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use twox_hash::XxHash3_128;
use uuid::Uuid;

const DELIM: &str = " ";

/// Extract embeddable content from a CodeEntity
pub fn extract_embedding_content(entity: &CodeEntity) -> String {
    // Calculate accurate capacity
    let estimated_size = entity.name.len()
        + entity.qualified_name.len()
        + entity.documentation_summary.as_ref().map_or(0, |s| s.len())
        + entity.content.as_ref().map_or(0, |s| s.len())
        + 100; // Extra padding for delimiters and formatting

    let mut content = String::with_capacity(estimated_size);

    // Add entity type and name
    content.push_str(&format!("{} {}", entity.entity_type, entity.name));
    chain_delim(&mut content, &entity.qualified_name);

    // Add documentation summary if available
    if let Some(doc) = &entity.documentation_summary {
        chain_delim(&mut content, doc);
    }

    // Add signature information for functions/methods
    if let Some(sig) = &entity.signature {
        for (name, type_opt) in &sig.parameters {
            content.push_str(DELIM);
            content.push_str(name);
            if let Some(param_type) = type_opt {
                content.push_str(": ");
                content.push_str(param_type);
            }
        }

        if let Some(ret_type) = &sig.return_type {
            chain_delim(&mut content, &format!("-> {ret_type}"));
        }
    }

    // Add the full entity content (most important for semantic search)
    if let Some(entity_content) = &entity.content {
        chain_delim(&mut content, entity_content);
    }

    content
}

fn chain_delim(out_str: &mut String, text: &str) {
    out_str.push_str(DELIM);
    out_str.push_str(text);
}

/// Extract entities from a single file
pub async fn extract_entities_from_file(
    file_path: &Path,
    repo_id: &str,
) -> Result<Vec<CodeEntity>> {
    debug!("Extracting from file: {}", file_path.display());

    // Create extractor for this file
    let extractor = match create_extractor(file_path, repo_id)? {
        Some(ext) => ext,
        None => {
            debug!("No extractor available for file: {}", file_path.display());
            return Ok(Vec::new());
        }
    };

    // Read file
    let content = tokio::fs::read_to_string(file_path)
        .await
        .map_err(|e| Error::parse(file_path.display().to_string(), e.to_string()))?;

    // Extract entities
    let entities = extractor
        .extract(&content, file_path)
        .map_err(|e| Error::parse(file_path.display().to_string(), e.to_string()))?;

    debug!(
        "Extracted {} entities from {}",
        entities.len(),
        file_path.display()
    );

    Ok(entities)
}

/// Calculate token counts for a batch of entities
///
/// Uses the BM25 tokenizer to count tokens in each entity's embedding content.
/// These counts are used for maintaining avgdl statistics.
pub fn calculate_token_counts(entities: &[CodeEntity]) -> Result<Vec<usize>> {
    use codesearch_embeddings::{CodeTokenizer, Tokenizer};

    let tokenizer = CodeTokenizer;
    let mut token_counts = Vec::with_capacity(entities.len());

    for entity in entities {
        let content = extract_embedding_content(entity);
        let tokens = tokenizer.tokenize(&content);
        token_counts.push(tokens.len());
    }

    Ok(token_counts)
}

/// Statistics for entity batch processing
#[derive(Debug, Default)]
pub struct BatchProcessingStats {
    pub entities_added: usize,
    pub entities_updated: usize,
    pub entities_skipped_size: usize,
}

/// Process a batch of entities: generate embeddings, check metadata, and store with outbox
pub async fn process_entity_batch(
    entities: Vec<CodeEntity>,
    repo_id: Uuid,
    collection_name: String,
    git_commit: Option<String>,
    embedding_manager: &Arc<EmbeddingManager>,
    postgres_client: &(dyn PostgresClientTrait + Send + Sync),
    max_batch_size: usize,
) -> Result<(BatchProcessingStats, HashMap<String, Vec<String>>)> {
    let mut stats = BatchProcessingStats::default();
    let mut entities_by_file: HashMap<String, Vec<String>> = HashMap::new();

    if entities.is_empty() {
        return Ok((stats, entities_by_file));
    }

    // Deduplicate entities by entity_id (keep last occurrence to match PostgreSQL ON CONFLICT behavior)
    let original_count = entities.len();
    let mut unique_entities = HashMap::new();
    for entity in entities {
        if let Some(previous) = unique_entities.insert(entity.entity_id.clone(), entity.clone()) {
            warn!(
                "Duplicate entity_id in batch (will keep last): {}\n\
                 - First: {} (line {}-{}, type: {:?})\n\
                 - Second: {} (line {}-{}, type: {:?})\n\
                 File: {}",
                previous.entity_id,
                previous.qualified_name,
                previous.location.start_line,
                previous.location.end_line,
                previous.entity_type,
                entity.qualified_name,
                entity.location.start_line,
                entity.location.end_line,
                entity.entity_type,
                entity.file_path.display()
            );
        }
    }
    let mut entities: Vec<CodeEntity> = unique_entities.into_values().collect();

    if entities.len() < original_count {
        info!(
            "Deduplicated batch: {} entities -> {} unique entities ({} duplicates removed)",
            original_count,
            entities.len(),
            original_count - entities.len()
        );
    }

    // Sort by entity_id for deterministic ordering
    entities.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));

    // Final safety check: validate no duplicates remain after deduplication
    // This catches bugs in entity ID generation
    let mut seen_ids: HashMap<&str, &CodeEntity> = HashMap::new();
    for entity in &entities {
        if let Some(existing) = seen_ids.insert(&entity.entity_id, entity) {
            error!(
                "CRITICAL BUG: Duplicate entity_id {} detected after deduplication!\n\
                 Entity 1: {} in {} (type: {:?}, line: {})\n\
                 Entity 2: {} in {} (type: {:?}, line: {})\n\
                 This is a bug in entity ID generation - entity IDs must be unique.",
                entity.entity_id,
                existing.qualified_name,
                existing.file_path.display(),
                existing.entity_type,
                existing.location.start_line,
                entity.qualified_name,
                entity.file_path.display(),
                entity.entity_type,
                entity.location.start_line
            );
            return Err(Error::entity_extraction(format!(
                "Duplicate entity_id {} found after deduplication. This is a critical bug in entity extraction.",
                entity.entity_id
            )));
        }
    }

    // collection_name is now passed as parameter

    // Chunk entities if batch exceeds max size
    let chunks: Vec<Vec<CodeEntity>> = entities
        .chunks(max_batch_size)
        .map(|chunk| chunk.to_vec())
        .collect();

    let num_chunks = chunks.len();
    let total_entities = entities.len();

    if num_chunks > 1 {
        info!(
            "Processing {} entities in {} chunks of max {} entities each",
            total_entities, num_chunks, max_batch_size
        );
    }

    // Process each chunk separately
    for (chunk_idx, chunk) in chunks.into_iter().enumerate() {
        if num_chunks > 1 {
            info!(
                "Processing chunk {}/{} with {} entities",
                chunk_idx + 1,
                num_chunks,
                chunk.len()
            );
        }

        let (chunk_stats, chunk_entities_by_file) = process_entity_chunk(
            chunk,
            repo_id,
            &collection_name,
            git_commit.clone(),
            embedding_manager,
            postgres_client,
        )
        .await?;

        // Aggregate stats
        stats.entities_added += chunk_stats.entities_added;
        stats.entities_updated += chunk_stats.entities_updated;
        stats.entities_skipped_size += chunk_stats.entities_skipped_size;

        // Merge entities_by_file
        for (file_path, entity_ids) in chunk_entities_by_file {
            entities_by_file
                .entry(file_path)
                .or_default()
                .extend(entity_ids);
        }
    }

    Ok((stats, entities_by_file))
}

/// Process a single chunk of entities (internal helper)
async fn process_entity_chunk(
    entities: Vec<CodeEntity>,
    repo_id: Uuid,
    collection_name: &str,
    git_commit: Option<String>,
    embedding_manager: &Arc<EmbeddingManager>,
    postgres_client: &(dyn PostgresClientTrait + Send + Sync),
) -> Result<(BatchProcessingStats, HashMap<String, Vec<String>>)> {
    let mut stats = BatchProcessingStats::default();
    let mut entities_by_file: HashMap<String, Vec<String>> = HashMap::new();

    info!("Generating embeddings for {} entities", entities.len());

    // Calculate token counts for all entities (will be used for storage)
    // Note: BM25 statistics are updated by the outbox processor within its transaction
    let token_counts = calculate_token_counts(&entities)?;

    // Get current avgdl for sparse embedding generation
    let bm25_stats = postgres_client
        .get_bm25_statistics(repo_id)
        .await
        .storage_err("Failed to get BM25 statistics")?;

    // Create sparse embedding manager with current avgdl
    let sparse_manager = codesearch_embeddings::create_sparse_manager(bm25_stats.avgdl)
        .storage_err("Failed to create sparse embedding manager")?;

    // Phase 1: Compute content hashes for all entities
    let entity_contents_and_hashes: Vec<(String, String)> = entities
        .iter()
        .map(|entity| {
            let content = extract_embedding_content(entity);
            let content_hash = format!("{:032x}", XxHash3_128::oneshot(content.as_bytes()));
            (content, content_hash)
        })
        .collect();

    // Phase 2: Batch lookup cached embeddings
    let content_hashes: Vec<String> = entity_contents_and_hashes
        .iter()
        .map(|(_, hash)| hash.clone())
        .collect();

    let model_version = embedding_manager.model_version();
    let cached_embeddings = postgres_client
        .get_embeddings_by_content_hash(repo_id, &content_hashes, model_version)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Cache lookup failed, will generate all embeddings: {}", e);
            HashMap::new()
        });

    // Phase 3: Separate cache hits from misses
    let mut cache_hits = Vec::new();
    let mut cache_misses = Vec::new();
    let mut cache_miss_texts = Vec::new();

    for (idx, (content, content_hash)) in entity_contents_and_hashes.iter().enumerate() {
        if let Some((embedding_id, cached_embedding, _sparse)) = cached_embeddings.get(content_hash)
        {
            cache_hits.push((idx, *embedding_id, cached_embedding.clone()));
        } else {
            cache_misses.push((idx, content_hash.clone()));
            cache_miss_texts.push(content.clone());
        }
    }

    info!(
        "Embedding cache: {} hits, {} misses ({:.1}% hit rate)",
        cache_hits.len(),
        cache_misses.len(),
        if !entities.is_empty() {
            (cache_hits.len() as f64 / entities.len() as f64) * 100.0
        } else {
            0.0
        }
    );

    // Phase 4: Generate embeddings only for cache misses
    let mut all_embeddings = vec![None; entities.len()];
    let mut all_embedding_ids = vec![None; entities.len()];

    // Fill in cache hits
    for (idx, embedding_id, embedding) in cache_hits {
        all_embeddings[idx] = Some(embedding);
        all_embedding_ids[idx] = Some(embedding_id);
    }

    // Generate sparse embeddings for ALL entities (locally generated, will be stored with cache entries)
    info!("Generating {} sparse embeddings locally", entities.len());
    let all_entity_contents: Vec<&str> = entity_contents_and_hashes
        .iter()
        .map(|(content, _)| content.as_str())
        .collect();

    let all_sparse_embeddings = sparse_manager
        .embed_sparse(all_entity_contents)
        .await
        .storage_err("Failed to generate sparse embeddings")?;

    // Generate embeddings for cache misses only
    if !cache_miss_texts.is_empty() {
        info!(
            "Generating {} new dense embeddings via API",
            cache_miss_texts.len()
        );

        let new_embeddings = embedding_manager
            .embed(cache_miss_texts)
            .await
            .storage_err("Failed to generate embeddings")?;

        // Fill in newly generated embeddings
        let mut new_embeddings_iter = new_embeddings.into_iter();
        for (idx, _content_hash) in &cache_misses {
            if let Some(Some(embedding)) = new_embeddings_iter.next() {
                all_embeddings[*idx] = Some(embedding);
            }
        }

        // Phase 5: Store newly generated embeddings in cache and capture their IDs
        let cache_entries_to_store: Vec<((usize, String), Vec<f32>)> = cache_misses
            .iter()
            .filter_map(|(idx, content_hash)| {
                all_embeddings[*idx]
                    .clone()
                    .map(|emb| ((*idx, content_hash.clone()), emb))
            })
            .collect();

        if !cache_entries_to_store.is_empty() {
            let dimension = cache_entries_to_store[0].1.len();

            // Extract (content_hash, embedding, sparse_embedding) tuples for storage
            let entries_for_storage: Vec<EmbeddingCacheEntry> = cache_entries_to_store
                .iter()
                .map(|((idx, content_hash), embedding)| {
                    let sparse_embedding = all_sparse_embeddings[*idx].clone();
                    (content_hash.clone(), embedding.clone(), sparse_embedding)
                })
                .collect();

            let new_embedding_ids = postgres_client
                .store_embeddings(repo_id, &entries_for_storage, model_version, dimension)
                .await
                .storage_err("Failed to store embeddings in cache")?;

            // Map returned IDs back to entity indices
            for (entry, embedding_id) in cache_entries_to_store
                .iter()
                .zip(new_embedding_ids.into_iter())
            {
                let (idx, _content_hash) = &entry.0;
                all_embedding_ids[*idx] = Some(embedding_id);
            }

            info!(
                "Stored {} new embeddings in cache",
                cache_entries_to_store.len()
            );
        }
    }

    // Filter entities with valid embedding IDs and sparse embeddings
    type EntityEmbeddingPair = (CodeEntity, i64, Vec<(u32, f32)>);
    let mut entity_embedding_id_pairs: Vec<EntityEmbeddingPair> =
        Vec::with_capacity(entities.len());
    for (((entity, opt_embedding), opt_embedding_id), opt_sparse_embedding) in entities
        .into_iter()
        .zip(all_embeddings.into_iter())
        .zip(all_embedding_ids.into_iter())
        .zip(all_sparse_embeddings.into_iter())
    {
        if let (Some(embedding), Some(embedding_id), Some(sparse_embedding)) =
            (opt_embedding, opt_embedding_id, opt_sparse_embedding)
        {
            // Validate embedding is not all zeros
            let is_all_zeros = embedding.iter().all(|&v| v == 0.0);
            if is_all_zeros {
                debug!(
                    "Warning: embedding is all zeros for entity {} in {}",
                    entity.qualified_name,
                    entity.file_path.display()
                );
            }
            entity_embedding_id_pairs.push((entity, embedding_id, sparse_embedding));
        } else {
            stats.entities_skipped_size += 1;
            debug!(
                "Skipped entity due to size or missing embedding ID: {} in {}",
                entity.qualified_name,
                entity.file_path.display()
            );
        }
    }

    if entity_embedding_id_pairs.is_empty() {
        return Ok((stats, entities_by_file));
    }

    info!(
        "Storing {} entities with embeddings",
        entity_embedding_id_pairs.len()
    );

    // Batch fetch all entity metadata in a single query
    let entity_ids: Vec<String> = entity_embedding_id_pairs
        .iter()
        .map(|(entity, _, _)| entity.entity_id.clone())
        .collect();

    let metadata_map = postgres_client
        .get_entities_metadata_batch(repo_id, &entity_ids)
        .await
        .storage_err("Failed to fetch entity metadata")?;

    // Prepare batch data directly as references (no intermediate cloning)
    let mut batch_refs = Vec::with_capacity(entity_embedding_id_pairs.len());

    for (idx, (entity, embedding_id, _sparse_embedding)) in
        entity_embedding_id_pairs.iter().enumerate()
    {
        let existing_metadata = metadata_map.get(&entity.entity_id);

        let (point_id, operation) = if let Some((existing_point_id, deleted_at)) = existing_metadata
        {
            if deleted_at.is_some() {
                stats.entities_added += 1;
                (Uuid::new_v4(), OutboxOperation::Insert)
            } else {
                stats.entities_updated += 1;
                (*existing_point_id, OutboxOperation::Update)
            }
        } else {
            stats.entities_added += 1;
            (Uuid::new_v4(), OutboxOperation::Insert)
        };

        // Track for file snapshot
        let file_path_str = path_to_str(&entity.file_path)?.to_string();
        entities_by_file
            .entry(file_path_str)
            .or_default()
            .push(entity.entity_id.clone());

        batch_refs.push((
            entity,
            *embedding_id,
            operation,
            point_id,
            TargetStore::Qdrant,
            git_commit.clone(),
            token_counts[idx],
        ));
    }

    // Use cached collection_name passed from parent function
    postgres_client
        .store_entities_with_outbox_batch(repo_id, collection_name, &batch_refs)
        .await
        .storage_err("Failed to store entities")?;

    debug!(
        "Successfully stored {} entities",
        entity_embedding_id_pairs.len()
    );

    Ok((stats, entities_by_file))
}

/// Update file snapshot and mark stale entities as deleted
pub async fn update_file_snapshot_and_mark_stale(
    repo_id: Uuid,
    collection_name: &str,
    file_path: &str,
    new_entity_ids: Vec<String>,
    git_commit: Option<String>,
    postgres_client: &(dyn PostgresClientTrait + Send + Sync),
) -> Result<usize> {
    let old_entity_ids = postgres_client
        .get_file_snapshot(repo_id, file_path)
        .await
        .storage_err("Failed to get file snapshot")?
        .unwrap_or_default();

    // Use HashSet for O(1) lookups instead of O(n) Vec::contains
    let new_entity_set: HashSet<&String> = new_entity_ids.iter().collect();
    let stale_ids: Vec<String> = old_entity_ids
        .iter()
        .filter(|old_id| !new_entity_set.contains(old_id))
        .cloned()
        .collect();

    let stale_count = stale_ids.len();

    if !stale_ids.is_empty() {
        info!("Found {} stale entities in {}", stale_ids.len(), file_path);

        // Fetch token counts for stale entities before deletion
        let entity_refs: Vec<(Uuid, String)> = stale_ids
            .iter()
            .map(|entity_id| (repo_id, entity_id.clone()))
            .collect();

        let token_counts = postgres_client
            .get_entity_token_counts(&entity_refs)
            .await
            .storage_err("Failed to get entity token counts")?;

        // Mark entities as deleted
        postgres_client
            .mark_entities_deleted_with_outbox(repo_id, collection_name, &stale_ids)
            .await
            .storage_err("Failed to mark entities as deleted with outbox")?;

        // Update avgdl statistics after deletion
        if !token_counts.is_empty() {
            postgres_client
                .update_bm25_statistics_after_deletion(repo_id, &token_counts)
                .await
                .storage_err("Failed to update BM25 statistics after deletion")?;
        }
    }

    postgres_client
        .update_file_snapshot(repo_id, file_path, new_entity_ids, git_commit)
        .await
        .storage_err("Failed to update file snapshot")?;

    Ok(stale_count)
}

/// Mark all entities in a file as deleted (for file deletion)
pub async fn mark_file_entities_deleted(
    repo_id: Uuid,
    collection_name: &str,
    file_path: &str,
    postgres_client: &(dyn PostgresClientTrait + Send + Sync),
) -> Result<usize> {
    info!("Handling deletion of file: {}", file_path);

    let entity_ids = postgres_client
        .get_file_snapshot(repo_id, file_path)
        .await
        .storage_err("Failed to get file snapshot")?
        .unwrap_or_default();

    if entity_ids.is_empty() {
        return Ok(0);
    }

    let count = entity_ids.len();

    // Fetch token counts for entities before deletion
    let entity_refs: Vec<(Uuid, String)> = entity_ids
        .iter()
        .map(|entity_id| (repo_id, entity_id.clone()))
        .collect();

    let token_counts = postgres_client
        .get_entity_token_counts(&entity_refs)
        .await
        .storage_err("Failed to get entity token counts")?;

    // Mark entities as deleted
    postgres_client
        .mark_entities_deleted_with_outbox(repo_id, collection_name, &entity_ids)
        .await
        .storage_err("Failed to mark entities as deleted with outbox")?;

    // Update avgdl statistics after deletion
    if !token_counts.is_empty() {
        postgres_client
            .update_bm25_statistics_after_deletion(repo_id, &token_counts)
            .await
            .storage_err("Failed to update BM25 statistics after deletion")?;
    }

    info!("Marked {} entities as deleted and updated avgdl", count);
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesearch_core::{
        CodeEntity, EntityType, FunctionSignature, Language, SourceLocation, Visibility,
    };
    use std::path::PathBuf;
    use uuid::Uuid;

    fn create_sample_function_entity() -> CodeEntity {
        CodeEntity {
            entity_id: Uuid::new_v4().to_string(),
            repository_id: Uuid::new_v4().to_string(),
            entity_type: EntityType::Function,
            name: "handle_request".to_string(),
            qualified_name: "my_crate::handlers::handle_request".to_string(),
            parent_scope: None,
            dependencies: vec![],
            file_path: PathBuf::from("src/handlers.rs"),
            location: SourceLocation {
                start_line: 10,
                end_line: 25,
                start_column: 1,
                end_column: 1,
            },
            visibility: Visibility::Public,
            language: Language::Rust,
            signature: Some(FunctionSignature {
                parameters: vec![
                    ("req".to_string(), Some("HttpRequest".to_string())),
                    ("ctx".to_string(), Some("Context".to_string())),
                ],
                return_type: Some("Result<Response>".to_string()),
                is_async: false,
                generics: vec![],
            }),
            documentation_summary: Some(
                "Handles incoming HTTP requests and returns responses".to_string(),
            ),
            content: Some("fn handle_request(req: HttpRequest, ctx: Context) -> Result<Response> {\n    // Implementation\n    Ok(Response::new())\n}".to_string()),
            metadata: Default::default(),
        }
    }

    #[test]
    fn test_extracted_content_quality() {
        let entity = create_sample_function_entity();
        let content = extract_embedding_content(&entity);

        // Verify all key components are present
        assert!(
            content.contains(&entity.name),
            "Content should contain entity name"
        );
        assert!(
            content.contains(&entity.qualified_name),
            "Content should contain qualified name"
        );
        if let Some(doc) = entity.documentation_summary.as_ref() {
            assert!(
                content.contains(doc),
                "Content should contain documentation"
            );
        }

        // Verify signature components
        assert!(
            content.contains("req"),
            "Content should contain parameter names"
        );
        assert!(
            content.contains("HttpRequest"),
            "Content should contain parameter types"
        );
        assert!(
            content.contains("-> Result<Response>"),
            "Content should contain return type"
        );

        // Verify entity content
        assert!(
            content.contains("fn handle_request"),
            "Content should contain the actual code"
        );

        // Verify reasonable length (not empty, not excessively long)
        assert!(content.len() > 50, "Content should be substantial");
        assert!(
            content.len() < 100_000,
            "Content should not be excessively long"
        );
    }

    #[test]
    fn test_extracted_content_no_instruction_prefix() {
        let entity = create_sample_function_entity();
        let content = extract_embedding_content(&entity);

        // Verify documents DO NOT use BGE instruction prefix
        assert!(
            !content.starts_with("<instruct>"),
            "Documents should not have <instruct> prefix"
        );
        assert!(
            !content.contains("<query>"),
            "Documents should not have <query> prefix"
        );

        // Should start with entity type
        assert!(
            content.starts_with("Function "),
            "Content should start with entity type"
        );
    }

    #[test]
    fn test_extracted_content_minimal_entity() {
        let minimal_entity = CodeEntity {
            entity_id: Uuid::new_v4().to_string(),
            repository_id: Uuid::new_v4().to_string(),
            entity_type: EntityType::Struct,
            name: "Point".to_string(),
            qualified_name: "geometry::Point".to_string(),
            parent_scope: None,
            dependencies: vec![],
            file_path: PathBuf::from("src/geometry.rs"),
            location: SourceLocation {
                start_line: 5,
                end_line: 8,
                start_column: 1,
                end_column: 1,
            },
            visibility: Visibility::Public,
            language: Language::Rust,
            signature: None,
            documentation_summary: None,
            content: None,
            metadata: Default::default(),
        };

        let content = extract_embedding_content(&minimal_entity);

        // Should still have basic structure
        assert!(content.contains("Point"), "Content should contain name");
        assert!(
            content.contains("geometry::Point"),
            "Content should contain qualified name"
        );
        assert!(
            !content.is_empty(),
            "Content should not be empty even for minimal entity"
        );
        assert!(
            content.len() > 10,
            "Content should have reasonable minimum length"
        );
    }

    #[test]
    fn test_extracted_content_with_special_characters() {
        let entity = CodeEntity {
            entity_id: Uuid::new_v4().to_string(),
            repository_id: Uuid::new_v4().to_string(),
            entity_type: EntityType::Function,
            name: "test<T>".to_string(),
            qualified_name: "crate::utils::test<T>".to_string(),
            parent_scope: None,
            dependencies: vec![],
            file_path: PathBuf::from("src/utils.rs"),
            location: SourceLocation {
                start_line: 1,
                end_line: 5,
                start_column: 1,
                end_column: 1,
            },
            visibility: Visibility::Public,
            language: Language::Rust,
            signature: None,
            documentation_summary: Some("Test with \"quotes\" and\nnewlines".to_string()),
            content: Some("fn test<T>() -> Result<T> { /* ... */ }".to_string()),
            metadata: Default::default(),
        };

        let content = extract_embedding_content(&entity);

        // Special characters should be preserved
        assert!(
            content.contains("<T>"),
            "Content should preserve generic type parameters"
        );
        assert!(
            content.contains("\"quotes\""),
            "Content should preserve quotes"
        );
        assert!(
            content.contains("newlines"),
            "Content should preserve text across newlines"
        );
    }

    #[test]
    fn test_extracted_content_consistency() {
        let entity = create_sample_function_entity();

        // Extract multiple times to verify consistency
        let content1 = extract_embedding_content(&entity);
        let content2 = extract_embedding_content(&entity);

        assert_eq!(
            content1, content2,
            "Content extraction should be deterministic"
        );
    }
}
