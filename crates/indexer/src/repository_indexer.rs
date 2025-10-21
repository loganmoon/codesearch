//! Repository indexer implementation
//!
//! Provides the main pipelined indexing pipeline for processing repositories.

use crate::common::{get_current_commit, path_to_str, ResultExt};
use crate::config::IndexerConfig;
use crate::entity_processor;
use crate::{IndexResult, IndexStats};
use anyhow::anyhow;
use async_trait::async_trait;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use codesearch_embeddings::EmbeddingManager;
use codesearch_storage::{OutboxOperation, PostgresClientTrait, TargetStore};
use futures::stream::{self, StreamExt};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

// Pipeline data structures for multi-stage indexing
struct FileBatch {
    paths: Vec<PathBuf>,
}

struct EntityBatch {
    entities: Vec<CodeEntity>,
    // Track which files produced which entities for snapshot updates
    // (file_path, entity_indices_in_batch)
    file_indices: Vec<(PathBuf, Vec<usize>)>,
    repo_id: uuid::Uuid,
    git_commit: Option<String>,
    collection_name: String,
    #[allow(dead_code)] // Tracked but not returned to caller (logged instead)
    failed_files: usize,
}

struct EmbeddedBatch {
    // Entities paired with embedding IDs (skipped entities filtered out)
    entity_embedding_id_pairs: Vec<(CodeEntity, i64)>,
    file_indices: Vec<(PathBuf, Vec<usize>)>,
    repo_id: uuid::Uuid,
    git_commit: Option<String>,
    collection_name: String,
    #[allow(dead_code)] // Tracked but not returned to caller (logged instead)
    entities_skipped: usize,
}

struct StoredBatch {
    // Metadata for snapshot updates (entities already stored)
    file_entity_map: std::collections::HashMap<PathBuf, Vec<String>>,
    repo_id: uuid::Uuid,
    collection_name: String,
    git_commit: Option<String>,
}

/// Main repository indexer
pub struct RepositoryIndexer {
    repository_path: PathBuf,
    repository_id: uuid::Uuid,
    embedding_manager: std::sync::Arc<EmbeddingManager>,
    postgres_client: std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
    git_repo: Option<codesearch_watcher::GitRepository>,
    config: IndexerConfig,
}

impl RepositoryIndexer {
    /// Create a new repository indexer
    pub fn new(
        repository_path: PathBuf,
        repository_id: String,
        embedding_manager: std::sync::Arc<EmbeddingManager>,
        postgres_client: std::sync::Arc<dyn codesearch_storage::PostgresClientTrait>,
        git_repo: Option<codesearch_watcher::GitRepository>,
        config: IndexerConfig,
    ) -> Result<Self> {
        debug!(
            "RepositoryIndexer::new called with repository_id string = {}",
            repository_id
        );
        let repository_id = uuid::Uuid::parse_str(&repository_id)
            .map_err(|e| Error::Storage(format!("Invalid repository ID: {e}")))?;

        debug!("RepositoryIndexer::new parsed UUID = {}", repository_id);

        Ok(Self {
            repository_path,
            repository_id,
            embedding_manager,
            postgres_client,
            git_repo,
            config,
        })
    }

    /// Get the repository path
    pub fn repository_path(&self) -> &Path {
        &self.repository_path
    }
}

// Pipeline stage functions

