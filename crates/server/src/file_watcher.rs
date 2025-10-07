use anyhow::{anyhow, Context};
use codesearch_core::error::Result;
use codesearch_embeddings::EmbeddingManager;
use codesearch_storage::postgres::{OutboxOperation, PostgresClient, TargetStore};
use codesearch_watcher::FileChange;
use serde_json::json;
use std::{path::Path, sync::Arc};
use tracing::{debug, info};
use uuid::Uuid;

/// Reindex a single file after it has changed
pub(crate) async fn reindex_single_file(
    repo_root: &Path,
    repository_id: Uuid,
    file_path: &Path,
    postgres_client: &PostgresClient,
    embedding_manager: &Arc<EmbeddingManager>,
) -> Result<()> {
    // Check if file should be indexed (language support, etc.)
    if !should_index_file(file_path) {
        return Ok(());
    }

    info!("Re-indexing file: {}", file_path.display());

    // 1. Extract entities from the file
    let extractor =
        match codesearch_languages::create_extractor(file_path, &repository_id.to_string()) {
            Some(ext) => ext,
            None => {
                debug!("No extractor available for file: {}", file_path.display());
                return Ok(());
            }
        };

    let content = tokio::fs::read_to_string(file_path)
        .await
        .context("Failed to read file")?;

    let entities = extractor
        .extract(&content, file_path)
        .context("Failed to extract entities from file")?;

    debug!(
        "Extracted {} entities from {}",
        entities.len(),
        file_path.display()
    );

    // Get current git commit
    let git_repo = codesearch_watcher::GitRepository::open(repo_root).ok();
    let git_commit = if let Some(ref repo) = git_repo {
        repo.current_commit_hash().ok()
    } else {
        None
    };

    let mut entity_ids = Vec::new();

    if !entities.is_empty() {
        // 2. Generate embeddings for extracted entities
        let embedding_texts: Vec<String> = entities
            .iter()
            .map(|entity| {
                // Extract embedding content (name, docs, signature, content)
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
            })
            .collect();

        let option_embeddings = embedding_manager
            .embed(embedding_texts)
            .await
            .context("Failed to generate embeddings")?;

        // Filter entities with valid embeddings
        let mut entity_embedding_pairs: Vec<(codesearch_core::CodeEntity, Vec<f32>)> = Vec::new();
        for (entity, opt_embedding) in entities.into_iter().zip(option_embeddings.into_iter()) {
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
            // 3. Check existing metadata and determine operations for each entity
            let mut batch_data = Vec::new();
            for (entity, embedding) in &entity_embedding_pairs {
                entity_ids.push(entity.entity_id.clone());

                let existing_metadata = postgres_client
                    .get_entity_metadata(repository_id, &entity.entity_id)
                    .await
                    .context("Failed to check existing entity metadata")?;

                let (point_id, operation) =
                    if let Some((existing_point_id, deleted_at)) = existing_metadata {
                        if deleted_at.is_some() {
                            // Was deleted, now being re-added
                            (Uuid::new_v4(), OutboxOperation::Insert)
                        } else {
                            // Still active - UPDATE
                            (existing_point_id, OutboxOperation::Update)
                        }
                    } else {
                        // New entity - INSERT
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

            // Store entities with outbox entries
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
                .store_entities_with_outbox_batch(repository_id, &batch_refs)
                .await
                .context("Failed to store entities with outbox")?;

            debug!(
                "Stored {} entities from {}",
                entity_embedding_pairs.len(),
                file_path.display()
            );
        }
    }

    // 4. Handle file change (mark stale entities as deleted, update file snapshot)
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| anyhow!("Invalid file path"))?;

    // Get previous snapshot
    let old_entity_ids = postgres_client
        .get_file_snapshot(repository_id, file_path_str)
        .await
        .context("Failed to get file snapshot")?
        .unwrap_or_default();

    // Find stale entities (in old but not in new)
    let stale_ids: Vec<String> = old_entity_ids
        .iter()
        .filter(|old_id| !entity_ids.contains(old_id))
        .cloned()
        .collect();

    if !stale_ids.is_empty() {
        info!(
            "Found {} stale entities in {}",
            stale_ids.len(),
            file_path_str
        );

        // Mark entities as deleted
        postgres_client
            .mark_entities_deleted(repository_id, &stale_ids)
            .await
            .context("Failed to mark entities as deleted")?;

        // Write DELETE entries to outbox
        for entity_id in &stale_ids {
            let payload = json!({
                "entity_ids": [entity_id],
                "reason": "file_change"
            });

            postgres_client
                .write_outbox_entry(
                    repository_id,
                    entity_id,
                    OutboxOperation::Delete,
                    TargetStore::Qdrant,
                    payload,
                )
                .await
                .context("Failed to write outbox entry")?;
        }
    }

    // Update snapshot with current state
    postgres_client
        .update_file_snapshot(repository_id, file_path_str, entity_ids, git_commit)
        .await
        .context("Failed to update file snapshot")?;

    info!("Successfully re-indexed {}", file_path.display());
    Ok(())
}

/// Handle deletion of a file
pub(crate) async fn handle_file_deletion(
    repository_id: Uuid,
    file_path: &Path,
    postgres_client: &PostgresClient,
) -> Result<()> {
    info!("Handling deletion of file: {}", file_path.display());

    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| anyhow!("Invalid file path"))?;

    // Get entities for this file
    let entity_ids = postgres_client
        .get_file_snapshot(repository_id, file_path_str)
        .await
        .context("Failed to get file snapshot")?
        .unwrap_or_default();

    if entity_ids.is_empty() {
        return Ok(());
    }

    // Mark entities as deleted
    postgres_client
        .mark_entities_deleted(repository_id, &entity_ids)
        .await
        .context("Failed to mark entities as deleted")?;

    // Write DELETE outbox entries
    for entity_id in &entity_ids {
        let payload = json!({
            "entity_ids": [entity_id],
            "reason": "file_deleted"
        });

        postgres_client
            .write_outbox_entry(
                repository_id,
                entity_id,
                OutboxOperation::Delete,
                TargetStore::Qdrant,
                payload,
            )
            .await
            .context("Failed to write outbox entry")?;
    }

    info!("Marked {} entities as deleted", entity_ids.len());
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

/// Handle a single file change event from the watcher
pub(crate) async fn handle_file_change_event(
    event: FileChange,
    repo_root: &Path,
    repository_id: Uuid,
    embedding_manager: &Arc<EmbeddingManager>,
    postgres_client: &Arc<PostgresClient>,
) -> Result<()> {
    match event {
        FileChange::Created(path, _) | FileChange::Modified(path, _) => {
            info!("Re-indexing file: {}", path.display());

            // Re-index the single file (reuse from catch-up)
            reindex_single_file(
                repo_root,
                repository_id,
                &path,
                postgres_client,
                embedding_manager,
            )
            .await?;
        }

        FileChange::Deleted(path) => {
            info!("File deleted: {}", path.display());

            handle_file_deletion(repository_id, &path, postgres_client).await?;
        }

        FileChange::Renamed { from, to } => {
            info!("File renamed: {} -> {}", from.display(), to.display());

            // Treat as delete + create
            handle_file_deletion(repository_id, &from, postgres_client).await?;
            reindex_single_file(
                repo_root,
                repository_id,
                &to,
                postgres_client,
                embedding_manager,
            )
            .await?;
        }

        FileChange::PermissionsChanged(_) => {
            // Ignore permission changes
        }
    }

    Ok(())
}
