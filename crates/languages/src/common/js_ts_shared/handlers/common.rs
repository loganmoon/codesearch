//! Common utilities for JavaScript/TypeScript entity handlers

use std::path::Path;
use std::sync::OnceLock;

use crate::common::entity_building::ExtractionContext;
use crate::common::{find_capture_node, module_utils, node_to_text};
use codesearch_core::entities::{
    EntityMetadata, EntityRelationshipData, ReferenceType, SourceLocation, SourceReference,
};
use codesearch_core::error::{Error, Result};
use im::HashMap as ImHashMap;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query};

use super::super::queries::{
    CALL_EXPRESSION_QUERY, IMPORT_STATEMENT_QUERY, REEXPORT_STATEMENT_QUERY, TYPE_ANNOTATION_QUERY,
};
use super::super::visibility::{is_async, is_generator, is_getter, is_setter, is_static_member};

// =============================================================================
// Lazy query compilation
// =============================================================================

/// Returns the lazily compiled call expression query.
/// Returns None only if the query string is invalid (which should never happen
/// since the query is a compile-time constant).
fn get_call_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            Query::new(&language, CALL_EXPRESSION_QUERY).ok()
        })
        .as_ref()
}

fn get_type_annotation_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            Query::new(&language, TYPE_ANNOTATION_QUERY).ok()
        })
        .as_ref()
}

fn get_import_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            Query::new(&language, IMPORT_STATEMENT_QUERY).ok()
        })
        .as_ref()
}

fn get_reexport_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            Query::new(&language, REEXPORT_STATEMENT_QUERY).ok()
        })
        .as_ref()
}

// =============================================================================
// Helper functions
// =============================================================================

/// Find the index of a named capture in a query
fn capture_index(query: &Query, name: &str) -> u32 {
    query
        .capture_names()
        .iter()
        .position(|n| *n == name)
        .unwrap_or(0) as u32
}

/// Create a SourceReference with common fields
fn create_source_ref(name: &str, node: Node, ref_type: ReferenceType) -> Option<SourceReference> {
    SourceReference::builder()
        .target(name.to_string())
        .simple_name(name.to_string())
        .location(SourceLocation::from_tree_sitter_node(node))
        .ref_type(ref_type)
        .build()
        .ok()
}

/// Create a SourceReference with a custom target (different from simple_name)
fn create_source_ref_with_target(
    target: String,
    simple_name: &str,
    node: Node,
    ref_type: ReferenceType,
) -> Option<SourceReference> {
    SourceReference::builder()
        .target(target)
        .simple_name(simple_name.to_string())
        .location(SourceLocation::from_tree_sitter_node(node))
        .ref_type(ref_type)
        .build()
        .ok()
}

// =============================================================================
// Documentation extraction
// =============================================================================

/// Extract JSDoc-style documentation comments preceding a node
pub(crate) fn extract_preceding_doc_comments(node: Node, source: &str) -> Option<String> {
    let mut doc_lines = Vec::new();
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        if doc_lines.len() >= 100 {
            break;
        }
        if sibling.kind() != "comment" {
            break;
        }
        if let Ok(text) = node_to_text(sibling, source) {
            if let Some(content) = text.strip_prefix("/**").and_then(|s| s.strip_suffix("*/")) {
                // JSDoc comment - use strip_prefix/suffix for UTF-8 safety
                for line in content.lines() {
                    let trimmed = line.trim().trim_start_matches('*').trim();
                    if !trimmed.is_empty() {
                        doc_lines.push(trimmed.to_string());
                    }
                }
            } else if let Some(content) = text.strip_prefix("//") {
                let content = content.trim();
                if !content.is_empty() {
                    doc_lines.push(content.to_string());
                }
            }
        }
        current = sibling.prev_sibling();
    }

    if doc_lines.is_empty() {
        None
    } else {
        doc_lines.reverse();
        Some(doc_lines.join("\n"))
    }
}

// =============================================================================
// Metadata helpers for define_handler! macro
// =============================================================================

