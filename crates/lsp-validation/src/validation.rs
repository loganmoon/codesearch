//! Validation utilities for comparing Neo4j relationships against LSP ground truth
//!
//! This module provides functions for validating that our extracted relationships
//! in Neo4j match what the LSP knows about the codebase.

use crate::lsp_client::{uri_to_path, LspClient};
use crate::report::{Discrepancy, ValidationReport};
use anyhow::Result;
use codesearch_core::entities::{Language, ReferenceType, SourceLocation};
use codesearch_core::CodeEntity;
use lsp_types::Location;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, warn};

/// A relationship edge from Neo4j
#[derive(Debug, Clone)]
pub struct Neo4jEdge {
    /// Entity ID of the source (referencing entity)
    pub from_entity_id: String,
    /// Entity ID of the target (referenced entity)
    pub to_entity_id: String,
    /// Type of relationship (CALLS, USES, IMPORTS, etc.)
    pub rel_type: String,
}

/// Result of validating relationships against LSP
#[derive(Debug)]
pub struct ValidationResult {
    /// Entity was correctly validated against LSP
    pub is_match: bool,
    /// Details about the validation
    pub details: String,
}

/// Engine for validating Neo4j relationships against LSP
pub struct ValidationEngine {
    lsp: LspClient,
    language: Language,
    workspace_root: PathBuf,
    /// Map of entity_id -> CodeEntity
    entity_map: HashMap<String, CodeEntity>,
}

impl ValidationEngine {
    /// Create a new validation engine
    pub fn new(
        lsp: LspClient,
        language: Language,
        workspace_root: PathBuf,
        entities: Vec<CodeEntity>,
    ) -> Self {
        let mut entity_map = HashMap::new();

        for entity in entities {
            entity_map.insert(entity.entity_id.clone(), entity);
        }

        Self {
            lsp,
            language,
            workspace_root,
            entity_map,
        }
    }

    /// Validate Neo4j relationships against LSP ground truth
    ///
    /// For each target entity that has incoming edges:
    /// 1. Query LSP find_references at the target's definition location
    /// 2. For each LSP reference location, find which of our entities contains it
    /// 3. Check if we have an edge from that entity to the target
    ///
    /// Precision: Of our edges, how many does LSP confirm?
    /// Recall: Of LSP's references, how many do we have edges for?
    pub fn validate_relationships(
        &mut self,
        edges: &[Neo4jEdge],
        codebase_name: &str,
    ) -> Result<ValidationReport> {
        let start = Instant::now();
        let mut report = ValidationReport::new(codebase_name, self.language);

        // Group edges by target entity
        let mut edges_by_target: HashMap<&str, Vec<&Neo4jEdge>> = HashMap::new();
        for edge in edges {
            edges_by_target
                .entry(&edge.to_entity_id)
                .or_default()
                .push(edge);
        }

        // Collect target IDs to iterate (avoid borrow issues)
        let target_ids: Vec<String> = edges_by_target.keys().map(|s| (*s).to_string()).collect();

        // For each target entity with incoming edges
        for target_id in &target_ids {
            // Extract target info upfront to avoid borrow conflicts
            let (target_file, target_line, target_col, target_qualified_name, is_external) = {
                let target = match self.entity_map.get(target_id) {
                    Some(e) => e,
                    None => {
                        debug!("Target entity not found: {target_id}");
                        continue;
                    }
                };
                (
                    target.file_path.clone(),
                    target.location.start_line.saturating_sub(1) as u32,
                    target.location.start_column as u32,
                    target.qualified_name.clone(),
                    self.is_external_entity(target),
                )
            };

            // Query LSP for references to this target (needs &mut self)
            let lsp_refs =
                self.lsp
                    .find_references(&target_file, target_line, target_col, false)?;

            // Filter to workspace files only
            let workspace_refs: Vec<_> = lsp_refs
                .iter()
                .filter(|loc| self.is_in_workspace(loc))
                .collect();

            // Track which edges we've confirmed
            let mut confirmed_edges: HashSet<String> = HashSet::new();
            let mut lsp_ref_count = 0;

            // Get edges for this target
            let target_edges = edges_by_target
                .get(target_id.as_str())
                .cloned()
                .unwrap_or_default();

            // For each LSP reference, find the containing entity
            for ref_loc in &workspace_refs {
                let ref_path = match uri_to_path(&ref_loc.uri) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let ref_line = ref_loc.range.start.line as usize + 1; // LSP is 0-indexed

                // Find which of our entities contains this reference location
                if let Some(source_entity) = self.find_entity_containing(&ref_path, ref_line) {
                    lsp_ref_count += 1;

                    // Check if we have an edge from this source to the target
                    let have_edge = target_edges
                        .iter()
                        .any(|e| e.from_entity_id == source_entity.entity_id);

                    if have_edge {
                        confirmed_edges.insert(source_entity.entity_id.clone());
                    } else {
                        // LSP found a reference we missed (false negative)
                        let ref_type = self.infer_reference_type(&target_edges[0].rel_type);
                        let metrics = report.metrics.entry(ref_type).or_default();
                        metrics.false_negatives += 1;

                        report.discrepancies.push(Discrepancy {
                            file: ref_path.clone(),
                            location: SourceLocation {
                                start_line: ref_line,
                                end_line: ref_line,
                                start_column: ref_loc.range.start.character as usize,
                                end_column: ref_loc.range.end.character as usize,
                            },
                            ref_type,
                            source_text: source_entity.qualified_name.clone(),
                            our_target: None,
                            lsp_target: Some(target_qualified_name.clone()),
                            reason: format!(
                                "LSP found reference from {} to {}, but we have no edge",
                                source_entity.qualified_name, target_qualified_name
                            ),
                        });
                    }
                }
            }

            // Check precision: for our edges, are they confirmed by LSP?
            for edge in &target_edges {
                let ref_type = self.infer_reference_type(&edge.rel_type);
                let metrics = report.metrics.entry(ref_type).or_default();

                if confirmed_edges.contains(&edge.from_entity_id) {
                    metrics.true_positives += 1;
                } else {
                    // We have an edge LSP didn't confirm
                    let source = self.entity_map.get(&edge.from_entity_id);

                    if is_external {
                        metrics.external_refs += 1;
                    } else if lsp_ref_count == 0 {
                        // LSP returned no references at all - might be LSP issue
                        metrics.lsp_errors += 1;
                        warn!(
                            "LSP returned no references for target: {}",
                            target_qualified_name
                        );
                    } else {
                        // We have an edge LSP doesn't recognize (false positive)
                        metrics.false_positives += 1;

                        if let Some(source) = source {
                            report.discrepancies.push(Discrepancy {
                                file: source.file_path.clone(),
                                location: source.location.clone(),
                                ref_type,
                                source_text: source.qualified_name.clone(),
                                our_target: Some(target_qualified_name.clone()),
                                lsp_target: None,
                                reason: format!(
                                    "We have edge {} -> {}, but LSP doesn't confirm it",
                                    source.qualified_name, target_qualified_name
                                ),
                            });
                        }
                    }
                }
            }
        }

        report.duration_secs = start.elapsed().as_secs_f64();
        Ok(report)
    }

