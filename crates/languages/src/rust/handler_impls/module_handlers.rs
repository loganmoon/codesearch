//! Handler for extracting Rust module definitions
//!
//! This module processes tree-sitter query matches for Rust module
//! declarations and builds EntityData instances.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use crate::common::language_path::LanguagePath;
use crate::common::path_config::RUST_PATH_CONFIG;
use crate::rust::handler_impls::common::{
    extract_preceding_doc_comments, extract_visibility, find_capture_node, node_to_text,
    require_capture_node,
};
use crate::rust::handler_impls::constants::capture_names;
use codesearch_core::entities::{
    EntityMetadata, EntityRelationshipData, EntityType, Language, ReferenceType, SourceLocation,
    SourceReference,
};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Node, Query, QueryMatch};

/// Extract use statements from a module node
fn extract_use_statements(node: Node, source: &str) -> Vec<SourceReference> {
    let mut imports = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "use_declaration" {
            if let Ok(import_text) = node_to_text(child, source) {
                // Parse "use std::collections::HashMap;" -> "std::collections::HashMap"
                let import_path = import_text
                    .trim_start_matches("use ")
                    .trim_end_matches(';')
                    .trim();

                // Skip empty imports
                if import_path.is_empty() {
                    continue;
                }

                // Use LanguagePath for proper parsing - encapsulates path logic
                let lang_path = LanguagePath::parse(import_path, &RUST_PATH_CONFIG);

                // Extract simple name using LanguagePath
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

                let location = SourceLocation::from_tree_sitter_node(child);

                // Use lang_path.to_qualified_name() to ensure consistency with LanguagePath parsing
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

/// Process a module query match and extract entity data
#[allow(clippy::too_many_arguments)]
pub fn handle_module_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    // Get the module node for location and content
    let module_node = require_capture_node(query_match, query, "module")?;

    // Create extraction context
    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
        repo_root,
    };

    // Extract common components
    let components = extract_common_components(&ctx, capture_names::NAME, module_node, "rust")?;

    // Extract Rust-specific: visibility, documentation, content
    let visibility = extract_visibility(query_match, query);
    let documentation = extract_preceding_doc_comments(module_node, source);
    let content = node_to_text(module_node, source).ok();

    // Check if this is an inline module (has body) or file module
    let has_body = find_capture_node(query_match, query, "mod_body").is_some();

    // Extract imports from the module
    let imports = extract_use_statements(module_node, source);

    // Build metadata
    let mut metadata = EntityMetadata::default();

    // Store whether this is an inline or file module
    metadata.attributes.insert(
        "is_inline".to_string(),
        if has_body { "true" } else { "false" }.to_string(),
    );

    // Build typed relationships
    let relationships = EntityRelationshipData {
        imports: imports.clone(),
        ..Default::default()
    };

    // Build the entity using the shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Module,
            language: Language::Rust,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships,
        },
    )?;

    Ok(vec![entity])
}
