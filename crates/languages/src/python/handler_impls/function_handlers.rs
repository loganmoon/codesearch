//! Python function handler implementations

use crate::common::{
    find_capture_node, node_to_text,
    python_common::{
        extract_decorators, extract_docstring, extract_python_parameters, extract_return_type,
        is_async_function,
    },
    require_capture_node,
};
use codesearch_core::{
    entities::{
        CodeEntityBuilder, EntityMetadata, EntityType, FunctionSignature, Language, SourceLocation,
        Visibility,
    },
    entity_id::generate_entity_id,
    error::Result,
    CodeEntity,
};
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Handle Python function definitions (module-level functions)
pub fn handle_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> Result<Vec<CodeEntity>> {
    let function_node = require_capture_node(query_match, query, "function")?;

    // Extract name
    let name_node = require_capture_node(query_match, query, "name")?;
    let name = node_to_text(name_node, source)?;

    // Build qualified name (Python uses "." separator)
    let qualified_name =
        crate::qualified_name::build_qualified_name_from_ast(function_node, source, "python");
    let full_qualified_name = if qualified_name.is_empty() {
        name.clone()
    } else {
        format!("{qualified_name}.{name}")
    };

    // Extract parameters from query capture
    let parameters = if let Some(params_node) = find_capture_node(query_match, query, "params") {
        extract_python_parameters(params_node, source)?
    } else {
        Vec::new()
    };

    // Extract return type annotation
    let return_type = extract_return_type(function_node, source);

    // Check for async modifier
    let is_async = is_async_function(function_node);

    // Extract docstring
    let documentation = extract_docstring(function_node, source);

    // Extract decorators
    let decorators = extract_decorators(function_node, source);

    // Build metadata
    let metadata = EntityMetadata {
        is_async,
        decorators,
        ..EntityMetadata::default()
    };

    // Build signature
    let signature = FunctionSignature {
        parameters,
        return_type,
        generics: Vec::new(),
        is_async,
    };

    // Generate entity_id
    let file_path_str = file_path.to_str().unwrap_or_default();
    let entity_id = generate_entity_id(repository_id, file_path_str, &full_qualified_name);

    // Build entity
    let entity = CodeEntityBuilder::default()
        .entity_id(entity_id)
        .repository_id(repository_id.to_string())
        .name(name)
        .qualified_name(full_qualified_name)
        .parent_scope(if qualified_name.is_empty() {
            None
        } else {
            Some(qualified_name)
        })
        .entity_type(EntityType::Function)
        .location(SourceLocation::from_tree_sitter_node(function_node))
        .visibility(Visibility::Public) // Python doesn't have visibility keywords
        .documentation_summary(documentation)
        .content(node_to_text(function_node, source).ok())
        .metadata(metadata)
        .signature(Some(signature))
        .language(Language::Python)
        .file_path(file_path.to_path_buf())
        .build()
        .map_err(|e| {
            codesearch_core::error::Error::entity_extraction(format!(
                "Failed to build CodeEntity: {e}"
            ))
        })?;

    Ok(vec![entity])
}
