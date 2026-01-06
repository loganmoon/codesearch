//! Function entity handlers for JavaScript and TypeScript

use crate::common::entity_building::{
    build_entity, extract_common_components, EntityDetails, ExtractionContext,
};
use codesearch_core::entities::{EntityType, Language};
use codesearch_core::error::Result;
use codesearch_core::CodeEntity;
use std::path::Path;
use tree_sitter::{Query, QueryMatch};

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
#[allow(clippy::too_many_arguments)]
pub fn handle_function_declaration_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(query_match, query, &["function"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    // Create extraction context
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
    let components = extract_common_components(&ctx, "name", node, "javascript")?;

    // Extract JS-specific details
    let visibility = extract_visibility(node, source);
    let is_async_fn = is_async(node);
    let is_generator_fn = is_generator(node);
    let documentation = extract_preceding_doc_comments(node, source);
    let content = node_to_text(node, source).ok();

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
#[allow(clippy::too_many_arguments)]
pub fn handle_function_expression_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(query_match, query, &["function"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    // Create extraction context
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
    let components = extract_common_components(&ctx, "name", node, "javascript")?;

    // Extract JS-specific details
    let visibility = extract_visibility(node, source);
    let is_async_fn = is_async(node);
    let is_generator_fn = is_generator(node);
    let documentation = extract_preceding_doc_comments(node, source);
    let content = node_to_text(node, source).ok();

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
#[allow(clippy::too_many_arguments)]
pub fn handle_arrow_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(query_match, query, &["function"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    // Create extraction context
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
    let components = extract_common_components(&ctx, "name", node, "javascript")?;

    // Extract JS-specific details
    let visibility = extract_visibility(node, source);
    let is_async_fn = is_async(node);
    let documentation = extract_preceding_doc_comments(node, source);
    let content = node_to_text(node, source).ok();

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
