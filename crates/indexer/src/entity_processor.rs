//! Shared entity processing logic
//!
//! This module provides common functions for entity extraction, embedding generation,
//! and storage that are used by both full repository indexing and incremental file updates.

use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use codesearch_embeddings::EmbeddingManager;
use codesearch_languages::create_extractor;
use codesearch_storage::postgres::{OutboxOperation, PostgresClientTrait, TargetStore};
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info};
use uuid::Uuid;

const DELIM: &str = " ";

/// Extract embeddable content from a CodeEntity
pub fn extract_embedding_content(entity: &CodeEntity) -> String {
    let mut content = String::with_capacity(500);

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
    let extractor = match create_extractor(file_path, repo_id) {
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
) -> Result<(BatchProcessingStats, HashMap<String, Vec<String>>)> {
    let mut stats = BatchProcessingStats::default();
    let mut entities_by_file: HashMap<String, Vec<String>> = HashMap::new();

    if entities.is_empty() {
        return Ok((stats, entities_by_file));
    }

    info!("Generating embeddings for {} entities", entities.len());

    // Generate embeddings
    let embedding_texts: Vec<String> = entities.iter().map(extract_embedding_content).collect();

    let option_embeddings = embedding_manager
        .embed(embedding_texts)
        .await
        .map_err(|e| Error::Storage(format!("Failed to generate embeddings: {e}")))?;

    // Filter entities with valid embeddings
    let mut entity_embedding_pairs: Vec<(CodeEntity, Vec<f32>)> = Vec::new();
    for (entity, opt_embedding) in entities.into_iter().zip(option_embeddings.into_iter()) {
        if let Some(embedding) = opt_embedding {
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

    // Prepare batch data
    let mut batch_data = Vec::new();

    for (entity, embedding) in &entity_embedding_pairs {
        let existing_metadata = postgres_client
            .get_entity_metadata(repo_id, &entity.entity_id)
            .await
            .map_err(|e| Error::Storage(format!("Failed to check entity metadata: {e}")))?;

        let (point_id, operation) = if let Some((existing_point_id, deleted_at)) = existing_metadata
        {
            if deleted_at.is_some() {
                stats.entities_added += 1;
                (Uuid::new_v4(), OutboxOperation::Insert)
            } else {
                stats.entities_updated += 1;
                (existing_point_id, OutboxOperation::Update)
            }
        } else {
            stats.entities_added += 1;
            (Uuid::new_v4(), OutboxOperation::Insert)
        };

        // Track for file snapshot
        let file_path_str = entity
            .file_path
            .to_str()
            .ok_or_else(|| Error::Storage("Invalid file path".to_string()))?
            .to_string();
        entities_by_file
            .entry(file_path_str.clone())
            .or_default()
            .push(entity.entity_id.clone());

        batch_data.push((
            entity.clone(),
            embedding.clone(),
            operation,
            point_id,
            TargetStore::Qdrant,
            git_commit.clone(),
        ));
    }

    // Store entities with outbox
    let batch_refs: Vec<_> = batch_data
        .iter()
        .map(|(entity, embedding, op, point_id, target, git_commit)| {
            (
                entity,
                embedding.as_slice(),
                *op,
                *point_id,
                *target,
                git_commit.clone(),
            )
        })
        .collect();

    postgres_client
        .store_entities_with_outbox_batch(repo_id, &batch_refs)
        .await
        .map_err(|e| Error::Storage(format!("Failed to store entities: {e}")))?;

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
        .map_err(|e| Error::Storage(format!("Failed to get file snapshot: {e}")))?
        .unwrap_or_default();

    let stale_ids: Vec<String> = old_entity_ids
        .iter()
        .filter(|old_id| !new_entity_ids.contains(old_id))
        .cloned()
        .collect();

    let stale_count = stale_ids.len();

    if !stale_ids.is_empty() {
        info!("Found {} stale entities in {}", stale_ids.len(), file_path);

        postgres_client
            .mark_entities_deleted(repo_id, &stale_ids)
            .await
            .map_err(|e| Error::Storage(format!("Failed to mark entities as deleted: {e}")))?;

        for entity_id in &stale_ids {
            let payload = json!({
                "entity_ids": [entity_id],
                "reason": "file_change"
            });

            postgres_client
                .write_outbox_entry(
                    repo_id,
                    entity_id,
                    OutboxOperation::Delete,
                    TargetStore::Qdrant,
                    payload,
                )
                .await
                .map_err(|e| Error::Storage(format!("Failed to write outbox entry: {e}")))?;
        }
    }

    postgres_client
        .update_file_snapshot(repo_id, file_path, new_entity_ids, git_commit)
        .await
        .map_err(|e| Error::Storage(format!("Failed to update file snapshot: {e}")))?;

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
        .map_err(|e| Error::Storage(format!("Failed to get file snapshot: {e}")))?
        .unwrap_or_default();

    if entity_ids.is_empty() {
        return Ok(0);
    }

    let count = entity_ids.len();

    postgres_client
        .mark_entities_deleted(repo_id, &entity_ids)
        .await
        .map_err(|e| Error::Storage(format!("Failed to mark entities as deleted: {e}")))?;

    for entity_id in &entity_ids {
        let payload = json!({
            "entity_ids": [entity_id],
            "reason": "file_deleted"
        });

        postgres_client
            .write_outbox_entry(
                repo_id,
                entity_id,
                OutboxOperation::Delete,
                TargetStore::Qdrant,
                payload,
            )
            .await
            .map_err(|e| Error::Storage(format!("Failed to write outbox entry: {e}")))?;
    }

    info!("Marked {} entities as deleted", count);
    Ok(count)
}