pub(crate) fn function_metadata(node: Node, _source: &str) -> EntityMetadata {
    let mut attributes = ImHashMap::new();
    if is_generator(node) {
        attributes.insert("is_generator".to_string(), "true".to_string());
    }
    EntityMetadata {
        is_async: is_async(node),
        attributes,
        ..Default::default()
    }
}

pub(crate) fn arrow_function_metadata(node: Node, _source: &str) -> EntityMetadata {
    let mut attributes = ImHashMap::new();
    attributes.insert("is_arrow".to_string(), "true".to_string());
    EntityMetadata {
        is_async: is_async(node),
        attributes,
        ..Default::default()
    }
}

pub(crate) fn method_metadata(node: Node, _source: &str) -> EntityMetadata {
    let mut attributes = ImHashMap::new();
    if is_generator(node) {
        attributes.insert("is_generator".to_string(), "true".to_string());
    }
    if is_getter(node) {
        attributes.insert("is_getter".to_string(), "true".to_string());
    }
    if is_setter(node) {
        attributes.insert("is_setter".to_string(), "true".to_string());
    }
    EntityMetadata {
        is_static: is_static_member(node),
        is_async: is_async(node),
        attributes,
        ..Default::default()
    }
}

pub(crate) fn const_metadata(_node: Node, _source: &str) -> EntityMetadata {
    EntityMetadata {
        is_const: true,
        ..Default::default()
    }
}

pub(crate) fn property_metadata(node: Node, source: &str) -> EntityMetadata {
    let mut attributes = ImHashMap::new();
    if node
        .child_by_field_name("name")
        .is_some_and(|n| source[n.byte_range()].starts_with('#'))
    {
        attributes.insert("is_private".to_string(), "true".to_string());
    }
    if node.child_by_field_name("value").is_some() {
        attributes.insert("has_initializer".to_string(), "true".to_string());
    }
    EntityMetadata {
        is_static: is_static_member(node),
        attributes,
        ..Default::default()
    }
}

pub(crate) fn enum_metadata(node: Node, source: &str) -> EntityMetadata {
    let is_const = source[node.byte_range()].trim_start().starts_with("const");
    EntityMetadata {
        is_const,
        ..Default::default()
    }
}

// =============================================================================
// Relationship extraction helpers
// =============================================================================

/// Collect type identifiers from a node tree, returning SourceReferences
fn collect_type_refs(node: Node, source: &str, ref_type: ReferenceType) -> Vec<SourceReference> {
    let mut refs = Vec::new();
    collect_type_refs_recursive(node, source, ref_type, &mut refs);
    refs
}

fn collect_type_refs_recursive(
    node: Node,
    source: &str,
    ref_type: ReferenceType,
    refs: &mut Vec<SourceReference>,
) {
    match node.kind() {
        "type_identifier" | "identifier" => {
            let name = &source[node.byte_range()];
            if let Some(r) = create_source_ref(name, node, ref_type) {
                refs.push(r);
            }
        }
        "generic_type" => {
            // Extract base type name from generic: Foo<T> -> Foo
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = &source[name_node.byte_range()];
                if let Some(r) = create_source_ref(name, name_node, ref_type) {
                    refs.push(r);
                }
            }
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    collect_type_refs_recursive(child, source, ref_type, refs);
                }
            }
        }
    }
}

/// Extract extends/implements relationships from class heritage
pub(crate) fn extract_extends_relationships(
    ctx: &ExtractionContext,
    _node: Node,
) -> EntityRelationshipData {
    let mut rels = EntityRelationshipData::default();

    if let Some(heritage) = find_capture_node(ctx.query_match, ctx.query, "heritage") {
        for i in 0..heritage.child_count() {
            if let Some(child) = heritage.child(i) {
                match child.kind() {
                    "extends_clause" => {
                        if let Some(value) = child.child_by_field_name("value") {
                            rels.extends =
                                collect_type_refs(value, ctx.source, ReferenceType::Extends);
                        }
                    }
                    "implements_clause" => {
                        rels.implements =
                            collect_type_refs(child, ctx.source, ReferenceType::Implements);
                    }
                    _ => {}
                }
            }
        }
    }
    rels
}

