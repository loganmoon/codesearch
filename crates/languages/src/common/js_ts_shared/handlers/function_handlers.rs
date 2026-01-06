//! Function entity handlers for JavaScript and TypeScript

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use codesearch_core::entities::{EntityType, Language};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

use super::super::visibility::{extract_visibility, is_async, is_generator};
use super::common::{
    build_js_metadata, extract_main_node, extract_preceding_doc_comments, node_to_text,
};

/// Handle function declaration extraction
///
/// Handles:
/// - `function foo() {}`
/// - `async function foo() {}`
/// - `function* foo() {}`
/// - `export function foo() {}`
pub(crate) fn handle_function_declaration_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["function"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "javascript")?;

    let visibility = extract_visibility(node, ctx.source);
    let is_async_fn = is_async(node);
    let is_generator_fn = is_generator(node);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    let metadata = build_js_metadata(false, is_async_fn, is_generator_fn, false, false, false);

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: Language::JavaScript,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Handle function expression extraction
///
/// Handles:
/// - `const foo = function() {}`
/// - `const foo = function bar() {}`
/// - `let foo = function() {}`
pub(crate) fn handle_function_expression_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["function"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "javascript")?;

    let visibility = extract_visibility(node, ctx.source);
    let is_async_fn = is_async(node);
    let is_generator_fn = is_generator(node);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    let metadata = build_js_metadata(false, is_async_fn, is_generator_fn, false, false, false);

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: Language::JavaScript,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Handle arrow function extraction
///
/// Handles:
/// - `const foo = () => {}`
/// - `const foo = (x) => x * 2`
/// - `const foo = async () => {}`
pub(crate) fn handle_arrow_function_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["function"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "javascript")?;

    let visibility = extract_visibility(node, ctx.source);
    let is_async_fn = is_async(node);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    // Arrow functions are marked with is_arrow = true
    let metadata = build_js_metadata(false, is_async_fn, false, false, false, true);

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: Language::JavaScript,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}
