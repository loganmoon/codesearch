//! JavaScript class handler implementations

use crate::common::{
    entity_building::{build_entity, extract_common_components, EntityDetails, ExtractionContext},
    find_capture_node,
    import_map::{get_ast_root, parse_file_imports, resolve_reference},
    node_to_text, require_capture_node,
};
use crate::javascript::utils::{
    extract_function_calls, extract_jsdoc_comments, extract_parameters,
    extract_type_references_from_jsdoc,
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

    // Extract just the class name from the class_heritage node for resolution
    let extends_class_name =
        if let Some(extends_node) = find_capture_node(query_match, query, "extends") {
            extract_base_class_name(extends_node, source)
        } else {
            None
        };

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(class_node, source);

    // Build import map for extends resolution
    let root = get_ast_root(class_node);
    let import_map = parse_file_imports(root, source, Language::JavaScript);

    // Build metadata
    let mut metadata = EntityMetadata::default();
    if let Some(ref extends_text) = extends {
        metadata
            .attributes
            .insert("extends".to_string(), extends_text.clone());

        // Resolve extends to qualified name using extracted class name
        if let Some(ref class_name) = extends_class_name {
            let extends_resolved = resolve_reference(
                class_name,
                &import_map,
                components.parent_scope.as_deref(),
                ".",
            );
            metadata
                .attributes
                .insert("extends_resolved".to_string(), extends_resolved);
        }
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

    // Build import map from file's imports for qualified name resolution
    let root = get_ast_root(method_node);
    let import_map = parse_file_imports(root, source, Language::JavaScript);

    // Extract function calls from the method body with qualified name resolution
    let calls = extract_function_calls(
        method_node,
        source,
        &import_map,
        components.parent_scope.as_deref(),
    );

    // Extract type references from JSDoc for USES relationships
    let type_refs = extract_type_references_from_jsdoc(
        documentation.as_deref(),
        &import_map,
        components.parent_scope.as_deref(),
    );

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

/// Extract the base class name from a class_heritage node
///
/// The class_heritage node contains the full "extends BaseClass" text.
/// This function extracts just the class name for resolution.
fn extract_base_class_name(class_heritage_node: tree_sitter::Node, source: &str) -> Option<String> {
    // Walk through children to find the identifier (base class name)
    for child in class_heritage_node.named_children(&mut class_heritage_node.walk()) {
        match child.kind() {
            // Direct identifier: `extends BaseClass`
            "identifier" => {
                return node_to_text(child, source).ok();
            }
            // Member expression: `extends module.BaseClass`
            "member_expression" => {
                return node_to_text(child, source).ok();
            }
            // Call expression: `extends BaseClass(args)` - get the function
            "call_expression" => {
                if let Some(func) = child.child_by_field_name("function") {
                    return node_to_text(func, source).ok();
                }
            }
            _ => {}
        }
    }
    None
}
