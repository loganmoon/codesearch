//! Stage 4: Entity storage
//!
//! Stores entities and embeddings in the database with outbox pattern.

use crate::common::ResultExt;
use crate::repository_indexer::batches::{EmbeddedBatch, EntityEmbeddingTriple, StoredBatch};
use anyhow::anyhow;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use codesearch_storage::{OutboxOperation, PostgresClientTrait, TargetStore};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Stage 4: Store entities and embeddings in database
pub(crate) async fn stage_store_entities(
    mut embedded_rx: mpsc::Receiver<EmbeddedBatch>,
    stored_tx: mpsc::Sender<StoredBatch>,
    postgres_client: Arc<dyn PostgresClientTrait>,
) -> Result<usize> {
    let mut total_stored = 0;
    let max_batch_size = postgres_client.max_entity_batch_size();

    while let Some(batch) = embedded_rx.recv().await {
        info!(
            "Stage 4: Received {} entity-embedding_id-sparse triples from {} files",
            batch.entity_embedding_id_sparse_triples.len(),
            batch.file_indices.len()
        );

        // Use cached collection_name from batch
        let collection_name = &batch.collection_name;

        // Process in chunks to respect max_entity_batch_size
        for chunk_start in
            (0..batch.entity_embedding_id_sparse_triples.len()).step_by(max_batch_size)
        {
            let chunk_end =
                (chunk_start + max_batch_size).min(batch.entity_embedding_id_sparse_triples.len());
            let chunk = &batch.entity_embedding_id_sparse_triples[chunk_start..chunk_end];

            // Deduplicate chunk by entity_id (keep last occurrence)
            // This prevents "ON CONFLICT DO UPDATE command cannot affect row a second time" errors
            let mut unique_chunk: HashMap<String, &EntityEmbeddingTriple> = HashMap::new();
            for triple in chunk {
                unique_chunk.insert(triple.0.entity_id.clone(), triple);
            }
            let deduplicated_chunk: Vec<&EntityEmbeddingTriple> =
                unique_chunk.into_values().collect();

            if deduplicated_chunk.len() < chunk.len() {
                warn!(
                    "Deduplicated {} duplicate entity_ids in Stage 4 chunk ({} -> {} unique)",
                    chunk.len() - deduplicated_chunk.len(),
                    chunk.len(),
                    deduplicated_chunk.len()
                );
            }

            // Batch fetch existing metadata for this chunk
            let entity_ids: Vec<String> = deduplicated_chunk
                .iter()
                .map(|(e, _, _)| e.entity_id.clone())
                .collect();

            let metadata_map = postgres_client
                .get_entities_metadata_batch(batch.repo_id, &entity_ids)
                .await
                .storage_err("Failed to fetch metadata")?;

            // Calculate token counts for this chunk
            let entities_vec: Vec<&CodeEntity> =
                deduplicated_chunk.iter().map(|(e, _, _)| e).collect();
            let entities_owned: Vec<CodeEntity> = entities_vec.iter().map(|&e| e.clone()).collect();
            let token_counts = crate::entity_processor::calculate_token_counts(&entities_owned)
                .storage_err("Failed to calculate token counts")?;

            // Prepare batch refs (no cloning - use references)
            let mut batch_refs = Vec::with_capacity(deduplicated_chunk.len());

            // Clone git_commit once for the chunk instead of per entity
            let git_commit = batch.git_commit.clone();

            for (idx, (entity, embedding_id, _sparse_embedding)) in
                deduplicated_chunk.iter().enumerate()
            {
                let (point_id, operation) = if let Some((existing_point_id, deleted_at)) =
                    metadata_map.get(&entity.entity_id)
                {
                    if deleted_at.is_some() {
                        (Uuid::new_v4(), OutboxOperation::Insert)
                    } else {
                        (*existing_point_id, OutboxOperation::Update)
                    }
                } else {
                    (Uuid::new_v4(), OutboxOperation::Insert)
                };

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

            // Store in DB with outbox
            postgres_client
                .store_entities_with_outbox_batch(batch.repo_id, collection_name, &batch_refs)
                .await
                .storage_err("Failed to store entities")?;

            // Note: BM25 statistics are updated by the outbox processor within its transaction

            total_stored += batch_refs.len();
            info!(
                "Stage 4: Successfully stored chunk of {} entities ({}/{} total in this batch)",
                batch_refs.len(),
                chunk_end,
                batch.entity_embedding_id_sparse_triples.len()
            );
        }

        info!(
            "Stage 4: Completed storing {} entities from this batch",
            batch.entity_embedding_id_sparse_triples.len()
        );

        // Build file→entity_id map for snapshots
        let mut file_entity_map = HashMap::new();

        for (path, entity_indices) in batch.file_indices {
            let entity_ids: Vec<String> = entity_indices
                .iter()
                .filter_map(|&idx| {
                    if idx < batch.entity_embedding_id_sparse_triples.len() {
                        Some(
                            batch.entity_embedding_id_sparse_triples[idx]
                                .0
                                .entity_id
                                .clone(),
                        )
                    } else {
                        error!(
                            "Stage 4: Index {} out of bounds (len: {})",
                            idx,
                            batch.entity_embedding_id_sparse_triples.len()
                        );
                        None
                    }
                })
                .collect();

            // Always insert files into map, even if they have 0 entities
            // This ensures file snapshots are updated and old entities are deleted
            file_entity_map.insert(path, entity_ids);
        }

        info!(
            "Stage 4: Built file_entity_map with {} files",
            file_entity_map.len()
        );

        stored_tx
            .send(StoredBatch {
                file_entity_map,
                repo_id: batch.repo_id,
                collection_name: collection_name.to_string(),
                git_commit: batch.git_commit,
            })
            .await
            .map_err(|_| Error::Other(anyhow!("Stored channel closed")))?;
    }

    drop(stored_tx);
    info!("Stored {total_stored} entities");
    Ok(total_stored)
}
