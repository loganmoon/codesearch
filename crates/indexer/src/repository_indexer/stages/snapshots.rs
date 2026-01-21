//! Stage 5: Snapshot updates
//!
//! Updates file snapshots and marks stale entities for deletion.

use crate::common::{path_to_str, ResultExt};
use crate::entity_processor;
use crate::repository_indexer::batches::StoredBatch;
use anyhow::anyhow;
use codesearch_core::error::{Error, Result};
use codesearch_storage::PostgresClientTrait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;
use uuid::Uuid;

/// Stage 5: Update file snapshots and mark stale entities
pub(crate) async fn stage_update_snapshots(
    mut stored_rx: mpsc::Receiver<StoredBatch>,
    postgres_client: Arc<dyn PostgresClientTrait>,
    _snapshot_update_concurrency: usize,
) -> Result<usize> {
    // Collect all batches and aggregate files to prevent duplicate processing
    // when a file's entities span multiple batches
    let mut aggregated_files: HashMap<PathBuf, Vec<String>> = HashMap::new();
    let mut repo_id_opt: Option<Uuid> = None;
    let mut collection_name_opt: Option<String> = None;
    let mut git_commit_opt: Option<String> = None;
    let mut total_batches = 0;

    while let Some(batch) = stored_rx.recv().await {
        total_batches += 1;

        // Store metadata from first batch (all batches have same repo/collection/commit)
        if repo_id_opt.is_none() {
            repo_id_opt = Some(batch.repo_id);
            collection_name_opt = Some(batch.collection_name.clone());
            git_commit_opt = batch.git_commit.clone();
        }

        // Merge file entity maps
        for (path, entity_ids) in batch.file_entity_map {
            aggregated_files.entry(path).or_default().extend(entity_ids);
        }
    }

    // Handle empty repository case (no files indexed)
    if total_batches == 0 {
        info!("Stage 5: No batches received (empty repository)");
        return Ok(0);
    }

    let repo_id = repo_id_opt.ok_or_else(|| Error::Other(anyhow!("No batches received")))?;
    let collection_name =
        collection_name_opt.ok_or_else(|| Error::Other(anyhow!("No batches received")))?;
    let git_commit = git_commit_opt.as_ref();

    info!(
        "Stage 5: Aggregated {} batches into {} unique files",
        total_batches,
        aggregated_files.len()
    );

    if aggregated_files.is_empty() {
        return Ok(0);
    }

    // Convert PathBuf to String for all files
    let file_data: Result<Vec<(String, Vec<String>)>> = aggregated_files
        .into_iter()
        .map(|(path, entity_ids)| {
            let file_path_str = path_to_str(&path)?.to_string();
            Ok((file_path_str, entity_ids))
        })
        .collect();
    let file_data = file_data?;

    // Batch fetch all old snapshots (chunked to avoid PostgreSQL stack depth limit)
    let file_refs: Vec<(Uuid, String)> = file_data
        .iter()
        .map(|(path, _)| (repo_id, path.clone()))
        .collect();

    // Chunk into batches of 1000 to avoid "stack depth limit exceeded" error
    const SNAPSHOT_BATCH_SIZE: usize = 1000;
    let mut old_snapshots = std::collections::HashMap::new();
    for chunk in file_refs.chunks(SNAPSHOT_BATCH_SIZE) {
        let chunk_results = postgres_client
            .get_file_snapshots_batch(chunk)
            .await
            .storage_err("Failed to batch fetch file snapshots")?;
        old_snapshots.extend(chunk_results);
    }

    // Compute stale entities for all files
    let mut all_stale_ids = Vec::new();
    for (file_path, new_entity_ids) in &file_data {
        let old_entity_ids = old_snapshots
            .get(&(repo_id, file_path.clone()))
            .cloned()
            .unwrap_or_default();

        let stale_ids = entity_processor::find_stale_entity_ids(&old_entity_ids, new_entity_ids);

        if !stale_ids.is_empty() {
            info!(
                "Stage 5: Found {} stale entities in {}",
                stale_ids.len(),
                file_path
            );
            all_stale_ids.extend(stale_ids);
        }
    }

    // Batch mark all stale entities as deleted
    if !all_stale_ids.is_empty() {
        info!(
            "Stage 5: Marking {} total stale entities as deleted",
            all_stale_ids.len()
        );

        // Fetch token counts for stale entities before deletion
        let entity_refs: Vec<(Uuid, String)> = all_stale_ids
            .iter()
            .map(|entity_id| (repo_id, entity_id.clone()))
            .collect();

        let token_counts = postgres_client
            .get_entity_token_counts(&entity_refs)
            .await
            .storage_err("Failed to get entity token counts")?;

        postgres_client
            .mark_entities_deleted_with_outbox(
                repo_id,
                &collection_name,
                &all_stale_ids,
                &token_counts,
            )
            .await
            .storage_err("Failed to mark entities as deleted")?;
    }

    // Batch update all file snapshots
    let total_snapshots = file_data.len();
    let snapshot_updates: Vec<(String, Vec<String>, Option<String>)> = file_data
        .into_iter()
        .map(|(file_path, entity_ids)| (file_path, entity_ids, git_commit.cloned()))
        .collect();

    // Chunk updates to avoid PostgreSQL stack depth limit
    for chunk in snapshot_updates.chunks(SNAPSHOT_BATCH_SIZE) {
        postgres_client
            .update_file_snapshots_batch(repo_id, chunk)
            .await
            .storage_err("Failed to batch update file snapshots")?;
    }
    info!(
        "Stage 5: Successfully updated {} file snapshots",
        total_snapshots
    );

    Ok(total_snapshots)
}
