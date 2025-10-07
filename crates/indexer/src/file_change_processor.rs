//! File change processing for real-time indexing
//!
//! Processes file change events from the watcher in batches for optimal throughput.

use crate::Result;
use codesearch_core::{error::Error, CodeEntity};
use codesearch_embeddings::EmbeddingManager;
use codesearch_languages::create_extractor;
use codesearch_storage::postgres::{OutboxOperation, PostgresClient, TargetStore};
use codesearch_watcher::FileChange;
use serde_json::json;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Statistics for file change processing
#[derive(Debug, Clone, Default)]
pub struct ProcessingStats {
    pub files_processed: usize,
    pub files_failed: usize,
    pub entities_added: usize,
    pub entities_updated: usize,
    pub entities_deleted: usize,
}

/// Process a batch of file changes (always batched, even if batch size is 1)
pub async fn process_file_changes(
    changes: Vec<FileChange>,
    repo_id: Uuid,
    repo_root: &Path,
    embedding_manager: &Arc<EmbeddingManager>,
    postgres_client: &Arc<PostgresClient>,
) -> Result<ProcessingStats> {
    if changes.is_empty() {
        return Ok(ProcessingStats::default());
    }

    info!("Processing batch of {} file changes", changes.len());
    let mut stats = ProcessingStats::default();

    // Separate changes by type
    let mut files_to_index = Vec::new();
    let mut files_to_delete = Vec::new();
    let mut renames = Vec::new();

    for change in changes {
        match change {
            FileChange::Created(path, _) | FileChange::Modified(path, _) => {
                files_to_index.push(path);
            }
            FileChange::Deleted(path) => {
                files_to_delete.push(path);
            }
            FileChange::Renamed { from, to } => {
                renames.push((from, to));
            }
            FileChange::PermissionsChanged(_) => {
                // Ignore permission changes
            }
        }
    }

    // Handle renames (delete old, index new)
    for (from, to) in renames {
        if let Err(e) = handle_file_deletion(repo_id, &from, postgres_client).await {
            warn!(
                "Failed to handle rename deletion for {}: {}",
                from.display(),
                e
            );
            stats.files_failed += 1;
        } else {
            stats.entities_deleted += 1;
        }
        files_to_index.push(to);
    }

    // Process deletions
    for path in files_to_delete {
        match handle_file_deletion(repo_id, &path, postgres_client).await {
            Ok(count) => {
                stats.files_processed += 1;
                stats.entities_deleted += count;
            }
            Err(e) => {
                warn!("Failed to handle deletion for {}: {}", path.display(), e);
                stats.files_failed += 1;
            }
        }
    }

    // Process files to index (in batch)
    if !files_to_index.is_empty() {
        match process_file_batch(
            &files_to_index,
            repo_id,
            repo_root,
            embedding_manager,
            postgres_client,
        )
        .await
        {
            Ok(batch_stats) => {
                stats.files_processed += batch_stats.files_processed;
                stats.files_failed += batch_stats.files_failed;
                stats.entities_added += batch_stats.entities_added;
                stats.entities_updated += batch_stats.entities_updated;
            }
            Err(e) => {
                warn!("Batch processing failed: {}", e);
                stats.files_failed += files_to_index.len();
            }
        }
    }

    info!(
        "Batch complete: {} processed, {} failed, {} entities added/updated, {} deleted",
        stats.files_processed,
        stats.files_failed,
        stats.entities_added + stats.entities_updated,
        stats.entities_deleted
    );

    Ok(stats)
}

