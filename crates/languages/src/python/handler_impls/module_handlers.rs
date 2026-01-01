//! Handler for extracting Python module definitions
//!
//! This module processes tree-sitter query matches for Python module nodes
//! and builds Module entities with import tracking.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::{
    entity_building::{build_entity, CommonEntityComponents, EntityDetails},
    module_utils::{derive_module_name, derive_qualified_name},
    node_to_text, require_capture_node,
};
use codesearch_core::{
    entities::{EntityMetadata, EntityType, Language, SourceLocation, Visibility},
    entity_id::generate_entity_id,
    error::Result,
    CodeEntity,
};
use std::path::Path;
use std::sync::OnceLock;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor, QueryMatch};

// Cached tree-sitter query for import extraction
static PYTHON_IMPORT_QUERY: OnceLock<Option<Query>> = OnceLock::new();

const PYTHON_IMPORT_QUERY_SOURCE: &str = r#"
    (import_statement
      name: (dotted_name) @import_name)

    (import_from_statement
      module_name: (dotted_name) @from_module)

    (import_from_statement
      module_name: (relative_import) @relative_module)
"#;

/// Get or initialize the cached import query
fn python_import_query() -> Option<&'static Query> {
    PYTHON_IMPORT_QUERY
        .get_or_init(|| {
            let language = tree_sitter_python::LANGUAGE.into();
            Query::new(&language, PYTHON_IMPORT_QUERY_SOURCE).ok()
        })
        .as_ref()
}

/// Extract import module paths from a Python module node
///
/// Handles:
/// - `import os` -> "os"
/// - `import os.path` -> "os.path"
/// - `from os import path` -> "os"
/// - `from os.path import join` -> "os.path"
fn extract_import_sources(module_node: Node, source: &str) -> Vec<String> {
    let Some(query) = python_import_query() else {
        return Vec::new();
    };

    let mut imports = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, module_node, source.as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            if let Ok(import_path) = capture.node.utf8_text(source.as_bytes()) {
                let import_path = import_path.trim();
                if !import_path.is_empty() && !imports.contains(&import_path.to_string()) {
                    imports.push(import_path.to_string());
                }
            }
        }
    }

    imports
}

/// Handle Python module node as a Module entity
#[allow(clippy::too_many_arguments)]
pub fn handle_module_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let module_node = require_capture_node(query_match, query, "module")?;

    // Extract module name from file path
    let name = derive_module_name(file_path);

    // Build qualified name from file path
    let qualified_name = derive_qualified_name(file_path, source_root, repo_root, ".");

    // Build path_entity_identifier (repo-relative path for import resolution)
    let path_entity_identifier =
        crate::common::module_utils::derive_path_entity_identifier(file_path, repo_root, ".");

    // Generate entity ID
    let file_path_str = file_path.to_string_lossy();
    let entity_id = generate_entity_id(repository_id, &file_path_str, &qualified_name);

    // Get location
    let location = SourceLocation::from_tree_sitter_node(module_node);

    // Create components
    let components = CommonEntityComponents {
        entity_id,
        repository_id: repository_id.to_string(),
        name,
        qualified_name,
        path_entity_identifier: Some(path_entity_identifier),
        parent_scope: None,
        file_path: file_path.to_path_buf(),
        location,
    };

    // Extract imports
    let imports = extract_import_sources(module_node, source);

    // Only create a Module entity if there are imports to track
    // Module entities exist to establish IMPORTS relationships
    if imports.is_empty() {
        return Ok(vec![]);
    }

    // Build metadata
    let mut metadata = EntityMetadata::default();

    // Store imports as JSON array (used by imports_resolver)
    if let Ok(imports_json) = serde_json::to_string(&imports) {
        metadata
            .attributes
            .insert("imports".to_string(), imports_json);
    }

    // Build the entity
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Module,
            language: Language::Python,
            visibility: Some(Visibility::Public),
            documentation: None,
            content: node_to_text(module_node, source).ok(),
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}
