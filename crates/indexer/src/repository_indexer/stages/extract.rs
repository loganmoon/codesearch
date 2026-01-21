//! Stage 2: Entity extraction
//!
//! Extracts code entities from files in parallel and creates entity batches for embedding.

use crate::common::is_js_ts_file;
use crate::entity_processor;
use crate::repository_indexer::batches::{EntityBatch, FileBatch};
use anyhow::anyhow;
use codesearch_core::entities::{
    CodeEntityBuilder, EntityRelationshipData, EntityType, Language, ReferenceType, SourceLocation,
    SourceReference, Visibility,
};
use codesearch_core::error::{Error, Result};
use codesearch_core::project_manifest::PackageMap;
use codesearch_core::QualifiedName;
use codesearch_languages::common::language_path::LanguagePath;
use codesearch_languages::common::path_config::RUST_PATH_CONFIG;
use futures::stream::{self, StreamExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Create crate root module entities from the project manifest
///
/// This creates a Module entity for each crate in the project.
/// The crate root is the top-level module that contains all other modules.
/// Uses the `CrateInfo` from the manifest when available (Rust projects),
/// otherwise falls back to file-based discovery for non-Rust projects.
pub(crate) fn create_crate_root_entities(
    package_map: &PackageMap,
    repo_id: &str,
) -> Vec<codesearch_core::CodeEntity> {
    package_map
        .iter()
        .flat_map(|(_pkg_dir, info)| {
            // Use crate info from manifest when available (Rust projects)
            if !info.crates.is_empty() {
                info.crates
                    .iter()
                    .filter_map(|crate_info| {
                        // Only create entity if the entry file exists
                        if !crate_info.entry_path.exists() {
                            debug!(
                                "Crate entry file {} does not exist for crate {}",
                                crate_info.entry_path.display(),
                                crate_info.name
                            );
                            return None;
                        }

                        let entity_id = uuid::Uuid::new_v4().to_string();

                        // Extract file-level imports for IMPORTS relationships
                        let imports = extract_file_level_imports(&crate_info.entry_path);
                        let relationships = EntityRelationshipData {
                            imports,
                            ..Default::default()
                        };

                        let qn = match QualifiedName::parse(&crate_info.name) {
                            Ok(qn) => qn,
                            Err(e) => {
                                warn!(
                                    "Failed to parse qualified name for {}: {}",
                                    crate_info.name, e
                                );
                                return None;
                            }
                        };
                        match CodeEntityBuilder::default()
                            .entity_id(entity_id)
                            .repository_id(repo_id.to_string())
                            .name(crate_info.name.clone())
                            .qualified_name(qn)
                            .entity_type(EntityType::Module)
                            .file_path(crate_info.entry_path.clone())
                            .location(SourceLocation {
                                start_line: 1,
                                start_column: 1,
                                end_line: 1,
                                end_column: 1,
                            })
                            .visibility(Some(Visibility::Public))
                            .language(Language::Rust)
                            .relationships(relationships)
                            .build()
                        {
                            Ok(entity) => Some(entity),
                            Err(e) => {
                                warn!(
                                    "Failed to build crate root entity for {}: {}",
                                    crate_info.name, e
                                );
                                None
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                // Fallback for non-Rust projects: check for lib.rs or main.rs
                let lib_rs = info.source_root.join("lib.rs");
                let main_rs = info.source_root.join("main.rs");
                let crate_root_file = if lib_rs.exists() {
                    lib_rs
                } else if main_rs.exists() {
                    main_rs
                } else {
                    debug!(
                        "No lib.rs or main.rs found in {} for package {}",
                        info.source_root.display(),
                        info.name
                    );
                    return vec![];
                };

                let entity_id = uuid::Uuid::new_v4().to_string();

                // Extract file-level imports for IMPORTS relationships
                let imports = extract_file_level_imports(&crate_root_file);
                let relationships = EntityRelationshipData {
                    imports,
                    ..Default::default()
                };

                let qn = match QualifiedName::parse(&info.name) {
                    Ok(qn) => qn,
                    Err(e) => {
                        warn!("Failed to parse qualified name for {}: {}", info.name, e);
                        return vec![];
                    }
                };
                match CodeEntityBuilder::default()
                    .entity_id(entity_id)
                    .repository_id(repo_id.to_string())
                    .name(info.name.clone())
                    .qualified_name(qn)
                    .entity_type(EntityType::Module)
                    .file_path(crate_root_file)
                    .location(SourceLocation {
                        start_line: 1,
                        start_column: 1,
                        end_line: 1,
                        end_column: 1,
                    })
                    .visibility(Some(Visibility::Public))
                    .language(Language::Rust)
                    .relationships(relationships)
                    .build()
                {
                    Ok(entity) => vec![entity],
                    Err(e) => {
                        warn!("Failed to build crate root entity for {}: {}", info.name, e);
                        vec![]
                    }
                }
            }
        })
        .collect()
}

/// Extract file-level use declarations from a Rust source file
///
/// This extracts imports that are direct children of the source_file (not nested in mod blocks).
/// Uses tree-sitter queries to properly parse the file structure.
pub(crate) fn extract_file_level_imports(file_path: &Path) -> Vec<SourceReference> {
    // Read file content
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            debug!(
                "Failed to read file for import extraction {:?}: {}",
                file_path, e
            );
            return Vec::new();
        }
    };

    // Parse with tree-sitter
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .is_err()
    {
        debug!("Failed to set parser language for {:?}", file_path);
        return Vec::new();
    }

    let tree = match parser.parse(&content, None) {
        Some(t) => t,
        None => {
            debug!("Failed to parse file for import extraction {:?}", file_path);
            return Vec::new();
        }
    };

    let root = tree.root_node();
    let mut imports = Vec::new();

    // Iterate through direct children of source_file looking for use_declaration
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "use_declaration" {
            // Extract the use path from the declaration
            if let Some(argument) = child.child_by_field_name("argument") {
                let import_path = &content[argument.byte_range()];
                if import_path.is_empty() {
                    continue;
                }

                // Use LanguagePath for proper parsing - encapsulates all path logic
                let lang_path = LanguagePath::parse(import_path, &RUST_PATH_CONFIG);

                // Extract simple name using LanguagePath methods
                let simple_name = lang_path
                    .simple_name()
                    .unwrap_or_else(|| {
                        lang_path
                            .segments()
                            .first()
                            .map(String::as_str)
                            .unwrap_or("")
                    })
                    .to_string();

                // Skip if simple_name is empty (shouldn't happen with valid imports)
                if simple_name.is_empty() {
                    continue;
                }

                // Determine if external: relative paths (crate::, self::, super::) are internal
                let is_external = !lang_path.is_relative();

                let location = SourceLocation {
                    start_line: child.start_position().row + 1,
                    end_line: child.end_position().row + 1,
                    start_column: child.start_position().column,
                    end_column: child.end_position().column,
                };

                // Use lang_path.to_qualified_name() for consistency with LanguagePath parsing
                if let Ok(source_ref) = SourceReference::builder()
                    .target(lang_path.to_qualified_name())
                    .simple_name(simple_name)
                    .is_external(is_external)
                    .location(location)
                    .ref_type(ReferenceType::Import)
                    .build()
                {
                    imports.push(source_ref);
                }
            }
        }
    }

    imports
}

/// Stage 2: Extract entities from files in parallel
#[allow(clippy::too_many_arguments)]
pub(crate) async fn stage_extract_entities(
    mut file_rx: mpsc::Receiver<FileBatch>,
    entity_tx: mpsc::Sender<EntityBatch>,
    repo_id: Uuid,
    git_commit: Option<String>,
    collection_name: String,
    max_entity_batch_size: usize,
    file_extraction_concurrency: usize,
    package_map: Option<Arc<PackageMap>>,
    repo_path: PathBuf,
) -> Result<(usize, usize)> {
    let mut total_extracted = 0;
    let mut total_failed = 0;

    // Create crate root entities from manifest before processing files
    if let Some(ref pm) = package_map {
        let repo_id_str = repo_id.to_string();
        let crate_roots = create_crate_root_entities(pm, &repo_id_str);
        if !crate_roots.is_empty() {
            let count = crate_roots.len();
            info!("Stage 2: Creating {} crate root module entities", count);

            entity_tx
                .send(EntityBatch {
                    entities: crate_roots,
                    file_indices: Vec::new(), // Crate roots don't come from file extraction
                    repo_id,
                    git_commit: git_commit.clone(),
                    collection_name: collection_name.clone(),
                })
                .await
                .map_err(|_| Error::Other(anyhow!("Entity channel closed")))?;

            total_extracted += count;
        }
    }

    // Accumulator for building entity batches
    let mut entities = Vec::new();
    let mut file_indices = Vec::new();
    let mut batch_failed = 0;

    while let Some(FileBatch { paths }) = file_rx.recv().await {
        debug!("Extracting entities from {} files", paths.len());

        // Convert repo_id once for the entire batch
        let repo_id_str = repo_id.to_string();
        let package_map_ref = &package_map;

        // Process files in parallel (8 concurrent extractions), collect results
        let repo_path_ref = &repo_path;
        let results = stream::iter(paths.into_iter())
            .map(|path| {
                let repo_id_ref = &repo_id_str;
                async move {
                    // Look up package context for this file
                    let (package_name, source_root) = package_map_ref
                        .as_ref()
                        .and_then(|pm| pm.find_package_for_file(&path))
                        .map(|pkg| (Some(pkg.name.as_str()), Some(pkg.source_root.as_path())))
                        .unwrap_or((None, None));

                    // For JS/TS files, don't include package name in qualified names
                    // (unlike Rust where crate names are part of the FQN)
                    let package_name = if is_js_ts_file(&path) {
                        None
                    } else {
                        package_name
                    };

                    match entity_processor::extract_entities_from_file(
                        &path,
                        repo_id_ref,
                        package_name,
                        source_root,
                        repo_path_ref,
                    )
                    .await
                    {
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