    /// Check if a location is within the workspace (not in node_modules, etc.)
    fn is_in_workspace(&self, location: &Location) -> bool {
        let path = match uri_to_path(&location.uri) {
            Ok(p) => p,
            Err(_) => return false,
        };

        // Check if path is under workspace root
        if !path.starts_with(&self.workspace_root) {
            return false;
        }

        // Exclude common external directories
        let path_str = path.to_string_lossy();
        let excluded = ["node_modules", ".git", "__pycache__", "target", "dist"];
        !excluded.iter().any(|ex| path_str.contains(ex))
    }

    /// Find the entity that contains a given file:line location
    fn find_entity_containing(&self, file_path: &Path, line: usize) -> Option<&CodeEntity> {
        self.entity_map.values().find(|e| {
            paths_equivalent(&e.file_path, file_path)
                && e.location.start_line <= line
                && e.location.end_line >= line
        })
    }

    /// Check if an entity is external (stdlib, dependencies)
    fn is_external_entity(&self, entity: &CodeEntity) -> bool {
        let external_prefixes = [
            "external::",
            "external.",
            "std::",
            "core::",
            "typing.",
            "collections.",
        ];
        external_prefixes
            .iter()
            .any(|p| entity.qualified_name.starts_with(p))
    }

    /// Infer ReferenceType from Neo4j relationship type string
    fn infer_reference_type(&self, rel_type: &str) -> ReferenceType {
        match rel_type.to_uppercase().as_str() {
            "CALLS" | "CALLED_BY" => ReferenceType::Call,
            "USES" | "USED_BY" => ReferenceType::Uses,
            "IMPORTS" | "IMPORTED_BY" => ReferenceType::Import,
            "INHERITS_FROM" | "HAS_SUBCLASS" | "EXTENDS_INTERFACE" | "EXTENDED_BY" => {
                ReferenceType::Extends
            }
            _ => ReferenceType::Uses,
        }
    }

    /// Shutdown the LSP client
    pub fn shutdown(self) -> Result<()> {
        self.lsp.shutdown()
    }
}

/// Check if two paths are equivalent (handling symlinks, etc.)
fn paths_equivalent(a: &Path, b: &Path) -> bool {
    // Try canonical paths first
    if let (Ok(ca), Ok(cb)) = (a.canonicalize(), b.canonicalize()) {
        return ca == cb;
    }
    // Fall back to direct comparison
    a == b
}
