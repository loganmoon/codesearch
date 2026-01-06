//! Function entity handlers for JavaScript and TypeScript

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use crate::common::js_ts_shared::{JavaScript, TypeScript};
use crate::common::language_extractors::{extract_main_node, no_relationships, LanguageExtractors};
use crate::common::{find_capture_node, node_to_text};
use crate::define_handler;
use codesearch_core::entities::EntityType;
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

use super::common::{arrow_function_metadata, function_metadata};

// JavaScript handlers
define_handler!(JavaScript, handle_function_declaration_impl, "function", Function, metadata: function_metadata);
define_handler!(JavaScript, handle_arrow_function_impl, "function", Function, metadata: arrow_function_metadata);

// TypeScript handlers
define_handler!(TypeScript, handle_ts_function_declaration_impl, "function", Function, metadata: function_metadata);
define_handler!(TypeScript, handle_ts_arrow_function_impl, "function", Function, metadata: arrow_function_metadata);

/// Check if a node has an export_statement ancestor
fn has_export_ancestor(node: tree_sitter::Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "export_statement" {
            return true;
        }
        current = parent.parent();
    }
    false
}

/// Extract function expression entity, preferring function's own name over variable name
///
/// For named function expressions like `const x = function bar() {}`, we use `bar`
/// as the entity name, not `x`. For anonymous function expressions like
/// `const x = function() {}`, we use the variable name `x`.
/// For IIFEs like `(function foo() {})()`, we use the function's name `foo`.
fn extract_function_expression<L: LanguageExtractors>(
    ctx: &ExtractionContext,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["function"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    // Prefer @fn_name (function's own name) over @name (variable name)
    let name = find_capture_node(ctx.query_match, ctx.query, "fn_name")
        .or_else(|| find_capture_node(ctx.query_match, ctx.query, "name"))
        .and_then(|n| node_to_text(n, ctx.source).ok())
        .unwrap_or_default();

    if name.is_empty() {
        return Ok(Vec::new());
    }

    // For IIFEs and other patterns where @name might not be captured,
    // try to build components using @fn_name or @name
    let name_capture = if find_capture_node(ctx.query_match, ctx.query, "name").is_some() {
        "name"
    } else {
        "fn_name"
    };

    // Build components with the resolved name
    let mut components = extract_common_components(ctx, name_capture, node, L::LANG_STR)?;
    components.name = name.clone();

    // Rebuild qualified_name with the correct name
    let parent_scope = components.parent_scope.clone().unwrap_or_default();
    let separator = if L::LANG_STR == "javascript" || L::LANG_STR == "typescript" {
        "."
    } else {
        "::"
    };

    components.qualified_name = if parent_scope.is_empty() {
        name
    } else {
        format!("{parent_scope}{separator}{name}")
    };

    // Determine visibility by checking for export ancestor
    // Since we now match at variable_declarator level, we need to walk up
    let visibility = if has_export_ancestor(node) {
        codesearch_core::entities::Visibility::Public
    } else {
        codesearch_core::entities::Visibility::Private
    };

    let documentation = L::extract_docs(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();
    let metadata = function_metadata(node, ctx.source);
    let relationships = no_relationships(ctx, node);

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: L::LANGUAGE,
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

// JavaScript function expression handler
pub(crate) fn handle_function_expression_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    extract_function_expression::<JavaScript>(ctx)
}

// TypeScript function expression handler
pub(crate) fn handle_ts_function_expression_impl(
    ctx: &ExtractionContext,
) -> Result<Vec<CodeEntity>> {
    extract_function_expression::<TypeScript>(ctx)
}
