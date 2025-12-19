//! Handler for extracting JavaScript module definitions
//!
//! This module processes tree-sitter query matches for JavaScript program nodes
//! and builds Module entities with import tracking.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::{
    entity_building::{build_entity, CommonEntityComponents, EntityDetails},
    node_to_text, require_capture_node,
};
use codesearch_core::{
    entities::{EntityMetadata, EntityType, Language, SourceLocation, Visibility},
    entity_id::generate_entity_id,
    error::Result,
    CodeEntity,
};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor, QueryMatch};

/// Extract import source paths from a JavaScript/TypeScript program node
fn extract_import_sources(program_node: Node, source: &str) -> Vec<String> {
    let mut imports = Vec::new();

    // Query for import statements
    let query_source = r#"
        (import_statement
          source: (string) @source)
    "#;

    let language = tree_sitter_javascript::LANGUAGE.into();
    let query = match Query::new(&language, query_source) {
        Ok(q) => q,
        Err(_) => return imports,
    };

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, program_node, source.as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            if let Ok(source_path) = capture.node.utf8_text(source.as_bytes()) {
                // Remove quotes from source path
                let source_path = source_path.trim_matches(|c| c == '"' || c == '\'');
                if !source_path.is_empty() {
                    imports.push(source_path.to_string());
                }
            }
        }
    }

    imports
}

/// Derive module name from file path
///
/// For JavaScript, the module name is the file name without extension
/// e.g., "/src/utils/helpers.js" -> "helpers"
fn derive_module_name(file_path: &Path) -> String {
    file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string()
}

/// Derive qualified name for the module
///
/// Uses the file path relative to source root to build the qualified name
/// e.g., "/src/utils/helpers.js" relative to "/src" -> "utils.helpers"
fn derive_qualified_name(file_path: &Path, source_root: Option<&Path>, separator: &str) -> String {
    let relative = source_root
        .and_then(|root| file_path.strip_prefix(root).ok())
        .unwrap_or(file_path);

    let mut parts: Vec<&str> = Vec::new();

    for component in relative.components() {
        if let std::path::Component::Normal(s) = component {
            if let Some(s) = s.to_str() {
                // Skip file extension for the last component
                let name = if relative.extension().is_some()
                    && relative.file_name() == Some(std::ffi::OsStr::new(s))
                {
                    s.rsplit('.').next_back().unwrap_or(s)
                } else {
                    s
                };
                parts.push(name);
            }
        }
    }

    parts.join(separator)
}

/// Handle JavaScript program node as a Module entity
pub fn handle_module_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    _package_name: Option<&str>,
    source_root: Option<&Path>,
) -> Result<Vec<CodeEntity>> {
    let program_node = require_capture_node(query_match, query, "module")?;

    // Extract module name from file path
    let name = derive_module_name(file_path);

    // Build qualified name from file path
    let qualified_name = derive_qualified_name(file_path, source_root, ".");

    // Generate entity ID
    let file_path_str = file_path.to_string_lossy();
    let entity_id = generate_entity_id(repository_id, &file_path_str, &qualified_name);

    // Get location
    let location = SourceLocation::from_tree_sitter_node(program_node);

    // Create components
    let components = CommonEntityComponents {
        entity_id,
        repository_id: repository_id.to_string(),
        name,
        qualified_name,
        parent_scope: None,
        file_path: file_path.to_path_buf(),
        location,
    };

    // Extract imports
    let imports = extract_import_sources(program_node, source);

    // Only create a Module entity if there are imports to track
    // Module entities exist to establish IMPORTS relationships
    if imports.is_empty() {
        return Ok(vec![]);
    }

    // Build metadata
    let mut metadata = EntityMetadata::default();

    // Store imports as JSON array (expected by ImportsResolver)
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
            language: Language::JavaScript,
            visibility: Visibility::Public,
            documentation: None,
            content: node_to_text(program_node, source).ok(),
            metadata,
            signature: None,
        },
    )?;

    Ok(vec![entity])
}
