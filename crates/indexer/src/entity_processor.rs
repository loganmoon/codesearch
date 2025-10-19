//! Shared entity processing logic
//!
//! This module provides common functions for entity extraction, embedding generation,
//! and storage that are used by both full repository indexing and incremental file updates.

use crate::common::{path_to_str, ResultExt};
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use codesearch_embeddings::EmbeddingManager;
use codesearch_languages::create_extractor;
use codesearch_storage::{OutboxOperation, PostgresClientTrait, TargetStore};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info};
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
        if let Some(previous) = unique_entities.insert(entity.entity_id.clone(), entity) {
            debug!(
                "Duplicate entity_id detected in batch: {} in file {} (qualified_name: {})",
                previous.entity_id,
                previous.file_path.display(),
                previous.qualified_name
            );
        }
    }
    let entities: Vec<CodeEntity> = unique_entities.into_values().collect();

    if entities.len() < original_count {
        info!(
            "Deduplicated batch: {} entities -> {} unique entities ({} duplicates removed)",
            original_count,
            entities.len(),
            original_count - entities.len()
        );
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
        .get_embeddings_by_content_hash(&content_hashes, model_version)
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
        if let Some((embedding_id, cached_embedding)) = cached_embeddings.get(content_hash) {
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

    // Generate embeddings for cache misses only
    if !cache_miss_texts.is_empty() {
        info!(
            "Generating {} new embeddings via API",
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

            // Extract just the (content_hash, embedding) pairs for storage
            let entries_for_storage: Vec<(String, Vec<f32>)> = cache_entries_to_store
                .iter()
                .map(|((_, content_hash), embedding)| (content_hash.clone(), embedding.clone()))
                .collect();

            let new_embedding_ids = postgres_client
                .store_embeddings(&entries_for_storage, model_version, dimension)
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

    // Filter entities with valid embedding IDs
    let mut entity_embedding_id_pairs: Vec<(CodeEntity, i64)> = Vec::with_capacity(entities.len());
    for ((entity, opt_embedding), opt_embedding_id) in entities
        .into_iter()
        .zip(all_embeddings.into_iter())
        .zip(all_embedding_ids.into_iter())
    {
        if let (Some(embedding), Some(embedding_id)) = (opt_embedding, opt_embedding_id) {
            // Validate embedding is not all zeros
            let is_all_zeros = embedding.iter().all(|&v| v == 0.0);
            if is_all_zeros {
                debug!(
                    "Warning: embedding is all zeros for entity {} in {}",
                    entity.qualified_name,
                    entity.file_path.display()
                );
            }
            entity_embedding_id_pairs.push((entity, embedding_id));
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
        .map(|(entity, _)| entity.entity_id.clone())
        .collect();

    let metadata_map = postgres_client
        .get_entities_metadata_batch(repo_id, &entity_ids)
        .await
        .storage_err("Failed to fetch entity metadata")?;

    // Prepare batch data directly as references (no intermediate cloning)
    let mut batch_refs = Vec::with_capacity(entity_embedding_id_pairs.len());

    for (entity, embedding_id) in &entity_embedding_id_pairs {
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

        postgres_client
            .mark_entities_deleted_with_outbox(repo_id, collection_name, &stale_ids)
            .await
            .storage_err("Failed to mark entities as deleted with outbox")?;
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

    postgres_client
        .mark_entities_deleted_with_outbox(repo_id, collection_name, &entity_ids)
        .await
        .storage_err("Failed to mark entities as deleted with outbox")?;

    info!("Marked {} entities as deleted", count);
    Ok(count)
}
