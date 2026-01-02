//! Handler for extracting TypeScript module definitions
//!
//! This module processes tree-sitter query matches for TypeScript program nodes
//! and builds Module entities with import tracking.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::{
    entity_building::{build_entity, CommonEntityComponents, EntityDetails},
    module_utils::derive_module_name,
    node_to_text, require_capture_node,
};
use crate::javascript::module_path::derive_module_path;
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
static TS_IMPORT_QUERY: OnceLock<Option<Query>> = OnceLock::new();
// Cached tree-sitter query for export detection
static TS_EXPORT_QUERY: OnceLock<Option<Query>> = OnceLock::new();

const TS_IMPORT_QUERY_SOURCE: &str = r#"
    (import_statement
      source: (string) @source)
"#;

const TS_EXPORT_QUERY_SOURCE: &str = r#"
    (export_statement) @export
"#;

/// Get or initialize the cached import query
fn ts_import_query() -> Option<&'static Query> {
    TS_IMPORT_QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            match Query::new(&language, TS_IMPORT_QUERY_SOURCE) {
                Ok(query) => Some(query),
                Err(e) => {
                    tracing::error!(
                        "Failed to compile TypeScript import query: {e}. This is a bug."
                    );
                    None
                }
            }
        })
        .as_ref()
}

/// Get or initialize the cached export query
fn ts_export_query() -> Option<&'static Query> {
    TS_EXPORT_QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            match Query::new(&language, TS_EXPORT_QUERY_SOURCE) {
                Ok(query) => Some(query),
                Err(e) => {
                    tracing::error!(
                        "Failed to compile TypeScript export query: {e}. This is a bug."
                    );
                    None
                }
            }
        })
        .as_ref()
}

/// Extract import source paths from a TypeScript program node
fn extract_import_sources(program_node: Node, source: &str) -> Vec<String> {
    let Some(query) = ts_import_query() else {
        return Vec::new();
    };

    let mut imports = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, program_node, source.as_bytes());

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

/// Check if a TypeScript program node has any export statements
fn has_exports(program_node: Node, source: &str) -> bool {
    let Some(query) = ts_export_query() else {
        return false;
    };

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, program_node, source.as_bytes());

    // If there's at least one export, return true
    matches.next().is_some()
}

/// Handle TypeScript program node as a Module entity
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
    let program_node = require_capture_node(query_match, query, "module")?;

    // Extract module name from file path
    let name = derive_module_name(file_path);

    // Build qualified name from file path using derive_module_path
    // This correctly handles index.ts files as directory modules (models/index.ts -> "models")
    let qualified_name = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root))
        .unwrap_or_else(|| name.clone());

    // Build path_entity_identifier (repo-relative path for import resolution)
    let path_entity_identifier =
        crate::common::module_utils::derive_path_entity_identifier(file_path, repo_root, ".");

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
        path_entity_identifier: Some(path_entity_identifier),
        parent_scope: None,
        file_path: file_path.to_path_buf(),
        location,
    };

    // Extract imports
    let imports = extract_import_sources(program_node, source);

    // Check for exports
    let has_export_statements = has_exports(program_node, source);

    // Only create a Module entity if there are imports or exports
    // Per E-MOD-FILE: "A file with import/export statements produces a Module entity"
    if imports.is_empty() && !has_export_statements {
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
            language: Language::TypeScript,
            visibility: Some(Visibility::Public),
            documentation: None,
            content: node_to_text(program_node, source).ok(),
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Handle TypeScript namespace declaration as a Module entity
/// NOTE: Currently disabled - causes timeout issues
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub fn handle_namespace_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let namespace_node = require_capture_node(query_match, query, "namespace")?;
    let name_node = require_capture_node(query_match, query, "name")?;

    // Extract namespace name
    let name = node_to_text(name_node, source)?;

    // Derive module path from file
    let module_path = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root));

    // Build qualified name from AST (includes parent namespace scope)
    let scope_result =
        crate::qualified_name::build_qualified_name_from_ast(namespace_node, source, "typescript");

    // Compose full qualified name: module.parent_namespace.name
    let full_qualified_name = match (&module_path, scope_result.parent_scope.is_empty()) {
        (Some(module), false) => format!("{module}.{}.{name}", scope_result.parent_scope),
        (Some(module), true) => format!("{module}.{name}"),
        (None, false) => format!("{}.{name}", scope_result.parent_scope),
        (None, true) => name.clone(),
    };

    // Parent scope includes module path
    let parent_scope = match (&module_path, scope_result.parent_scope.is_empty()) {
        (Some(module), false) => Some(format!("{module}.{}", scope_result.parent_scope)),
        (Some(module), true) => Some(module.clone()),
        (None, false) => Some(scope_result.parent_scope.clone()),
        (None, true) => None,
    };

    // Check if exported
    let is_exported = is_namespace_exported(namespace_node);

    // Generate entity ID
    let file_path_str = file_path.to_string_lossy();
    let entity_id = generate_entity_id(repository_id, &file_path_str, &full_qualified_name);

    // Get location
    let location = SourceLocation::from_tree_sitter_node(namespace_node);

    // Create components
    let components = CommonEntityComponents {
        entity_id,
        repository_id: repository_id.to_string(),
        name,
        qualified_name: full_qualified_name,
        path_entity_identifier: None,
        parent_scope,
        file_path: file_path.to_path_buf(),
        location,
    };

    // Build the entity
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Module,
            language: Language::TypeScript,
            visibility: Some(if is_exported {
                Visibility::Public
            } else {
                Visibility::Private
            }),
            documentation: None,
            content: node_to_text(namespace_node, source).ok(),
            metadata: EntityMetadata::default(),
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Check if a namespace is exported (has an export_statement ancestor)
#[allow(dead_code)]
fn is_namespace_exported(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "export_statement" {
            return true;
        }
        current = parent.parent();
    }
    false
}
