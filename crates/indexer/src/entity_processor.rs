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
    git_commit: Option<String>,
    embedding_manager: &Arc<EmbeddingManager>,
    postgres_client: &(dyn PostgresClientTrait + Send + Sync),
) -> Result<(BatchProcessingStats, HashMap<String, Vec<String>>)> {
    let mut stats = BatchProcessingStats::default();
    let mut entities_by_file: HashMap<String, Vec<String>> = HashMap::new();

    info!("Generating embeddings for {} entities", entities.len());

    // Generate embeddings
    let embedding_texts: Vec<String> = entities.iter().map(extract_embedding_content).collect();

    let option_embeddings = embedding_manager
        .embed(embedding_texts)
        .await
        .storage_err("Failed to generate embeddings")?;

    // Filter entities with valid embeddings
    let mut entity_embedding_pairs: Vec<(CodeEntity, Vec<f32>)> =
        Vec::with_capacity(entities.len());
    for (entity, opt_embedding) in entities.into_iter().zip(option_embeddings.into_iter()) {
        if let Some(embedding) = opt_embedding {
            // Validate embedding is not all zeros
            let is_all_zeros = embedding.iter().all(|&v| v == 0.0);
            if is_all_zeros {
                debug!(
                    "Warning: embedding is all zeros for entity {} in {}",
                    entity.qualified_name,
                    entity.file_path.display()
                );
            }
            entity_embedding_pairs.push((entity, embedding));
        } else {
            stats.entities_skipped_size += 1;
            debug!(
                "Skipped entity due to size: {} in {}",
                entity.qualified_name,
                entity.file_path.display()
            );
        }
    }

    if entity_embedding_pairs.is_empty() {
        return Ok((stats, entities_by_file));
    }

    info!(
        "Storing {} entities with embeddings",
        entity_embedding_pairs.len()
    );

    // Batch fetch all entity metadata in a single query
    let entity_ids: Vec<String> = entity_embedding_pairs
        .iter()
        .map(|(entity, _)| entity.entity_id.clone())
        .collect();

    let metadata_map = postgres_client
        .get_entities_metadata_batch(repo_id, &entity_ids)
        .await
        .storage_err("Failed to fetch entity metadata")?;

    // Prepare batch data directly as references (no intermediate cloning)
    let mut batch_refs = Vec::with_capacity(entity_embedding_pairs.len());

    for (entity, embedding) in &entity_embedding_pairs {
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
            embedding.as_slice(),
            operation,
            point_id,
            TargetStore::Qdrant,
            git_commit.clone(),
        ));
    }

    postgres_client
        .store_entities_with_outbox_batch(repo_id, &batch_refs)
        .await
        .storage_err("Failed to store entities")?;

    debug!(
        "Successfully stored {} entities",
        entity_embedding_pairs.len()
    );

    Ok((stats, entities_by_file))
}

/// Update file snapshot and mark stale entities as deleted
pub async fn update_file_snapshot_and_mark_stale(
    repo_id: Uuid,
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
            .mark_entities_deleted_with_outbox(repo_id, &stale_ids)
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
        .mark_entities_deleted_with_outbox(repo_id, &entity_ids)
        .await
        .storage_err("Failed to mark entities as deleted with outbox")?;

    info!("Marked {} entities as deleted", count);
    Ok(count)
}
