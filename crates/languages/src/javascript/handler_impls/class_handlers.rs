//! JavaScript class handler implementations

use crate::common::{
    entity_building::{build_entity, extract_common_components, EntityDetails, ExtractionContext},
    find_capture_node,
    js_ts_common::{extract_jsdoc_comments, extract_parameters},
    node_to_text, require_capture_node,
};
use codesearch_core::{
    entities::{EntityMetadata, EntityType, FunctionSignature, Language, Visibility},
    error::Result,
    CodeEntity,
};
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Handle class declarations
pub fn handle_class_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
) -> Result<Vec<CodeEntity>> {
    let class_node = require_capture_node(query_match, query, "class")?;

    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
    };

    // Extract common components
    let components = extract_common_components(&ctx, "name", class_node, "javascript")?;

    // Extract extends clause if present
    let extends = if let Some(extends_node) = find_capture_node(query_match, query, "extends") {
        node_to_text(extends_node, source).ok()
    } else {
        None
    };

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(class_node, source);

    // Build metadata
    let mut metadata = EntityMetadata::default();
    if let Some(extends_text) = extends {
        metadata
            .attributes
            .insert("extends".to_string(), extends_text);
    }

    // Build entity using shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Class,
            language: Language::JavaScript,
            visibility: Visibility::Public,
            documentation,
            content: node_to_text(class_node, source).ok(),
            metadata,
            signature: None,
        },
    )?;

    Ok(vec![entity])
}

/// Handle class methods
pub fn handle_method_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
) -> Result<Vec<CodeEntity>> {
    let method_node = require_capture_node(query_match, query, "method")?;

    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
    };

    // Extract common components
    let components = extract_common_components(&ctx, "name", method_node, "javascript")?;

    // Extract parameters
    let parameters = if let Some(params_node) = find_capture_node(query_match, query, "params") {
        extract_parameters(params_node, source)?
    } else {
        Vec::new()
    };

    // Check for static and async modifiers
    let mut is_static = false;
    let mut is_async = false;

    for child in method_node.children(&mut method_node.walk()) {
        match child.kind() {
            "static" => is_static = true,
            "async" => is_async = true,
            _ => {}
        }
    }

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(method_node, source);

    // Build metadata
    let mut metadata = EntityMetadata {
        is_async,
        ..EntityMetadata::default()
    };
    if is_static {
        metadata
            .attributes
            .insert("static".to_string(), "true".to_string());
    }

    // Build entity using shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Method,
            language: Language::JavaScript,
            visibility: Visibility::Public,
            documentation,
            content: node_to_text(method_node, source).ok(),
            metadata,
            signature: Some(FunctionSignature {
                parameters,
                return_type: None,
                generics: Vec::new(),
                is_async,
            }),
        },
    )?;

    Ok(vec![entity])
}
