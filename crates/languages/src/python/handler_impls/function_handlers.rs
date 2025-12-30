//! Python function handler implementations

use crate::common::{
    entity_building::{build_entity, extract_common_components, EntityDetails, ExtractionContext},
    find_capture_node,
    import_map::{get_ast_root, parse_file_imports},
    node_to_text, require_capture_node,
};
use crate::python::{
    module_path::derive_module_path,
    utils::{
        extract_decorators, extract_docstring, extract_function_calls, extract_python_parameters,
        extract_return_type, extract_type_references, is_async_function,
    },
};
use codesearch_core::{
    entities::{EntityMetadata, EntityType, FunctionSignature, Language, Visibility},
    error::Result,
    CodeEntity,
};
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Handle Python function definitions (module-level functions)
#[allow(clippy::too_many_arguments)]
pub fn handle_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let function_node = require_capture_node(query_match, query, "function")?;

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

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Build import map from file's imports for qualified name resolution
    let root = get_ast_root(function_node);
    let import_map = parse_file_imports(root, source, Language::Python, module_path.as_deref());

    // Extract function calls from the function body with qualified name resolution
    let calls = extract_function_calls(
        function_node,
        source,
        &import_map,
        components.parent_scope.as_deref(),
    );

    // Extract type references from type hints for USES relationships
    let type_refs = extract_type_references(
        function_node,
        source,
        &import_map,
        components.parent_scope.as_deref(),
    );

    // Build metadata
    let mut metadata = EntityMetadata {
        is_async,
        decorators,
        ..EntityMetadata::default()
    };

    // Store function calls if any exist
    if !calls.is_empty() {
        if let Ok(json) = serde_json::to_string(&calls) {
            metadata.attributes.insert("calls".to_string(), json);
        }
    }

    // Store type references for USES relationships
    if !type_refs.is_empty() {
        if let Ok(json) = serde_json::to_string(&type_refs) {
            metadata.attributes.insert("uses_types".to_string(), json);
        }
    }

    // Build entity using shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: Language::Python,
            visibility: Visibility::Public, // Python doesn't have visibility keywords
            documentation,
            content: node_to_text(function_node, source).ok(),
            metadata,
            signature: Some(FunctionSignature {
                parameters,
                return_type,
                generics: Vec::new(),
                is_async,
            }),
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}
