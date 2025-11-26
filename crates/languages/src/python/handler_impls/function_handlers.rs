//! Python function handler implementations

use crate::common::{
    entity_building::{build_entity, extract_common_components, EntityDetails, ExtractionContext},
    find_capture_node, node_to_text,
    python_common::{
        extract_decorators, extract_docstring, extract_python_parameters, extract_return_type,
        is_async_function,
    },
    require_capture_node,
};
use codesearch_core::{
    entities::{EntityMetadata, EntityType, FunctionSignature, Language, Visibility},
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

    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
    };

    // Extract common components
    let components = extract_common_components(&ctx, "name", function_node, "python")?;

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

    // Build entity using shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: Language::Python,
            visibility: Visibility::Public, // Python doesn't have visibility keywords
            documentation,
            content: node_to_text(function_node, source).ok(),
            metadata: EntityMetadata {
                is_async,
                decorators,
                ..EntityMetadata::default()
            },
            signature: Some(FunctionSignature {
                parameters,
                return_type,
                generics: Vec::new(),
                is_async,
            }),
        },
    )?;

    Ok(vec![entity])
}
