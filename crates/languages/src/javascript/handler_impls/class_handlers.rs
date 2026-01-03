//! JavaScript class handler implementations

use crate::common::{
    entity_building::{build_entity, extract_common_components, EntityDetails, ExtractionContext},
    find_capture_node,
    import_map::{get_ast_root, parse_file_imports, resolve_reference},
    node_to_text, require_capture_node,
};
use crate::javascript::{
    module_path::derive_module_path,
    utils::{
        extract_function_calls, extract_jsdoc_comments, extract_parameters,
        extract_type_references_from_jsdoc,
    },
};
use codesearch_core::{
    entities::{
        EntityMetadata, EntityRelationshipData, EntityType, FunctionSignature, Language,
        ReferenceType, SourceLocation, Visibility,
    },
    error::Result,
    CodeEntity,
};
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

/// Handle class declarations
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
    let components = extract_common_components(&ctx, "name", class_node, "javascript")?;

    // Extract just the class name from the class_heritage node for resolution
    let extends_class_name =
        if let Some(extends_node) = find_capture_node(query_match, query, "extends") {
            extract_base_class_name(extends_node, source)
        } else {
            None
        };

    // Extract JSDoc documentation
    let documentation = extract_jsdoc_comments(class_node, source);

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Build import map for extends resolution
    let root = get_ast_root(class_node);
    let import_map = parse_file_imports(root, source, Language::JavaScript, module_path.as_deref());

    // Build metadata
    let metadata = EntityMetadata::default();

    // Build relationship data for extends
    let mut relationships = EntityRelationshipData::default();
    if let Some(ref class_name) = extends_class_name {
        let extends_resolved = resolve_reference(
            class_name,
            &import_map,
            components.parent_scope.as_deref(),
            ".",
        );
        // Build SourceReference for the extends relationship
        if let Ok(extends_ref) = codesearch_core::entities::SourceReference::builder()
            .target(extends_resolved)
            .simple_name(class_name.clone())
            .is_external(false) // JS doesn't track external refs
            .location(SourceLocation::default())
            .ref_type(ReferenceType::Extends)
            .build()
        {
            relationships.extends.push(extends_ref);
        }
    }

    // Build entity using shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Class,
            language: Language::JavaScript,
            visibility: Some(Visibility::Public),
            documentation,
            content: node_to_text(class_node, source).ok(),
            metadata,
            signature: None,
            relationships,
        },
    )?;

    Ok(vec![entity])
}

/// Handle class methods
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

    // Derive module path for qualified name resolution
    let module_path = source_root.and_then(|root| derive_module_path(file_path, root));

    // Build import map from file's imports for qualified name resolution
    let root = get_ast_root(method_node);
    let import_map = parse_file_imports(root, source, Language::JavaScript, module_path.as_deref());

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

    // Build relationship data with calls and type references
    let relationships = EntityRelationshipData {
        calls,
        uses_types: type_refs,
        ..Default::default()
    };

    // Build entity using shared helper
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Method,
            language: Language::JavaScript,
            visibility: Some(Visibility::Public),
            documentation,
            content: node_to_text(method_node, source).ok(),
            metadata,
            signature: Some(FunctionSignature {
                parameters,
                return_type: None,
                generics: Vec::new(),
                is_async,
            }),
            relationships,
        },
    )?;

    Ok(vec![entity])
}

/// Extract the base class name from a class_heritage node
///
/// The class_heritage node contains the full "extends BaseClass" text.
/// This function extracts just the class name for resolution.
///
/// Tree structure varies by parser:
/// - JavaScript: class_heritage -> identifier
/// - TypeScript: class_heritage -> extends_clause -> identifier
fn extract_base_class_name(class_heritage_node: tree_sitter::Node, source: &str) -> Option<String> {
    // Helper to extract class name from a node that should contain it
    fn extract_from_extends_node(node: tree_sitter::Node, source: &str) -> Option<String> {
        for child in node.named_children(&mut node.walk()) {
            match child.kind() {
                // Simple identifier: `extends BaseClass`
                "identifier" | "type_identifier" => {
                    return node_to_text(child, source).ok();
                }
                // Member expression: `extends module.BaseClass`
                "member_expression" | "nested_type_identifier" => {
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

    // First try to find extends_clause (TypeScript structure)
    for child in class_heritage_node.named_children(&mut class_heritage_node.walk()) {
        if child.kind() == "extends_clause" {
            if let Some(name) = extract_from_extends_node(child, source) {
                return Some(name);
            }
        }
    }

    // Fallback: try direct extraction (JavaScript structure)
    extract_from_extends_node(class_heritage_node, source)
}