/// Process a batch of files for indexing
async fn process_file_batch(
    file_paths: &[PathBuf],
    repo_id: Uuid,
    repo_root: &Path,
    embedding_manager: &Arc<EmbeddingManager>,
    postgres_client: &Arc<PostgresClient>,
) -> Result<ProcessingStats> {
    let mut stats = ProcessingStats::default();
    let mut batch_entities = Vec::new();
    let mut entities_by_file: HashMap<String, Vec<String>> = HashMap::new();

    // Extract entities from all files
    for file_path in file_paths {
        // Make path absolute relative to repo root
        let absolute_path = if file_path.is_absolute() {
            file_path.clone()
        } else {
            repo_root.join(file_path)
        };

        match extract_from_file(&absolute_path, &repo_id.to_string()).await {
            Ok(entities) => {
                debug!(
                    "Extracted {} entities from {}",
                    entities.len(),
                    file_path.display()
                );

                // Track entities by file
                let file_path_str = file_path
                    .to_str()
                    .ok_or_else(|| Error::Storage("Invalid file path".to_string()))?
                    .to_string();

                for entity in &entities {
                    entities_by_file
                        .entry(file_path_str.clone())
                        .or_default()
                        .push(entity.entity_id.clone());
                }

                batch_entities.extend(entities);
                stats.files_processed += 1;
            }
            Err(e) => {
                warn!("Failed to extract from {}: {}", file_path.display(), e);
                stats.files_failed += 1;
            }
        }
    }

    // Get git commit
    let git_repo = codesearch_watcher::GitRepository::open(repo_root).ok();
    let git_commit = git_repo
        .as_ref()
        .and_then(|repo| repo.current_commit_hash().ok());

    // Process entities with embeddings
    if !batch_entities.is_empty() {
        info!(
            "Generating embeddings for {} entities",
            batch_entities.len()
        );

        // Generate embeddings
        let embedding_texts: Vec<String> = batch_entities
            .iter()
            .map(extract_embedding_content)
            .collect();

        let option_embeddings = embedding_manager
            .embed(embedding_texts)
            .await
            .map_err(|e| Error::Storage(format!("Failed to generate embeddings: {e}")))?;

        // Filter entities with valid embeddings
        let mut entity_embedding_pairs: Vec<(CodeEntity, Vec<f32>)> = Vec::new();
        for (entity, opt_embedding) in batch_entities
            .into_iter()
            .zip(option_embeddings.into_iter())
        {
            if let Some(embedding) = opt_embedding {
                entity_embedding_pairs.push((entity, embedding));
            } else {
                debug!(
                    "Skipped entity due to size: {} in {}",
                    entity.qualified_name,
                    entity.file_path.display()
                );
            }
        }

        if !entity_embedding_pairs.is_empty() {
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

                let (point_id, operation) =
                    if let Some((existing_point_id, deleted_at)) = existing_metadata {
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
        }
    }

    // Update file snapshots and handle stale entities
    for file_path in file_paths {
        let file_path_str = file_path
            .to_str()
            .ok_or_else(|| Error::Storage("Invalid file path".to_string()))?;

        let new_entity_ids = entities_by_file
            .get(file_path_str)
            .cloned()
            .unwrap_or_default();

        if let Err(e) = handle_file_snapshot_update(
            repo_id,
            file_path_str,
            new_entity_ids,
            git_commit.clone(),
            postgres_client,
        )
        .await
        {
            warn!("Failed to update snapshot for {}: {}", file_path_str, e);
        }
    }

    Ok(stats)
}

/// Extract entities from a single file
async fn extract_from_file(file_path: &Path, repo_id: &str) -> Result<Vec<CodeEntity>> {
    if !should_index_file(file_path) {
        return Ok(Vec::new());
    }

    let extractor = match create_extractor(file_path, repo_id) {
        Some(ext) => ext,
        None => {
            debug!("No extractor available for file: {}", file_path.display());
            return Ok(Vec::new());
        }
    };

    let content = tokio::fs::read_to_string(file_path)
        .await
        .map_err(|e| Error::parse(file_path.display().to_string(), e.to_string()))?;

    let entities = extractor
        .extract(&content, file_path)
        .map_err(|e| Error::parse(file_path.display().to_string(), e.to_string()))?;

    Ok(entities)
}

/// Extract embedding content from a CodeEntity
fn extract_embedding_content(entity: &CodeEntity) -> String {
    let mut content = String::with_capacity(500);
    content.push_str(&format!("{} {}", entity.entity_type, entity.name));
    content.push(' ');
    content.push_str(&entity.qualified_name);

    if let Some(doc) = &entity.documentation_summary {
        content.push(' ');
        content.push_str(doc);
    }

    if let Some(sig) = &entity.signature {
        for (name, type_opt) in &sig.parameters {
            content.push(' ');
            content.push_str(name);
            if let Some(param_type) = type_opt {
                content.push_str(": ");
                content.push_str(param_type);
            }
        }
    }

    if let Some(entity_content) = &entity.content {
        content.push(' ');
        content.push_str(entity_content);
    }

    content
}

/// Handle file deletion
async fn handle_file_deletion(
    repo_id: Uuid,
    file_path: &Path,
    postgres_client: &PostgresClient,
) -> Result<usize> {
    info!("Handling deletion of file: {}", file_path.display());

    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| Error::Storage("Invalid file path".to_string()))?;

    let entity_ids = postgres_client
        .get_file_snapshot(repo_id, file_path_str)
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

/// Update file snapshot and handle stale entities
async fn handle_file_snapshot_update(
    repo_id: Uuid,
    file_path: &str,
    new_entity_ids: Vec<String>,
    git_commit: Option<String>,
    postgres_client: &PostgresClient,
) -> Result<()> {
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

    Ok(())
}

/// Check if a file should be indexed based on extension
fn should_index_file(file_path: &Path) -> bool {
    let Some(extension) = file_path.extension() else {
        return false;
    };

    let ext_str = extension.to_string_lossy();
    matches!(
        ext_str.as_ref(),
        "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "go"
    )
}