/// Extract extends relationships from interface declaration
pub(crate) fn extract_interface_extends_relationships(
    ctx: &ExtractionContext,
    _node: Node,
) -> EntityRelationshipData {
    let mut rels = EntityRelationshipData::default();
    if let Some(extends_clause) = find_capture_node(ctx.query_match, ctx.query, "extends_clause") {
        rels.extended_types = collect_type_refs(extends_clause, ctx.source, ReferenceType::Extends);
    }
    rels
}

/// Extract function calls from function body using tree-sitter query
pub(crate) fn extract_function_calls(
    ctx: &ExtractionContext,
    _node: Node,
) -> EntityRelationshipData {
    let mut rels = EntityRelationshipData::default();

    let Some(body) = find_capture_node(ctx.query_match, ctx.query, "body") else {
        return rels;
    };

    let Some(query) = get_call_query() else {
        return rels;
    };

    let callee_idx = capture_index(query, "callee");
    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(query, body, ctx.source.as_bytes());

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            if capture.index == callee_idx {
                let name = &ctx.source[capture.node.byte_range()];
                if let Some(r) = create_source_ref(name, capture.node, ReferenceType::Call) {
                    rels.calls.push(r);
                }
            }
        }
    }

    rels
}

/// Primitive types to skip when extracting type usages
const PRIMITIVES: &[&str] = &[
    "string",
    "number",
    "boolean",
    "void",
    "null",
    "undefined",
    "any",
    "never",
    "unknown",
    "object",
    "symbol",
    "bigint",
];

/// Extract type usages from type annotations in entity
pub(crate) fn extract_type_usages(ctx: &ExtractionContext, _node: Node) -> EntityRelationshipData {
    let mut rels = EntityRelationshipData::default();

    // Search the whole entity node for type annotations (including parameters, return types)
    // Prefer the main entity capture, falling back to specific parts
    let search_node = find_capture_node(ctx.query_match, ctx.query, "function")
        .or_else(|| find_capture_node(ctx.query_match, ctx.query, "interface"))
        .or_else(|| find_capture_node(ctx.query_match, ctx.query, "class"))
        .or_else(|| find_capture_node(ctx.query_match, ctx.query, "body"))
        .or_else(|| find_capture_node(ctx.query_match, ctx.query, "params"));

    let Some(node) = search_node else {
        return rels;
    };

    let Some(query) = get_type_annotation_query() else {
        return rels;
    };

    let type_ref_idx = capture_index(query, "type_ref");
    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(query, node, ctx.source.as_bytes());

    // Track seen types to avoid duplicates
    let mut seen = std::collections::HashSet::new();

    while let Some(query_match) = matches.next() {
        for capture in query_match.captures {
            if capture.index != type_ref_idx {
                continue;
            }
            let name = &ctx.source[capture.node.byte_range()];
            if PRIMITIVES.contains(&name) || !seen.insert(name.to_string()) {
                continue;
            }
            if let Some(r) = create_source_ref(name, capture.node, ReferenceType::Uses) {
                rels.uses_types.push(r);
            }
        }
    }

    rels
}

/// Extract import relationships from module (program node)
pub(crate) fn extract_imports(ctx: &ExtractionContext, _node: Node) -> EntityRelationshipData {
    let mut rels = EntityRelationshipData::default();

    let Some(program) = find_capture_node(ctx.query_match, ctx.query, "program") else {
        return rels;
    };

    let Some(query) = get_import_query() else {
        return rels;
    };

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(query, program, ctx.source.as_bytes());

    while let Some(query_match) = matches.next() {
        if let Some(import_ref) = parse_import_match(query, query_match, ctx) {
            rels.imports.push(import_ref);
        }
    }

    rels
}