/// Stage 1: Discover all files in the repository and stream them in batches
///
/// This function implements streaming file discovery with the following optimizations:
/// - **Parallel traversal**: Uses multiple threads (auto-detected, capped at 12, defaults to 4 if detection fails) for faster discovery
/// - **Gitignore support**: Automatically respects `.gitignore`, `.git/info/exclude`, and global ignore files
/// - **Streaming batches**: Sends batches to downstream stages as they're discovered, enabling
///   pipeline parallelism where Stage 2 (entity extraction) begins processing files before
///   Stage 1 completes discovery
/// - **Memory efficiency**: Only keeps one batch in memory at a time, rather than all file paths
/// - **Lock-free architecture**: Uses channels instead of shared mutable state (Arc<Mutex<Vec>>)
///
/// Benefits over collect-then-batch approach:
/// - Reduced time-to-first-extraction: Downstream stages start immediately
/// - Better CPU utilization: All pipeline stages can run concurrently
/// - Lower peak memory usage: No need to hold all paths in memory
/// - No mutex contention between walker threads
async fn stage_file_discovery(
    file_tx: mpsc::Sender<FileBatch>,
    repo_path: PathBuf,
    batch_size: usize,
) -> Result<usize> {
    use ignore::WalkBuilder;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // Calculate parallelism: min(available cores, 12)
    // Higher cap for I/O-bound file discovery (benefits from concurrency on modern SSDs)
    let parallelism = std::thread::available_parallelism()
        .map(|n| n.get().min(12))
        .unwrap_or(4);

    debug!(
        "Streaming file discovery using {} threads for {}",
        parallelism,
        repo_path.display()
    );

    // Create bounded channel for individual paths from walker threads
    // Capacity of batch_size * 2 provides buffering while preventing unbounded memory growth
    // Walker threads will apply backpressure if coordinator falls behind
    let (path_tx, mut path_rx) = mpsc::channel::<PathBuf>(batch_size * 2);
    let total_files = Arc::new(AtomicUsize::new(0));

    // Spawn coordinator task to batch individual paths
    let batch_tx = file_tx.clone();
    let total_for_coordinator = Arc::clone(&total_files);
    let coordinator = tokio::spawn(async move {
        let mut current_batch = Vec::with_capacity(batch_size);

        while let Some(path) = path_rx.recv().await {
            current_batch.push(path);
            total_for_coordinator.fetch_add(1, Ordering::Relaxed);

            // Send batch when it reaches batch_size
            if current_batch.len() >= batch_size {
                let batch = std::mem::replace(&mut current_batch, Vec::with_capacity(batch_size));
                if let Err(e) = batch_tx.send(FileBatch { paths: batch }).await {
                    warn!("Failed to send file batch: {}", e);
                    break;
                }
            }
        }

        // Send any remaining files in the last batch
        if !current_batch.is_empty() {
            if let Err(e) = batch_tx
                .send(FileBatch {
                    paths: current_batch,
                })
                .await
            {
                warn!("Failed to send final file batch: {}", e);
            }
        }
    });

    // Build parallel walker with gitignore support
    // Run in blocking task since WalkBuilder::run is synchronous
    let walk_handle = tokio::task::spawn_blocking(move || {
        WalkBuilder::new(&repo_path)
            .standard_filters(true)
            .hidden(false)
            .parents(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .require_git(false)
            .threads(parallelism)
            .build_parallel()
            .run(|| {
                let tx = path_tx.clone();

                Box::new(move |entry_result| {
                    use crate::common::{has_supported_extension, should_include_file};
                    use ignore::WalkState;

                    match entry_result {
                        Ok(entry) => {
                            let path = entry.path();

                            // Apply filters in order of cost (cheap to expensive)
                            // 1. Check file type first (already cached in DirEntry, free)
                            if let Some(file_type) = entry.file_type() {
                                if !file_type.is_file() {
                                    return WalkState::Continue;
                                }
                            }

                            // 2. Check extension (cheap string operation)
                            if !has_supported_extension(path) {
                                return WalkState::Continue;
                            }

                            // 3. Check symlink/size (requires metadata syscall)
                            if !should_include_file(path) {
                                return WalkState::Continue;
                            }

                            // Send path to coordinator for batching
                            // Use blocking_send since we're in a sync context with bounded channel
                            if let Err(e) = tx.blocking_send(path.to_path_buf()) {
                                warn!("Failed to send path to coordinator: {}", e);
                                return WalkState::Quit;
                            }

                            WalkState::Continue
                        }
                        Err(e) => {
                            warn!("Error reading file entry: {}", e);
                            WalkState::Continue
                        }
                    }
                })
            });
    });

    // Wait for walker to complete
    // When this completes, path_tx is automatically dropped, signaling coordinator
    walk_handle
        .await
        .map_err(|e| Error::Other(anyhow!("Walker task panicked: {e}")))?;

    // Wait for coordinator to finish sending all batches
    coordinator
        .await
        .map_err(|e| Error::Other(anyhow!("Coordinator task panicked: {e}")))?;

    let total = total_files.load(Ordering::Relaxed);
    info!("Discovered {total} files to index");
    Ok(total)
}

/// Stage 2: Extract entities from files in parallel
async fn stage_extract_entities(
    mut file_rx: mpsc::Receiver<FileBatch>,
    entity_tx: mpsc::Sender<EntityBatch>,
    repo_id: Uuid,
    git_commit: Option<String>,
    collection_name: String,
    max_entity_batch_size: usize,
    file_extraction_concurrency: usize,
) -> Result<(usize, usize)> {
    let mut total_extracted = 0;
    let mut total_failed = 0;

    // Accumulator for building entity batches
    let mut entities = Vec::new();
    let mut file_indices = Vec::new();
    let mut batch_failed = 0;

    while let Some(FileBatch { paths }) = file_rx.recv().await {
        debug!("Extracting entities from {} files", paths.len());

        // Convert repo_id once for the entire batch
        let repo_id_str = repo_id.to_string();

        // Process files in parallel (8 concurrent extractions), collect results
        let results = stream::iter(paths.into_iter())
            .map(|path| {
                let repo_id_ref = &repo_id_str;
                async move {
                    match entity_processor::extract_entities_from_file(&path, repo_id_ref).await {
                        Ok(entities) => Ok((path, entities)),
                        Err(e) => {
                            error!("Failed to extract from {}: {e}", path.display());
                            Err(())
                        }
                    }
                }
            })
            .buffer_unordered(file_extraction_concurrency)
            .collect::<Vec<_>>()
            .await;

        // Process results and batch by entity count
        for result in results {
            match result {
                Ok((path, mut file_entities)) => {
                    if file_entities.is_empty() {
                        // File has 0 entities - track it so snapshot gets updated
                        file_indices.push((path, Vec::new()));
                    } else {
                        // Process entities from this file, potentially across multiple batches
                        while !file_entities.is_empty() {
                            let space_left = max_entity_batch_size.saturating_sub(entities.len());

                            if space_left == 0 {
                                // Current batch is full, send it
                                let batch_entities = std::mem::take(&mut entities);
                                let batch_file_indices = std::mem::take(&mut file_indices);

                                total_extracted += batch_entities.len();
                                total_failed += batch_failed;

                                info!(
                                    "Stage 2: Sending batch with {} entities from {} files ({} failed)",
                                    batch_entities.len(),
                                    batch_file_indices.len(),
                                    batch_failed
                                );

                                entity_tx
                                    .send(EntityBatch {
                                        entities: batch_entities,
                                        file_indices: batch_file_indices,
                                        repo_id,
                                        git_commit: git_commit.clone(),
                                        collection_name: collection_name.clone(),
                                        failed_files: batch_failed,
                                    })
                                    .await
                                    .map_err(|_| Error::Other(anyhow!("Entity channel closed")))?;

                                batch_failed = 0;
                                continue;
                            }

                            // Add as many entities as fit in current batch
                            let to_take = space_left.min(file_entities.len());
                            let start_idx = entities.len();
                            let chunk: Vec<_> = file_entities.drain(..to_take).collect();
                            entities.extend(chunk);
                            let end_idx = entities.len();

                            // Track file indices for this chunk
                            if let Some((last_path, last_indices)) = file_indices.last_mut() {
                                if last_path == &path {
                                    // Extend existing file entry
                                    last_indices.extend(start_idx..end_idx);
                                } else {
                                    // New file entry
                                    file_indices
                                        .push((path.clone(), (start_idx..end_idx).collect()));
                                }
                            } else {
                                // First file entry
                                file_indices.push((path.clone(), (start_idx..end_idx).collect()));
                            }
                        }
                    }
                }
                Err(()) => batch_failed += 1,
            }
        }
    }

    // Send any remaining entities or files with 0 entities
    if !entities.is_empty() || !file_indices.is_empty() {
        total_extracted += entities.len();
        total_failed += batch_failed;

        info!(
            "Stage 2: Sending batch with {} entities from {} files ({} failed)",
            entities.len(),
            file_indices.len(),
            batch_failed
        );

        entity_tx
            .send(EntityBatch {
                entities,
                file_indices,
                repo_id,
                git_commit: git_commit.clone(),
                collection_name: collection_name.clone(),
                failed_files: batch_failed,
            })
            .await
            .map_err(|_| Error::Other(anyhow!("Entity channel closed")))?;
    } else if batch_failed > 0 {
        // Track failures even if no entities remain
        total_failed += batch_failed;
    }

    drop(entity_tx);
    info!("Extracted {total_extracted} entities, {total_failed} files failed");
    Ok((total_extracted, total_failed))
}

/// Stage 3: Generate embeddings for entities in parallel
async fn stage_generate_embeddings(
    mut entity_rx: mpsc::Receiver<EntityBatch>,
    embedded_tx: mpsc::Sender<EmbeddedBatch>,
    embedding_manager: Arc<EmbeddingManager>,
    postgres_client: Arc<dyn PostgresClientTrait>,
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
        let sample_entities: Vec<&String> = batch
            .entities
            .iter()
            .take(3)
            .map(|e| &e.qualified_name)
            .collect();
        debug!("Stage 3: Sample entities: {:?}", sample_entities);

        //  Compute content hashes
        use twox_hash::XxHash3_128;
        let content_hashes: Vec<String> = texts
            .iter()
            .map(|text| format!("{:032x}", XxHash3_128::oneshot(text.as_bytes())))
            .collect();

        // Batch lookup cached embeddings
        let model_version = embedding_manager.model_version();
        let cached_embeddings = postgres_client
            .get_embeddings_by_content_hash(&content_hashes, model_version)
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

        // Separate cache hits from misses, populating results directly
        let mut cache_hit_count = 0;
        let mut cache_miss_indices: Vec<usize> = Vec::new();
        let mut cache_miss_texts: Vec<String> = Vec::new();

        for (idx, (text, content_hash)) in texts.iter().zip(content_hashes.iter()).enumerate() {
            if let Some((embedding_id, cached_embedding)) = cached_embeddings.get(content_hash) {
                // Directly populate results for cache hits
                all_embeddings[idx] = Some(cached_embedding.clone());
                all_embedding_ids[idx] = Some(*embedding_id);
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

            let new_embeddings = embedding_manager
                .embed(cache_miss_texts)
                .await
                .map_err(|e| {
                    error!(
                        "Stage 3: Embedding failed for {} texts, error: {}",
                        cache_miss_count, e
                    );
                    e
                })
                .storage_err("Failed to generate embeddings")?;

            // Fill in newly generated embeddings
            for (miss_idx, emb_opt) in cache_miss_indices.iter().zip(new_embeddings.iter()) {
                if let Some(embedding) = emb_opt {
                    all_embeddings[*miss_idx] = Some(embedding.clone());
                }
            }

            // Store newly generated embeddings in cache
            let embeddings_to_store: Vec<(String, Vec<f32>)> = cache_miss_indices
                .iter()
                .zip(new_embeddings.iter())
                .filter_map(|(idx, emb_opt)| {
                    emb_opt
                        .as_ref()
                        .map(|emb| (content_hashes[*idx].clone(), emb.clone()))
                })
                .collect();

            if !embeddings_to_store.is_empty() {
                let dimension = embeddings_to_store[0].1.len();

                let new_embedding_ids = postgres_client
                    .store_embeddings(&embeddings_to_store, model_version, dimension)
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
        info!(
            "Stage 3: Successfully obtained {} embeddings ({} skipped)",
            successful_embeddings,
            texts.len() - successful_embeddings
        );

        // Pair entities with embedding IDs, tracking which indices survived
        let mut pairs = Vec::new();
        let mut old_to_new_idx: HashMap<usize, usize> = HashMap::new();

        for (old_idx, (entity, (emb_opt, id_opt))) in batch
            .entities
            .into_iter()
            .zip(
                all_embeddings
                    .into_iter()
                    .zip(all_embedding_ids.into_iter()),
            )
            .enumerate()
        {
            if let (Some(_embedding), Some(embedding_id)) = (emb_opt, id_opt) {
                let new_idx = pairs.len();
                old_to_new_idx.insert(old_idx, new_idx);
                pairs.push((entity, embedding_id));
            }
        }

        let skipped = texts.len() - pairs.len();
        total_embedded += pairs.len();
        total_skipped += skipped;

        // Update file_indices to use new indices (after filtering)
        // Keep files with 0 entities so their snapshots get updated
        let updated_file_indices: Vec<(PathBuf, Vec<usize>)> = batch
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
                entity_embedding_id_pairs: pairs,
                file_indices: updated_file_indices,
                repo_id: batch.repo_id,
                git_commit: batch.git_commit,
                collection_name: batch.collection_name,
                entities_skipped: skipped,
            })
            .await
            .map_err(|_| Error::Other(anyhow!("Embedded channel closed")))?;
    }

    drop(embedded_tx);
    info!("Embedded {total_embedded} entities, skipped {total_skipped}");
    Ok(total_embedded)
}

/// Stage 4: Store entities and embeddings in database
async fn stage_store_entities(
    mut embedded_rx: mpsc::Receiver<EmbeddedBatch>,
    stored_tx: mpsc::Sender<StoredBatch>,
    postgres_client: Arc<dyn PostgresClientTrait>,
) -> Result<usize> {
    let mut total_stored = 0;
    let max_batch_size = postgres_client.max_entity_batch_size();

    while let Some(batch) = embedded_rx.recv().await {
        info!(
            "Stage 4: Received {} entity-embedding_id pairs from {} files",
            batch.entity_embedding_id_pairs.len(),
            batch.file_indices.len()
        );

        // Use cached collection_name from batch
        let collection_name = &batch.collection_name;

        // Process in chunks to respect max_entity_batch_size
        for chunk_start in (0..batch.entity_embedding_id_pairs.len()).step_by(max_batch_size) {
            let chunk_end =
                (chunk_start + max_batch_size).min(batch.entity_embedding_id_pairs.len());
            let chunk = &batch.entity_embedding_id_pairs[chunk_start..chunk_end];

            // Batch fetch existing metadata for this chunk
            let entity_ids: Vec<String> = chunk.iter().map(|(e, _)| e.entity_id.clone()).collect();

            let metadata_map = postgres_client
                .get_entities_metadata_batch(batch.repo_id, &entity_ids)
                .await
                .storage_err("Failed to fetch metadata")?;

            // Calculate token counts for this chunk
            let entities_vec: Vec<&CodeEntity> = chunk.iter().map(|(e, _)| e).collect();
            let entities_owned: Vec<CodeEntity> = entities_vec.iter().map(|&e| e.clone()).collect();
            let token_counts = crate::entity_processor::calculate_token_counts(&entities_owned)
                .storage_err("Failed to calculate token counts")?;

            // Prepare batch refs (no cloning - use references)
            let mut batch_refs = Vec::with_capacity(chunk.len());

            // Clone git_commit once for the chunk instead of per entity
            let git_commit = batch.git_commit.clone();

            for (idx, (entity, embedding_id)) in chunk.iter().enumerate() {
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

            // Update avgdl statistics incrementally
            let avgdl = postgres_client
                .update_bm25_statistics_incremental(batch.repo_id, &token_counts)
                .await
                .storage_err("Failed to update BM25 statistics")?;

            total_stored += batch_refs.len();
            info!(
                "Stage 4: Successfully stored chunk of {} entities ({}/{} total in this batch), avgdl={:.2}",
                batch_refs.len(),
                chunk_end,
                batch.entity_embedding_id_pairs.len(),
                avgdl
            );
        }

        info!(
            "Stage 4: Completed storing {} entities from this batch",
            batch.entity_embedding_id_pairs.len()
        );

        // Build file→entity_id map for snapshots
        let mut file_entity_map = HashMap::new();

        for (path, entity_indices) in batch.file_indices {
            let entity_ids: Vec<String> = entity_indices
                .iter()
                .filter_map(|&idx| {
                    if idx < batch.entity_embedding_id_pairs.len() {
                        Some(batch.entity_embedding_id_pairs[idx].0.entity_id.clone())
                    } else {
                        error!(
                            "Stage 4: Index {} out of bounds (len: {})",
                            idx,
                            batch.entity_embedding_id_pairs.len()
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

/// Stage 5: Update file snapshots and mark stale entities
async fn stage_update_snapshots(
    mut stored_rx: mpsc::Receiver<StoredBatch>,
    postgres_client: Arc<dyn PostgresClientTrait>,
    _snapshot_update_concurrency: usize,
) -> Result<usize> {
    let mut total_snapshots = 0;

    while let Some(batch) = stored_rx.recv().await {
        info!(
            "Stage 5: Updating {} file snapshots",
            batch.file_entity_map.len()
        );

        let file_count = batch.file_entity_map.len();
        let repo_id = batch.repo_id;
        let collection_name = &batch.collection_name;
        let git_commit = batch.git_commit.as_ref();

        // Convert PathBuf to String for all files
        let file_data: Result<Vec<(String, Vec<String>)>> = batch
            .file_entity_map
            .into_iter()
            .map(|(path, entity_ids)| {
                let file_path_str = path_to_str(&path)?.to_string();
                Ok((file_path_str, entity_ids))
            })
            .collect();
        let file_data = file_data?;

        // Batch fetch all old snapshots
        let file_refs: Vec<(Uuid, String)> = file_data
            .iter()
            .map(|(path, _)| (repo_id, path.clone()))
            .collect();

        let old_snapshots = postgres_client
            .get_file_snapshots_batch(&file_refs)
            .await
            .storage_err("Failed to batch fetch file snapshots")?;

        // Compute stale entities for all files
        let mut all_stale_ids = Vec::new();
        for (file_path, new_entity_ids) in &file_data {
            let old_entity_ids = old_snapshots
                .get(&(repo_id, file_path.clone()))
                .cloned()
                .unwrap_or_default();

            // Use HashSet for O(1) lookups instead of O(n) Vec::contains
            let new_entity_set: std::collections::HashSet<&String> =
                new_entity_ids.iter().collect();
            let stale_ids: Vec<String> = old_entity_ids
                .iter()
                .filter(|old_id| !new_entity_set.contains(old_id))
                .cloned()
                .collect();

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
            postgres_client
                .mark_entities_deleted_with_outbox(repo_id, collection_name, &all_stale_ids)
                .await
                .storage_err("Failed to mark entities as deleted")?;
        }

        // Batch update all file snapshots
        let snapshot_updates: Vec<(String, Vec<String>, Option<String>)> = file_data
            .into_iter()
            .map(|(file_path, entity_ids)| (file_path, entity_ids, git_commit.cloned()))
            .collect();

        postgres_client
            .update_file_snapshots_batch(repo_id, &snapshot_updates)
            .await
            .storage_err("Failed to batch update file snapshots")?;

        info!(
            "Stage 5: Successfully updated {} file snapshots",
            file_count
        );

        total_snapshots += file_count;
    }

    info!("Updated {total_snapshots} file snapshots");
    Ok(total_snapshots)
}

#[async_trait]
impl crate::Indexer for RepositoryIndexer {
    /// Index the entire repository using a pipelined architecture
    async fn index_repository(&mut self) -> Result<IndexResult> {
        let start_time = Instant::now();
        let config = &self.config;

        info!(
            repository_path = %self.repository_path.display(),
            "Starting pipelined repository indexing with config: \
             index_batch_size={}, max_entity_batch_size={}, channel_buffer_size={}, \
             file_extraction_concurrency={}, snapshot_update_concurrency={}",
            config.index_batch_size,
            config.max_entity_batch_size,
            config.channel_buffer_size,
            config.file_extraction_concurrency,
            config.snapshot_update_concurrency
        );

        // Create channels with configurable buffer sizes
        let (file_tx, file_rx) = mpsc::channel::<FileBatch>(config.channel_buffer_size);
        let (entity_tx, entity_rx) = mpsc::channel::<EntityBatch>(config.channel_buffer_size);
        let (embedded_tx, embedded_rx) = mpsc::channel::<EmbeddedBatch>(config.channel_buffer_size);
        let (stored_tx, stored_rx) = mpsc::channel::<StoredBatch>(config.channel_buffer_size);

        // Clone shared state for each stage
        let repo_path = self.repository_path.clone();
        let repo_id = self.repository_id;
        let git_repo = self.git_repo.clone();
        let git_commit = get_current_commit(git_repo.as_ref(), &repo_path);
        let embedding_manager = self.embedding_manager.clone();
        let postgres_client = self.postgres_client.clone();
        let postgres_client_2 = self.postgres_client.clone();

        // Fetch collection_name once for entire pipeline
        let collection_name = postgres_client
            .get_collection_name(repo_id)
            .await
            .map_err(|e| Error::Other(anyhow!("Failed to get collection name: {e}")))?
            .ok_or_else(|| Error::Other(anyhow!("Repository not found for repo_id {repo_id}")))?;

        // Spawn all 5 stages concurrently
        let stage1 = tokio::spawn(stage_file_discovery(
            file_tx,
            repo_path,
            config.index_batch_size,
        ));

        let stage2 = tokio::spawn(stage_extract_entities(
            file_rx,
            entity_tx,
            repo_id,
            git_commit.clone(),
            collection_name.clone(),
            config.max_entity_batch_size,
            config.file_extraction_concurrency,
        ));

        let postgres_client_3 = self.postgres_client.clone();
        let stage3 = tokio::spawn(stage_generate_embeddings(
            entity_rx,
            embedded_tx,
            embedding_manager,
            postgres_client_3,
        ));

        let stage4 = tokio::spawn(stage_store_entities(
            embedded_rx,
            stored_tx,
            postgres_client,
        ));

        let stage5 = tokio::spawn(stage_update_snapshots(
            stored_rx,
            postgres_client_2,
            config.snapshot_update_concurrency,
        ));

        // Await all stages and handle errors
        let stage1_result = stage1
            .await
            .map_err(|e| Error::Other(anyhow!("Stage 1 panicked: {e}")))?;
        let stage2_result = stage2
            .await
            .map_err(|e| Error::Other(anyhow!("Stage 2 panicked: {e}")))?;
        let stage3_result = stage3
            .await
            .map_err(|e| Error::Other(anyhow!("Stage 3 panicked: {e}")))?;
        let stage4_result = stage4
            .await
            .map_err(|e| Error::Other(anyhow!("Stage 4 panicked: {e}")))?;
        let stage5_result = stage5
            .await
            .map_err(|e| Error::Other(anyhow!("Stage 5 panicked: {e}")))?;

        // Aggregate results
        let total_files = stage1_result?;
        let (entities_extracted, failed_files) = stage2_result?;
        let _entities_embedded = stage3_result?;
        let _entities_stored = stage4_result?;
        let _snapshots_updated = stage5_result?;

        // Build final statistics
        let mut stats = IndexStats::new();
        stats.set_total_files(total_files);
        stats.set_entities_extracted(entities_extracted);
        stats.set_processing_time_ms(start_time.elapsed().as_millis() as u64);

        // Track failed files from extraction stage
        for _ in 0..failed_files {
            stats.increment_failed_files();
        }

        // Note: entities_skipped_size is tracked internally by embedding stage
        // but not aggregated to final stats in pipelined version (logged instead)

        // Set last indexed commit
        let commit_hash = git_commit.unwrap_or_else(|| "indexed".to_string());
        self.postgres_client
            .set_last_indexed_commit(self.repository_id, &commit_hash)
            .await?;
        info!(commit = %commit_hash, "Updated last indexed commit");

        let total_time = start_time.elapsed();
        let throughput = if total_time.as_secs_f64() > 0.0 {
            entities_extracted as f64 / total_time.as_secs_f64()
        } else {
            0.0
        };

        info!(
            total_files = stats.total_files(),
            entities_extracted = stats.entities_extracted(),
            processing_time_s = stats.processing_time_ms() as f64 / 1000.0,
            failed_files = stats.failed_files(),
            throughput_entities_per_sec = format!("{throughput:.1}"),
            "Pipeline completed"
        );

        // No granular errors tracked in pipelined version (all logged during processing)
        Ok(IndexResult::new(stats, Vec::new()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::expect_used)]
mod tests {
    use crate::entity_processor;
    use codesearch_core::entities::{
        EntityMetadata, EntityType, Language, SourceLocation, Visibility,
    };
    use codesearch_core::CodeEntity;
    use codesearch_storage::MockPostgresClient;
    use codesearch_storage::PostgresClientTrait;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn create_test_entity(
        name: &str,
        entity_id: &str,
        file_path: &str,
        repo_id: &str,
    ) -> CodeEntity {
        CodeEntity {
            entity_id: entity_id.to_string(),
            repository_id: repo_id.to_string(),
            name: name.to_string(),
            qualified_name: name.to_string(),
            entity_type: EntityType::Function,
            language: Language::Rust,
            file_path: PathBuf::from(file_path),
            location: SourceLocation {
                start_line: 1,
                end_line: 10,
                start_column: 0,
                end_column: 10,
            },
            visibility: Visibility::Public,
            parent_scope: None,
            dependencies: Vec::new(),
            signature: None,
            documentation_summary: None,
            content: Some(format!("fn {name}() {{}}")),
            metadata: EntityMetadata::default(),
        }
    }

    #[tokio::test]
    async fn test_handle_file_change_detects_stale_entities() {
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        // Register repository with mock and get the repo UUID
        let repo_uuid = postgres
            .ensure_repository(std::path::Path::new("/test/repo"), "test_collection", None)
            .await
            .unwrap();
        let repo_id = repo_uuid.to_string();

        let file_path = "test.rs";

        // Setup: store entities in mock database
        let entity1 = create_test_entity("entity1", "entity1", file_path, &repo_id);
        let entity2 = create_test_entity("entity2", "entity2", file_path, &repo_id);
        postgres
            .store_entity_metadata(repo_uuid, &entity1, None, Uuid::new_v4())
            .await
            .unwrap();
        postgres
            .store_entity_metadata(repo_uuid, &entity2, None, Uuid::new_v4())
            .await
            .unwrap();

        // Setup: previous snapshot had two entities
        let old_entities = vec!["entity1".to_string(), "entity2".to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // New state: only entity1 remains
        let new_entities = vec!["entity1".to_string()];

        // Run update_file_snapshot_and_mark_stale
        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // Verify entity2 was marked as deleted
        let entity_ids = vec!["entity2".to_string(), "entity1".to_string()];
        let metadata_map = postgres
            .get_entities_metadata_batch(repo_uuid, &entity_ids)
            .await
            .unwrap();

        let entity2_meta = metadata_map.get("entity2").unwrap();
        assert!(entity2_meta.1.is_some()); // deleted_at is Some

        let entity1_meta = metadata_map.get("entity1").unwrap();
        assert!(entity1_meta.1.is_none()); // deleted_at is None

        // Verify snapshot was updated
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap();
        assert_eq!(snapshot, Some(new_entities));

        // Verify DELETE outbox entry was created
        use codesearch_storage::TargetStore;
        let entries = postgres
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn test_handle_file_change_detects_renamed_function() {
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        // Register repository with mock and get the repo UUID
        let repo_uuid = postgres
            .ensure_repository(std::path::Path::new("/test/repo"), "test_collection", None)
            .await
            .unwrap();
        let repo_id = repo_uuid.to_string();

        let file_path = "test.rs";

        // Setup: store old entity
        let old_entity = create_test_entity("old_name", "entity_old_name", file_path, &repo_id);
        postgres
            .store_entity_metadata(repo_uuid, &old_entity, None, Uuid::new_v4())
            .await
            .unwrap();

        // Old snapshot: function named "old_name"
        let old_entities = vec!["entity_old_name".to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // New state: function renamed to "new_name" (different entity ID)
        let new_entities = vec!["entity_new_name".to_string()];

        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // Old entity should be marked deleted
        let entity_ids = vec!["entity_old_name".to_string()];
        let metadata_map = postgres
            .get_entities_metadata_batch(repo_uuid, &entity_ids)
            .await
            .unwrap();
        let old_entity_meta = metadata_map.get("entity_old_name").unwrap();
        assert!(old_entity_meta.1.is_some()); // deleted_at is Some
    }

    #[tokio::test]
    async fn test_handle_file_change_handles_added_entities() {
        let repo_uuid = Uuid::new_v4();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let file_path = "test.rs";

        // Old snapshot: one entity
        let old_entities = vec!["entity1".to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // New state: added entity2
        let new_entities = vec!["entity1".to_string(), "entity2".to_string()];

        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // Snapshot should be updated
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap();
        assert_eq!(snapshot, Some(new_entities));

        // No DELETE outbox entries
        use codesearch_storage::TargetStore;
        let entries = postgres
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await
            .unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[tokio::test]
    async fn test_handle_file_change_empty_file() {
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        // Register repository with mock and get the repo UUID
        let repo_uuid = postgres
            .ensure_repository(std::path::Path::new("/test/repo"), "test_collection", None)
            .await
            .unwrap();
        let repo_id = repo_uuid.to_string();

        let file_path = "test.rs";

        // Setup: store entities
        for i in 1..=3 {
            let entity = create_test_entity(
                &format!("entity{i}"),
                &format!("entity{i}"),
                file_path,
                &repo_id,
            );
            postgres
                .store_entity_metadata(repo_uuid, &entity, None, Uuid::new_v4())
                .await
                .unwrap();
        }

        // Old snapshot: three entities
        let old_entities = vec![
            "entity1".to_string(),
            "entity2".to_string(),
            "entity3".to_string(),
        ];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // New state: file is now empty (all entities removed)
        let new_entities = vec![];

        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // All entities should be marked as deleted
        let entity_ids = vec![
            "entity1".to_string(),
            "entity2".to_string(),
            "entity3".to_string(),
        ];
        let metadata_map = postgres
            .get_entities_metadata_batch(repo_uuid, &entity_ids)
            .await
            .unwrap();

        let entity1_meta = metadata_map.get("entity1").unwrap();
        assert!(entity1_meta.1.is_some()); // deleted_at is Some

        let entity2_meta = metadata_map.get("entity2").unwrap();
        assert!(entity2_meta.1.is_some()); // deleted_at is Some

        let entity3_meta = metadata_map.get("entity3").unwrap();
        assert!(entity3_meta.1.is_some()); // deleted_at is Some

        // Should have 3 DELETE outbox entries
        use codesearch_storage::TargetStore;
        let entries = postgres
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await
            .unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[tokio::test]
    async fn test_handle_file_change_no_previous_snapshot() {
        let repo_uuid = Uuid::new_v4();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let file_path = "test.rs";

        // No previous snapshot
        let new_entities = vec!["entity1".to_string(), "entity2".to_string()];

        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            new_entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // No entities should be deleted (first time indexing)
        use codesearch_storage::TargetStore;
        let entries = postgres
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await
            .unwrap();
        assert_eq!(entries.len(), 0);

        // Snapshot should be created
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap();
        assert_eq!(snapshot, Some(new_entities));
    }

    #[tokio::test]
    async fn test_handle_file_change_no_changes() {
        let repo_uuid = Uuid::new_v4();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let file_path = "test.rs";

        // Old snapshot
        let entities = vec!["entity1".to_string(), "entity2".to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, entities.clone(), None)
            .await
            .unwrap();

        // Re-index with same entities
        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            entities.clone(),
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // No entities deleted
        use codesearch_storage::TargetStore;
        let entries = postgres
            .get_unprocessed_outbox_entries(TargetStore::Qdrant, 10)
            .await
            .unwrap();
        assert_eq!(entries.len(), 0);

        // Snapshot still updated (for git commit tracking)
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap();
        assert_eq!(snapshot, Some(entities));
    }

    #[tokio::test]
    async fn test_handle_file_change_writes_delete_to_outbox() {
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        // Register repository with mock and get the repo UUID
        let repo_uuid = postgres
            .ensure_repository(std::path::Path::new("/test/repo"), "test_collection", None)
            .await
            .unwrap();

        let file_path = "test.rs";

        // Setup with entities - store entity in metadata first
        let old_entity_id = "stale_entity";
        let old_entity =
            create_test_entity("stale_fn", old_entity_id, file_path, &repo_uuid.to_string());
        postgres
            .store_entity_metadata(repo_uuid, &old_entity, None, uuid::Uuid::new_v4())
            .await
            .unwrap();

        let old_entities = vec![old_entity_id.to_string()];
        postgres
            .update_file_snapshot(repo_uuid, file_path, old_entities, None)
            .await
            .unwrap();

        // Remove entity
        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            vec![],
            None,
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // Verify outbox entry
        let entries = postgres
            .get_unprocessed_outbox_entries(codesearch_storage::TargetStore::Qdrant, 10)
            .await
            .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entity_id, "stale_entity");
        assert_eq!(entries[0].operation, "DELETE");
        assert_eq!(entries[0].target_store, "qdrant");

        // Verify payload contains reason
        let payload = &entries[0].payload;
        assert_eq!(payload["reason"], "file_change");
        assert!(payload["entity_ids"].is_array());
    }

    #[tokio::test]
    async fn test_handle_file_change_updates_snapshot_with_git_commit() {
        let repo_uuid = Uuid::new_v4();
        let postgres = std::sync::Arc::new(MockPostgresClient::new());

        let file_path = "test.rs";
        let git_commit = Some("abc123".to_string());
        let new_entities = vec!["entity1".to_string()];

        entity_processor::update_file_snapshot_and_mark_stale(
            repo_uuid,
            "test_collection",
            file_path,
            new_entities.clone(),
            git_commit.clone(),
            postgres.as_ref(),
        )
        .await
        .unwrap();

        // Snapshot should be stored with git commit
        let snapshot = postgres
            .get_file_snapshot(repo_uuid, file_path)
            .await
            .unwrap()
            .expect("Snapshot should exist");
        assert_eq!(snapshot, new_entities);
    }
}
