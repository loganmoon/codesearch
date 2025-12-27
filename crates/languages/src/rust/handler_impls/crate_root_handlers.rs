//! Handler for extracting the Rust crate root module
//!
//! This module creates a synthetic Module entity for the crate root when
//! processing lib.rs or main.rs files. This entity represents the implicit
//! root module of the crate that has no explicit `mod` declaration.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::module_utils::derive_path_entity_identifier;
use crate::rust::handler_impls::common::require_capture_node;
use crate::rust::module_path::is_crate_root;
use codesearch_core::entities::{
    CodeEntityBuilder, EntityMetadata, EntityType, Language, SourceLocation, Visibility,
};
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Process the source_file node to create a crate root module entity
///
/// This handler is called for every Rust file, but only creates an entity
/// when the file is a crate root (lib.rs or main.rs).
#[allow(clippy::too_many_arguments)]
pub fn handle_crate_root_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    _source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Only create an entity for crate root files
    if !is_crate_root(file_path) {
        return Ok(Vec::new());
    }

    // Need a package name to create the crate root module
    let Some(crate_name) = package_name else {
        return Ok(Vec::new());
    };

    // Get the source_file node for location
    let source_file_node = require_capture_node(query_match, query, "crate_root")?;

    // The crate name is both the name and qualified_name
    let name = crate_name.to_string();
    let qualified_name = crate_name.to_string();

    // Generate path_entity_identifier
    let path_module = derive_path_entity_identifier(file_path, repo_root, "::");
    // For the crate root, the path identifier is just the path module (no additional scope)
    let path_entity_identifier = if path_module.is_empty() {
        name.clone()
    } else {
        path_module
    };

    // Generate entity_id
    let file_path_str = file_path.to_string_lossy();
    let entity_id = generate_entity_id(repository_id, &file_path_str, &qualified_name);

    // Get location from source_file node
    let location = SourceLocation::from_tree_sitter_node(source_file_node);

    // Extract documentation from the file's leading comments
    // Look for doc comments at the very start of the file (crate-level docs)
    let documentation = extract_crate_level_docs(source_file_node, source);

    // Build the entity
    let entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name)
        .qualified_name(qualified_name)
        .path_entity_identifier(Some(path_entity_identifier))
        .parent_scope(None) // Crate root has no parent
        .entity_type(EntityType::Module)
        .location(location)
        .visibility(Visibility::Public) // Crate root is always public
        .documentation_summary(documentation)
        .content(None) // Don't store entire file content
        .metadata(EntityMetadata::default())
        .signature(None)
        .language(Language::Rust)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| {
            codesearch_core::error::Error::entity_extraction(format!(
                "Failed to build crate root entity: {e}"
            ))
        })?;

    Ok(vec![entity])
}

/// Extract crate-level documentation from the start of a file
///
/// Crate-level docs use //! or /*! comments at the start of the file.
fn extract_crate_level_docs(source_file: tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = source_file.walk();
    let mut doc_lines = Vec::new();

    for child in source_file.children(&mut cursor) {
        match child.kind() {
            "inner_line_doc_comment" | "inner_block_doc_comment" => {
                if let Ok(text) = child.utf8_text(source.as_bytes()) {
                    // Strip the //! or /*! prefix
                    let content = if text.starts_with("//!") {
                        text.strip_prefix("//!").unwrap_or(text).trim()
                    } else if text.starts_with("/*!") {
                        text.strip_prefix("/*!")
                            .and_then(|s| s.strip_suffix("*/"))
                            .unwrap_or(text)
                            .trim()
                    } else {
                        text.trim()
                    };
                    if !content.is_empty() {
                        doc_lines.push(content.to_string());
                    }
                }
            }
            // Stop at first non-doc-comment item
            _ if child.kind() != "line_comment" && child.kind() != "block_comment" => break,
            _ => {}
        }
    }

    if doc_lines.is_empty() {
        None
    } else {
        Some(doc_lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_crate_root_detection() {
        assert!(is_crate_root(Path::new("/project/src/lib.rs")));
        assert!(is_crate_root(Path::new("/project/src/main.rs")));
        assert!(!is_crate_root(Path::new("/project/src/foo.rs")));
        assert!(!is_crate_root(Path::new("/project/src/foo/mod.rs")));
    }
}
