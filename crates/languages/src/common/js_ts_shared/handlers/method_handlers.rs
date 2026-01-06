//! Method entity handlers for JavaScript and TypeScript

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use codesearch_core::entities::{EntityType, Language};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;

use super::super::visibility::{
    extract_visibility, is_async, is_generator, is_getter, is_setter, is_static_member,
};
use super::common::{
    build_js_metadata, extract_main_node, extract_preceding_doc_comments, node_to_text,
};

/// Handle method extraction
///
/// Handles:
/// - `method() {}`
/// - `static method() {}`
/// - `async method() {}`
/// - `*generatorMethod() {}`
/// - `get prop() {}`
/// - `set prop(v) {}`
/// - `#privateMethod() {}`
pub(crate) fn handle_method_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["method"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "javascript")?;

    let visibility = extract_visibility(node, ctx.source);
    let is_static = is_static_member(node);
    let is_async_fn = is_async(node);
    let is_generator_fn = is_generator(node);
    let is_getter_fn = is_getter(node);
    let is_setter_fn = is_setter(node);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    let metadata = build_js_metadata(
        is_static,
        is_async_fn,
        is_generator_fn,
        is_getter_fn,
        is_setter_fn,
        false,
    );

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Method,
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