/// Parse a single import match into a SourceReference
fn parse_import_match<'a>(
    query: &Query,
    query_match: &tree_sitter::QueryMatch<'a, 'a>,
    ctx: &ExtractionContext,
) -> Option<SourceReference> {
    let mut import_name: Option<&str> = None;
    let mut import_node: Option<tree_sitter::Node> = None;
    let mut source_path: Option<&str> = None;
    let mut is_namespace = false;

    for capture in query_match.captures {
        let capture_name = query.capture_names().get(capture.index as usize).copied();
        match capture_name {
            Some("default_import" | "named_import") => {
                import_name = Some(&ctx.source[capture.node.byte_range()]);
                import_node = Some(capture.node);
            }
            Some("ns_import") => {
                import_name = Some(&ctx.source[capture.node.byte_range()]);
                import_node = Some(capture.node);
                is_namespace = true;
            }
            Some("source") => {
                let path = &ctx.source[capture.node.byte_range()];
                source_path = Some(path.trim_matches(|c| c == '"' || c == '\''));
            }
            _ => {}
        }
    }

    let (name, node, path) = (import_name?, import_node?, source_path?);
    let module_name = module_path_from_import(path, ctx);
    let target = if is_namespace {
        module_name
    } else {
        format!("{module_name}.{name}")
    };

    create_source_ref_with_target(target, name, node, ReferenceType::Import)
}

/// Extract reexport relationships from module (program node)
pub(crate) fn extract_reexports(ctx: &ExtractionContext, _node: Node) -> EntityRelationshipData {
    let mut rels = EntityRelationshipData::default();

    let Some(program) = find_capture_node(ctx.query_match, ctx.query, "program") else {
        return rels;
    };

    let Some(query) = get_reexport_query() else {
        return rels;
    };

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(query, program, ctx.source.as_bytes());

    while let Some(query_match) = matches.next() {
        if let Some(reexport_ref) = parse_reexport_match(query, query_match, ctx, program) {
            rels.reexports.push(reexport_ref);
        }
    }

    rels
}

/// Parse a single reexport match into a SourceReference
fn parse_reexport_match<'a>(
    query: &Query,
    query_match: &tree_sitter::QueryMatch<'a, 'a>,
    ctx: &ExtractionContext,
    fallback_node: Node,
) -> Option<SourceReference> {
    let mut export_name: Option<&str> = None;
    let mut export_node: Option<tree_sitter::Node> = None;
    let mut source_path: Option<&str> = None;
    let mut is_star = false;
    let mut star_node: Option<tree_sitter::Node> = None;

    for capture in query_match.captures {
        let capture_name = query.capture_names().get(capture.index as usize).copied();
        match capture_name {
            Some("export_name") => {
                export_name = Some(&ctx.source[capture.node.byte_range()]);
                export_node = Some(capture.node);
            }
            Some("source") => {
                let path = &ctx.source[capture.node.byte_range()];
                source_path = Some(path.trim_matches(|c| c == '"' || c == '\''));
            }
            Some("star_export") => {
                is_star = true;
                star_node = Some(capture.node);
            }
            _ => {}
        }
    }

    let path = source_path?;
    let module_name = module_path_from_import(path, ctx);

    if is_star {
        // Star reexport: target is the module itself
        let location_node = star_node.unwrap_or(fallback_node);
        create_source_ref(&module_name, location_node, ReferenceType::Reexport)
    } else {
        // Named reexport: target is module.name
        let (name, node) = (export_name?, export_node?);
        let target = format!("{module_name}.{name}");
        create_source_ref_with_target(target, name, node, ReferenceType::Reexport)
    }
}

