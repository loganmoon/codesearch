//! Stage 3: Embedding generation
//!
//! Generates dense and sparse embeddings for entities with caching support.

use crate::common::ResultExt;
use crate::entity_processor;
use crate::repository_indexer::batches::{EmbeddedBatch, EntityBatch};
use anyhow::anyhow;
use codesearch_core::config::SparseEmbeddingsConfig;
use codesearch_core::error::{Error, Result};
use codesearch_embeddings::{EmbeddingContext, EmbeddingManager, EmbeddingTask};
use codesearch_storage::{EmbeddingCacheEntry, PostgresClientTrait};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

/// Stage 3: Generate embeddings for entities in parallel
pub(crate) async fn stage_generate_embeddings(
    mut entity_rx: mpsc::Receiver<EntityBatch>,
    embedded_tx: mpsc::Sender<EmbeddedBatch>,
    embedding_manager: Arc<EmbeddingManager>,
    postgres_client: Arc<dyn PostgresClientTrait>,
    sparse_embeddings_config: SparseEmbeddingsConfig,
    pre_initialized_sparse_manager: Option<Arc<codesearch_embeddings::SparseEmbeddingManager>>,
) -> Result<usize> {
    let mut total_embedded = 0;
    let mut total_skipped = 0;

    while let Some(batch) = entity_rx.recv().await {
        info!(
            "Stage 3: Received batch with {} entities from {} files",
            batch.entities.len(),
            batch.file_indices.len()
        );

        // Extract embedding content and compute hashes
        let texts: Vec<String> = batch
            .entities
            .iter()
            .map(entity_processor::extract_embedding_content)
            .collect();

        // Log text statistics
        let text_lengths: Vec<usize> = texts.iter().map(|t| t.len()).collect();
        let min_len = text_lengths.iter().copied().min().unwrap_or(0);
        let max_len = text_lengths.iter().copied().max().unwrap_or(0);
        let avg_len = if text_lengths.is_empty() {
            0
        } else {
            text_lengths.iter().sum::<usize>() / text_lengths.len()
        };

        info!(
            "Stage 3: Extracted {} texts for embedding (lengths: min={}, max={}, avg={})",
            texts.len(),
            min_len,
            max_len,
            avg_len
        );

        // Log first few entity names for debugging
        let sample_entities: Vec<String> = batch
            .entities
            .iter()
            .take(3)
            .map(|e| e.qualified_name.to_string())
            .collect();
        tracing::debug!("Stage 3: Sample entities: {:?}", sample_entities);

        //  Compute content hashes
        use twox_hash::XxHash3_128;
        let content_hashes: Vec<String> = texts
            .iter()
            .map(|text| format!("{:032x}", XxHash3_128::oneshot(text.as_bytes())))
            .collect();

        // Batch lookup cached embeddings
        let model_version = embedding_manager.model_version();
        let cached_embeddings = postgres_client
            .get_embeddings_by_content_hash(batch.repo_id, &content_hashes, model_version)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(
                    "Stage 3: Cache lookup failed, will generate all embeddings: {}",
                    e
                );
                HashMap::new()
            });

        // Initialize result vectors
        let mut all_embeddings: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        let mut all_embedding_ids: Vec<Option<i64>> = vec![None; texts.len()];
        let mut all_sparse_embeddings: Vec<Option<Vec<(u32, f32)>>> = vec![None; texts.len()];

        // Separate cache hits from misses, populating results directly
        let mut cache_hit_count = 0;
        let mut cache_miss_indices: Vec<usize> = Vec::new();
        let mut cache_miss_texts: Vec<String> = Vec::new();

        for (idx, (text, content_hash)) in texts.iter().zip(content_hashes.iter()).enumerate() {
            if let Some((embedding_id, cached_embedding, cached_sparse)) =
                cached_embeddings.get(content_hash)
            {
                // Directly populate results for cache hits
                all_embeddings[idx] = Some(cached_embedding.clone());
                all_embedding_ids[idx] = Some(*embedding_id);
                all_sparse_embeddings[idx] = cached_sparse.clone();
                cache_hit_count += 1;
            } else {
                cache_miss_indices.push(idx);
                cache_miss_texts.push(text.clone());
            }
        }

        info!(
            "Stage 3: Embedding cache: {} hits, {} misses ({:.1}% hit rate)",
            cache_hit_count,
            cache_miss_texts.len(),
            if !texts.is_empty() {
                (cache_hit_count as f64 / texts.len() as f64) * 100.0
            } else {
                0.0
            }
        );

        // Generate embeddings only for cache misses
        if !cache_miss_texts.is_empty() {
            let cache_miss_count = cache_miss_texts.len();
            info!(
                "Stage 3: Generating {} new embeddings via API",
                cache_miss_count
            );

            // Build EmbeddingContext for each cache miss entity
            let contexts: Vec<EmbeddingContext> = cache_miss_indices
                .iter()
                .map(|entity_idx| {
                    let entity = &batch.entities[*entity_idx];
                    EmbeddingContext {
                        qualified_name: entity.qualified_name.to_string(),
                        file_path: entity.file_path.clone(),
                        line_number: entity.location.start_line as u32,
                        entity_type: format!("{:?}", entity.entity_type),
                    }
                })
                .collect();

            let new_embeddings = embedding_manager
                .embed_for_task(
                    cache_miss_texts.clone(),
                    Some(contexts),
                    EmbeddingTask::Passage,
                )
                .await
                .storage_err("Failed to generate embeddings")?;

            // Fill in newly generated dense embeddings
            for (miss_idx, emb_opt) in cache_miss_indices.iter().zip(new_embeddings.iter()) {
                if let Some(embedding) = emb_opt {
                    all_embeddings[*miss_idx] = Some(embedding.clone());
                }
            }

            // Generate sparse embeddings for cache misses only
            info!(
                "Stage 3: Generating {} sparse embeddings for cache misses",
                cache_miss_count
            );

            // Use pre-initialized sparse manager if available, otherwise create one
            let sparse_manager = if let Some(ref mgr) = pre_initialized_sparse_manager {
                Arc::clone(mgr)
            } else {
                // Fall back to lazy creation (needed for BM25 which requires avgdl from DB)
                let bm25_stats = postgres_client
                    .get_bm25_statistics(batch.repo_id)
                    .await
                    .storage_err("Failed to get BM25 statistics")?;

                match codesearch_embeddings::create_sparse_manager_from_config(
                    &sparse_embeddings_config,
                    bm25_stats.avgdl,
                )
                .await
                {
                    Ok(mgr) => mgr,
                    Err(e) => {
                        error!("Stage 3: Failed to create sparse embedding manager: {e}");
                        return Err(e);
                    }
                }
            };

            let new_sparse_embeddings = match sparse_manager
                .embed_sparse(cache_miss_texts.iter().map(|s| s.as_str()).collect())
                .await
            {
                Ok(embs) => embs,
                Err(e) => {
                    error!("Stage 3: Failed to generate sparse embeddings: {e}");
                    return Err(Error::Storage(format!(
                        "Failed to generate sparse embeddings: {e}"
                    )));
                }
            };

            // Fill in newly generated sparse embeddings
            for (miss_idx, sparse_opt) in
                cache_miss_indices.iter().zip(new_sparse_embeddings.iter())
            {
                if let Some(sparse) = sparse_opt {
                    all_sparse_embeddings[*miss_idx] = Some(sparse.clone());
                }
            }

            // Store both dense and sparse embeddings in cache
            let embeddings_to_store: Vec<EmbeddingCacheEntry> = cache_miss_indices
                .iter()
                .zip(new_embeddings.iter().zip(new_sparse_embeddings.iter()))
                .filter_map(|(idx, (emb_opt, sparse_opt))| {
                    emb_opt.as_ref().map(|emb| {
                        (
                            content_hashes[*idx].clone(),
                            emb.clone(),
                            sparse_opt.clone(),
                        )
                    })
                })
                .collect();

            if !embeddings_to_store.is_empty() {
                let dimension = embeddings_to_store[0].1.len();

                let new_embedding_ids = postgres_client
                    .store_embeddings(
                        batch.repo_id,
                        &embeddings_to_store,
                        model_version,
                        dimension,
                    )
                    .await
                    .storage_err("Failed to store embeddings")?;

                // Map returned IDs back to entity indices
                let mut new_id_iter = new_embedding_ids.into_iter();
                for (idx, emb_opt) in cache_miss_indices.iter().zip(new_embeddings.iter()) {
                    if emb_opt.is_some() {
                        if let Some(embedding_id) = new_id_iter.next() {
                            all_embedding_ids[*idx] = Some(embedding_id);
                        }
                    }
                }

                info!(
                    "Stage 3: Stored {} new embeddings in cache",
                    embeddings_to_store.len()
                );
            }
        }

        let successful_embeddings = all_embeddings.iter().filter(|e| e.is_some()).count();
        let successful_sparse = all_sparse_embeddings.iter().filter(|e| e.is_some()).count();
        info!(
            "Stage 3: Successfully obtained {} embeddings and {} sparse embeddings ({} dense skipped, {} sparse skipped)",
            successful_embeddings,
            successful_sparse,
            texts.len() - successful_embeddings,
            texts.len() - successful_sparse
        );

        // Create triples of (entity, embedding_id, sparse_embedding), tracking which indices survived
        let mut triples = Vec::new();
        let mut old_to_new_idx: HashMap<usize, usize> = HashMap::new();

        for (old_idx, (entity, ((emb_opt, id_opt), sparse_emb_opt))) in batch
            .entities
            .into_iter()
            .zip(
                all_embeddings
                    .into_iter()
                    .zip(all_embedding_ids.into_iter())
                    .zip(all_sparse_embeddings.into_iter()),
            )
            .enumerate()
        {
            if let (Some(_embedding), Some(embedding_id), Some(sparse_emb)) =
                (emb_opt, id_opt, sparse_emb_opt)
            {
                let new_idx = triples.len();
                old_to_new_idx.insert(old_idx, new_idx);
                triples.push((entity, embedding_id, sparse_emb));
            }
        }

        let skipped = texts.len() - triples.len();
        total_embedded += triples.len();
        total_skipped += skipped;

        // Update file_indices to use new indices (after filtering)
        // Keep files with 0 entities so their snapshots get updated
        let updated_file_indices: Vec<(std::path::PathBuf, Vec<usize>)> = batch
            .file_indices
            .into_iter()
            .map(|(path, old_indices)| {
                let new_indices: Vec<usize> = old_indices
                    .into_iter()
                    .filter_map(|old_idx| old_to_new_idx.get(&old_idx).copied())
                    .collect();

                (path, new_indices)
            })
            .collect();

        embedded_tx
            .send(EmbeddedBatch {
                entity_embedding_id_sparse_triples: triples,
                file_indices: updated_file_indices,
                repo_id: batch.repo_id,
                git_commit: batch.git_commit,
                collection_name: batch.collection_name,
            })
            .await
            .map_err(|_| Error::Other(anyhow!("Embedded channel closed")))?;
    }

    drop(embedded_tx);
    info!("Embedded {total_embedded} entities, skipped {total_skipped}");
    Ok(total_embedded)
}
