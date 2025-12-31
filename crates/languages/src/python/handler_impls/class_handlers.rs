//! Python class and method handler implementations

use crate::common::{
    entity_building::{build_entity, extract_common_components, EntityDetails, ExtractionContext},
    find_capture_node,
    import_map::{get_ast_root, parse_file_imports, resolve_reference},
    node_to_text, require_capture_node,
};
use crate::python::{
    module_path::derive_module_path,
    utils::{
        extract_base_classes, extract_decorators, extract_docstring, extract_function_calls,
        extract_python_parameters, extract_return_type, extract_type_references,
        filter_self_parameter, is_async_function, is_python_primitive,
    },
};
use codesearch_core::{
    entities::{EntityMetadata, EntityType, FunctionSignature, Language, Visibility},
    error::Result,
    CodeEntity,
};
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Handle Python class definitions
#[allow(clippy::too_many_arguments)]
pub fn handle_class_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
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
        repo_root,
    };

    // Extract common components
    let components = extract_common_components(&ctx, "name", class_node, "python")?;

    // Extract base classes
    let base_classes = extract_base_classes(class_node, source);

    // Extract docstring
    let documentation = extract_docstring(class_node, source);

    // Extract decorators
    let decorators = extract_decorators(class_node, source);

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Build import map for base class resolution
    let root = get_ast_root(class_node);
    let import_map = parse_file_imports(root, source, Language::Python, module_path.as_deref());

    // Build metadata
    let mut metadata = EntityMetadata {
        decorators,
        ..EntityMetadata::default()
    };

    if !base_classes.is_empty() {
        // Resolve base classes through import map and store directly in 'bases'
        let bases_resolved: Vec<String> = base_classes
            .iter()
            .filter(|base| !is_python_primitive(base))
            .map(|base| {
                resolve_reference(base, &import_map, components.parent_scope.as_deref(), ".")
            })
            .collect();

        if !bases_resolved.is_empty() {
            if let Ok(json) = serde_json::to_string(&bases_resolved) {
                metadata.attributes.insert("bases".to_string(), json);
            }
        }
    }

    // Build entity using shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Class,
            language: Language::Python,
            visibility: Some(Visibility::Public),
            documentation,
            content: node_to_text(class_node, source).ok(),
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Handle Python method definitions (functions inside class body)
#[allow(clippy::too_many_arguments)]
pub fn handle_method_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
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
        repo_root,
    };

    // Extract common components (name, qualified_name, entity_id, location)
    let components = extract_common_components(&ctx, "name", method_node, "python")?;

    // Extract parameters from query capture (filter self/cls for display)
    let raw_parameters = if let Some(params_node) = find_capture_node(query_match, query, "params")
    {
        extract_python_parameters(params_node, source)?
    } else {
        Vec::new()
    };
    let parameters = filter_self_parameter(raw_parameters);

    // Extract return type annotation
    let return_type = extract_return_type(method_node, source);

    // Check for async modifier
    let is_async = is_async_function(method_node);

    // Extract docstring
    let documentation = extract_docstring(method_node, source);

    // Extract decorators
    let decorators = extract_decorators(method_node, source);

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Build import map from file's imports for qualified name resolution
    let root = get_ast_root(method_node);
    let import_map = parse_file_imports(root, source, Language::Python, module_path.as_deref());

    // Extract function calls from the method body with qualified name resolution
    let calls = extract_function_calls(
        method_node,
        source,
        &import_map,
        components.parent_scope.as_deref(),
    );

    // Extract type references from type hints for USES relationships
    let type_refs = extract_type_references(
        method_node,
        source,
        &import_map,
        components.parent_scope.as_deref(),
    );

    // Determine if this is a static method, class method, or property
    let mut is_static = false;
    let mut is_classmethod = false;
    let mut is_property = false;

    for decorator in &decorators {
        match decorator.as_str() {
            "staticmethod" => is_static = true,
            "classmethod" => is_classmethod = true,
            "property" => is_property = true,
            _ => {}
        }
    }

    // Build metadata
    let mut metadata = EntityMetadata {
        is_async,
        decorators,
        ..EntityMetadata::default()
    };

    if is_static {
        metadata
            .attributes
            .insert("static".to_string(), "true".to_string());
    }
    if is_classmethod {
        metadata
            .attributes
            .insert("classmethod".to_string(), "true".to_string());
    }
    if is_property {
        metadata
            .attributes
            .insert("property".to_string(), "true".to_string());
    }

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

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Method,
            language: Language::Python,
            visibility: Some(Visibility::Public),
            documentation,
            content: node_to_text(method_node, source).ok(),
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