/// Convert import path to module name, resolving relative to the current file
///
/// Uses repo_root to compute the relative directory path for proper resolution.
///
/// Examples (assuming current file is `models/index.ts` relative to repo root):
/// - `./user` -> `models.user`
/// - `./sub/mod` -> `models.sub.mod`
/// - `../sibling` -> `sibling`
///
/// Examples (assuming current file is `utils.ts` - no parent dir):
/// - `./helper` -> `helper`
fn module_path_from_import(import_path: &str, ctx: &ExtractionContext) -> String {
    // Get the relative path from repo_root (or source_root if available)
    let relative_file_path = ctx
        .source_root
        .and_then(|root| ctx.file_path.strip_prefix(root).ok())
        .or_else(|| ctx.file_path.strip_prefix(ctx.repo_root).ok())
        .unwrap_or(ctx.file_path);

    // Get the directory of the current file relative to the root
    let current_dir = relative_file_path.parent().unwrap_or(Path::new(""));

    // Convert current directory to module path prefix (e.g., "models" for "models/index.ts")
    let dir_prefix: Vec<&str> = current_dir
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();

    // Handle relative paths
    if let Some(rest) = import_path.strip_prefix("./") {
        // Same directory: prepend current directory path
        let import_parts: Vec<&str> = rest.split('/').collect();
        let mut parts = dir_prefix.clone();
        parts.extend(import_parts);
        parts.join(".")
    } else if let Some(rest) = import_path.strip_prefix("../") {
        // Parent directory: go up one level
        let import_parts: Vec<&str> = rest.split('/').collect();
        let mut parts: Vec<&str> = if !dir_prefix.is_empty() {
            dir_prefix[..dir_prefix.len() - 1].to_vec()
        } else {
            vec![]
        };
        parts.extend(import_parts);
        parts.join(".")
    } else {
        // Absolute/external module name - just use the path
        import_path.replace('/', ".")
    }
}

/// Extract function relationships (calls and type usages)
pub(crate) fn extract_function_relationships(
    ctx: &ExtractionContext,
    node: Node,
) -> EntityRelationshipData {
    let mut rels = extract_function_calls(ctx, node);
    let type_usages = extract_type_usages(ctx, node);
    rels.uses_types = type_usages.uses_types;
    rels
}

/// Extract interface relationships (extends and type usages)
pub(crate) fn extract_interface_relationships(
    ctx: &ExtractionContext,
    node: Node,
) -> EntityRelationshipData {
    let mut rels = extract_interface_extends_relationships(ctx, node);
    let type_usages = extract_type_usages(ctx, node);
    rels.uses_types = type_usages.uses_types;
    rels
}

/// Extract class relationships (extends, implements, and type usages)
pub(crate) fn extract_class_relationships(
    ctx: &ExtractionContext,
    node: Node,
) -> EntityRelationshipData {
    let mut rels = extract_extends_relationships(ctx, node);
    let type_usages = extract_type_usages(ctx, node);
    rels.uses_types = type_usages.uses_types;
    rels
}

/// Extract module relationships (imports and reexports)
pub(crate) fn extract_module_relationships(
    ctx: &ExtractionContext,
    node: Node,
) -> EntityRelationshipData {
    let mut rels = extract_imports(ctx, node);
    let reexports = extract_reexports(ctx, node);
    rels.reexports = reexports.reexports;
    rels
}

// =============================================================================
// Name derivation helpers for define_handler! macro
// =============================================================================

pub(crate) fn derive_module_name_from_ctx(ctx: &ExtractionContext, _node: Node) -> Result<String> {
    Ok(module_utils::derive_module_name(ctx.file_path))
}

pub(crate) fn derive_function_expression_name(
    ctx: &ExtractionContext,
    _node: Node,
) -> Result<String> {
    find_capture_node(ctx.query_match, ctx.query, "fn_name")
        .or_else(|| find_capture_node(ctx.query_match, ctx.query, "name"))
        .and_then(|n| node_to_text(n, ctx.source).ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::entity_extraction("Could not derive function expression name"))
}

pub(crate) fn derive_class_expression_name(ctx: &ExtractionContext, _node: Node) -> Result<String> {
    find_capture_node(ctx.query_match, ctx.query, "class_name")
        .or_else(|| find_capture_node(ctx.query_match, ctx.query, "name"))
        .and_then(|n| node_to_text(n, ctx.source).ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::entity_extraction("Could not derive class expression name"))
}

pub(crate) fn derive_index_signature_name(node: Node, source: &str) -> String {
    // Find first type identifier in the index signature
    fn find_type(node: Node, source: &str) -> Option<String> {
        if matches!(node.kind(), "predefined_type" | "type_identifier") {
            return Some(source[node.byte_range()].to_string());
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                if let Some(found) = find_type(child, source) {
                    return Some(found);
                }
            }
        }
        None
    }
    find_type(node, source)
        .map(|t| format!("[{t}]"))
        .unwrap_or_else(|| "[index]".to_string())
}
