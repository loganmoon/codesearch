//! Spec-driven extraction engine
//!
//! This module provides the core extraction logic that uses `HandlerConfig`
//! to extract entities from source code in a declarative way.

use super::{HandlerConfig, MetadataExtractor, NameStrategy, RelationshipExtractor};
use crate::common::entity_building::{build_entity, CommonEntityComponents, EntityDetails};
use crate::common::{find_capture_node, node_to_text};
use codesearch_core::entities::{
    EntityMetadata, EntityRelationshipData, EntityType, Language, Visibility,
};
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use std::collections::HashMap;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor, QueryMatch};

/// Context for spec-driven extraction
///
/// This context provides all the information needed for spec-driven extraction
/// without requiring the query_match and query fields that are only used
/// by the legacy handler-based extraction.
pub struct SpecDrivenContext<'a> {
    /// Source code being extracted
    pub source: &'a str,

    /// Path to the file being extracted
    pub file_path: &'a Path,

    /// Repository identifier
    pub repository_id: &'a str,

    /// Package/crate name from manifest
    pub package_name: Option<&'a str>,

    /// Source root for module path derivation
    pub source_root: Option<&'a Path>,

    /// Repository root for repo-relative paths
    pub repo_root: &'a Path,

    /// Language being extracted
    pub language: Language,

    /// Language string identifier (e.g., "rust", "javascript")
    pub language_str: &'a str,
}

/// Extract entities using a handler configuration
///
/// This function:
/// 1. Compiles and runs the tree-sitter query
/// 2. For each match, extracts the entity name using NameStrategy
/// 3. Builds the qualified name from the template
/// 4. Extracts metadata and relationships
/// 5. Builds and returns CodeEntity instances
pub fn extract_with_config(
    config: &HandlerConfig,
    ctx: &SpecDrivenContext,
    tree_root: Node,
) -> Result<Vec<CodeEntity>> {
    let ts_language = tree_root.language();

    // Compile the query
    let query = Query::new(&ts_language, config.query)
        .map_err(|e| Error::entity_extraction(format!("Failed to compile query: {e}")))?;

    let mut entities = Vec::new();
    let mut cursor = QueryCursor::new();

    // Set resource limits
    cursor.set_timeout_micros(5_000_000);
    cursor.set_match_limit(10_000);

    let mut matches = cursor.matches(&query, tree_root, ctx.source.as_bytes());

    while let Some(query_match) = matches.next() {
        // Find the primary capture node
        let Some(main_node) = find_capture_node(query_match, &query, config.capture) else {
            continue;
        };

        // Extract capture values for template expansion
        let captures = extract_capture_values(query_match, &query, ctx.source);

        // Evaluate name strategy to get entity name
        let name = evaluate_name_strategy(
            &config.name_strategy,
            &captures,
            ctx.file_path,
            ctx.package_name,
            main_node,
            ctx.source,
        )?;

        // Build common components with the derived name
        let components = build_common_components(ctx, &name, main_node)?;

        // Build qualified name from template if provided
        let qualified_name = if let Some(template) = config.qualified_name_template {
            expand_qualified_name_template(template, &captures, &components)
        } else {
            components.qualified_name.clone()
        };

        // Create modified components with custom qualified name
        let components = CommonEntityComponents {
            qualified_name,
            ..components
        };

        // Determine entity type from entity_rule
        let entity_type = entity_type_from_rule(config.entity_rule)?;

        // Extract metadata using the configured extractor
        let metadata =
            extract_metadata(config.metadata_extractor, main_node, ctx.source, &captures);

        // Extract relationships using the configured extractor
        let relationships = extract_relationships(config.relationship_extractor, main_node, ctx);

        // Determine visibility
        let visibility = config
            .visibility_override
            .or_else(|| extract_visibility_from_node(main_node, ctx.source));

        // Build the entity
        let entity = build_entity(
            components,
            EntityDetails {
                entity_type,
                language: ctx.language,
                visibility,
                documentation: extract_documentation(main_node, ctx.source),
                content: node_to_text(main_node, ctx.source).ok(),
                metadata,
                signature: None, // TODO: Extract function signature if applicable
                relationships,
            },
        )?;

        entities.push(entity);
    }

    Ok(entities)
}

/// Build common entity components from spec-driven context
fn build_common_components(
    ctx: &SpecDrivenContext,
    name: &str,
    main_node: Node,
) -> Result<CommonEntityComponents> {
    use crate::qualified_name::{build_qualified_name_from_ast, derive_module_path_for_language};
    use codesearch_core::entities::SourceLocation;
    use codesearch_core::entity_id::generate_entity_id;

    if name.is_empty() {
        return Err(Error::entity_extraction("Empty name provided".to_string()));
    }

    // Build qualified name via parent traversal using language-specific separator
    let scope_result = build_qualified_name_from_ast(main_node, ctx.source, ctx.language_str);
    let ast_scope = scope_result.parent_scope;
    let separator = scope_result.separator;

    // Derive module path from file path (if source_root is available)
    let module_prefix = ctx
        .source_root
        .and_then(|root| derive_module_path_for_language(ctx.file_path, root, ctx.language_str));

    // Compose fully qualified name: package::module::ast_scope::name
    let qualified_name = compose_qualified_name(
        ctx.package_name,
        module_prefix.as_deref(),
        &ast_scope,
        name,
        separator,
    );

    // Calculate parent_scope (everything except the final name)
    let parent_scope = compose_qualified_name(
        ctx.package_name,
        module_prefix.as_deref(),
        &ast_scope,
        "", // empty name to get just the parent scope
        separator,
    );

    // Generate path_entity_identifier using repo-relative path (for import resolution)
    let path_module = crate::common::module_utils::derive_path_entity_identifier(
        ctx.file_path,
        ctx.repo_root,
        separator,
    );
    let path_entity_identifier = compose_qualified_name(
        None, // No package prefix for path-based identifier
        Some(&path_module),
        &ast_scope,
        name,
        separator,
    );

    // Generate entity_id from repository + file_path + qualified name
    let file_path_str = ctx
        .file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path".to_string()))?;
    let entity_id = generate_entity_id(ctx.repository_id, file_path_str, &qualified_name);

    // Get location
    let location = SourceLocation::from_tree_sitter_node(main_node);

    Ok(CommonEntityComponents {
        entity_id,
        repository_id: ctx.repository_id.to_string(),
        name: name.to_string(),
        qualified_name,
        path_entity_identifier: Some(path_entity_identifier),
        parent_scope: if parent_scope.is_empty() {
            None
        } else {
            Some(parent_scope)
        },
        file_path: ctx.file_path.to_path_buf(),
        location,
    })
}

/// Compose a fully qualified name from components
fn compose_qualified_name(
    package: Option<&str>,
    module: Option<&str>,
    scope: &str,
    name: &str,
    separator: &str,
) -> String {
    let mut parts: Vec<&str> = Vec::with_capacity(4);

    if let Some(pkg) = package {
        if !pkg.is_empty() {
            parts.push(pkg);
        }
    }

    if let Some(mod_path) = module {
        if !mod_path.is_empty() {
            parts.push(mod_path);
        }
    }

    if !scope.is_empty() {
        parts.push(scope);
    }

    if !name.is_empty() {
        parts.push(name);
    }

    parts.join(separator)
}

/// Extract all capture values from a query match
fn extract_capture_values(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
) -> HashMap<String, String> {
    let mut captures = HashMap::new();

    for capture in query_match.captures {
        let capture_name = query
            .capture_names()
            .get(capture.index as usize)
            .cloned()
            .unwrap_or_default();

        if let Ok(text) = node_to_text(capture.node, source) {
            captures.insert(capture_name.to_string(), text);
        }
    }

    captures
}

/// Evaluate a name strategy to produce an entity name
fn evaluate_name_strategy(
    strategy: &NameStrategy,
    captures: &HashMap<String, String>,
    file_path: &Path,
    package_name: Option<&str>,
    main_node: Node,
    _source: &str,
) -> Result<String> {
    match strategy {
        NameStrategy::Capture { name } => captures
            .get(*name)
            .cloned()
            .ok_or_else(|| Error::entity_extraction(format!("Capture '{name}' not found"))),

        NameStrategy::Fallback { captures: names } => {
            for name in *names {
                if let Some(value) = captures.get(*name) {
                    if !value.is_empty() {
                        return Ok(value.clone());
                    }
                }
            }
            Err(Error::entity_extraction(
                "No fallback capture found with value".to_string(),
            ))
        }

        NameStrategy::Template { template } => Ok(expand_template(template, captures)),

        NameStrategy::Static { name } => Ok((*name).to_string()),

        NameStrategy::FilePath => {
            // Extract module name from file path
            file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(String::from)
                .ok_or_else(|| {
                    Error::entity_extraction("Cannot derive name from file path".to_string())
                })
        }

        NameStrategy::CrateName => package_name
            .map(String::from)
            .ok_or_else(|| Error::entity_extraction("No crate name available".to_string())),

        NameStrategy::PositionalIndex => {
            // Find the index of this node among its siblings of the same kind
            let index = compute_positional_index(main_node);
            Ok(index.to_string())
        }
    }
}

/// Expand a template string with capture values
fn expand_template(template: &str, captures: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (name, value) in captures {
        result = result.replace(&format!("{{{name}}}"), value);
    }
    result
}

/// Expand a qualified name template with captures and common components
fn expand_qualified_name_template(
    template: &str,
    captures: &HashMap<String, String>,
    components: &CommonEntityComponents,
) -> String {
    let mut result = template.to_string();

    // Replace engine-provided placeholders
    if let Some(ref scope) = components.parent_scope {
        result = result.replace("{scope}", scope);
    } else {
        result = result.replace("{scope}::", "");
        result = result.replace("{scope}.", "");
        result = result.replace("{scope}", "");
    }

    // Replace capture placeholders
    for (name, value) in captures {
        result = result.replace(&format!("{{{name}}}"), value);
    }

    result
}

/// Compute the positional index of a node among its same-kind siblings
fn compute_positional_index(node: Node) -> usize {
    let Some(parent) = node.parent() else {
        return 0;
    };

    let node_kind = node.kind();
    let mut index = 0;

    let mut cursor = parent.walk();
    for child in parent.children(&mut cursor) {
        if child.kind() == node_kind {
            if child.id() == node.id() {
                return index;
            }
            index += 1;
        }
    }

    index
}

/// Map entity rule ID to EntityType
fn entity_type_from_rule(rule: &str) -> Result<EntityType> {
    // Parse rule ID format: E-XXX or E-XXX-YYY
    match rule {
        // Modules
        r if r.starts_with("E-MOD") => Ok(EntityType::Module),

        // Functions
        "E-FN-FREE" | "E-FN-ASSOC" | "E-FN-DECL" => Ok(EntityType::Function),

        // Methods
        r if r.starts_with("E-METHOD") => Ok(EntityType::Method),

        // Types
        "E-STRUCT" => Ok(EntityType::Struct),
        "E-ENUM" => Ok(EntityType::Enum),
        "E-ENUM-VARIANT" => Ok(EntityType::EnumVariant),
        "E-TRAIT" | "E-INTERFACE" => Ok(EntityType::Interface),
        "E-TYPE-ALIAS" | "E-TYPE-ALIAS-ASSOC" => Ok(EntityType::TypeAlias),
        "E-UNION" => Ok(EntityType::Struct), // Rust unions are struct-like

        // Properties
        "E-PROPERTY" | "E-FIELD" | "E-INTERFACE-PROPERTY" => Ok(EntityType::Property),

        // Interface signatures (TypeScript)
        "E-INTERFACE-METHOD-SIG" | "E-INTERFACE-CALL-SIG" | "E-INTERFACE-CONSTRUCT-SIG" => {
            Ok(EntityType::Method)
        }
        "E-INTERFACE-INDEX-SIG" => Ok(EntityType::Property),

        // Constants
        "E-CONST" | "E-CONST-ASSOC" | "E-STATIC" => Ok(EntityType::Constant),

        // Macros
        "E-MACRO" => Ok(EntityType::Macro),

        // Classes (JS/TS)
        r if r.starts_with("E-CLASS") => Ok(EntityType::Class),

        // Variables (JS/TS)
        r if r.starts_with("E-VAR") || r.starts_with("E-LET") || r.starts_with("E-CONST-VAR") => {
            Ok(EntityType::Variable)
        }

        // Extern blocks
        "E-EXTERN-BLOCK" => Ok(EntityType::Module), // Treat as module-like container

        _ => Err(Error::entity_extraction(format!(
            "Unknown entity rule: {rule}"
        ))),
    }
}

/// Extract metadata using the configured extractor
fn extract_metadata(
    extractor: Option<MetadataExtractor>,
    node: Node,
    source: &str,
    captures: &HashMap<String, String>,
) -> EntityMetadata {
    let Some(extractor) = extractor else {
        return EntityMetadata::default();
    };

    match extractor {
        MetadataExtractor::FunctionMetadata => extract_function_metadata(node, source),
        MetadataExtractor::ArrowFunctionMetadata => extract_arrow_function_metadata(node, source),
        MetadataExtractor::MethodMetadata => extract_function_metadata(node, source),
        MetadataExtractor::ConstMetadata => extract_const_metadata(node, source, captures),
        MetadataExtractor::PropertyMetadata => extract_property_metadata(node, source, captures),
        MetadataExtractor::StructMetadata => extract_struct_metadata(node, source),
        MetadataExtractor::EnumMetadata => extract_enum_metadata(node, source),
        MetadataExtractor::TraitMetadata => extract_trait_metadata(node, source),
        MetadataExtractor::StaticMetadata => extract_static_metadata(node, source),
    }
}

/// Extract relationships using the configured extractor
fn extract_relationships(
    extractor: Option<RelationshipExtractor>,
    _node: Node,
    _ctx: &SpecDrivenContext,
) -> EntityRelationshipData {
    let Some(_extractor) = extractor else {
        return EntityRelationshipData::default();
    };

    // TODO: Implement relationship extraction
    // This requires significant work to port from the existing handlers
    EntityRelationshipData::default()
}

/// Extract visibility from a node (language-agnostic)
fn extract_visibility_from_node(_node: Node, _source: &str) -> Option<Visibility> {
    // TODO: Implement visibility extraction
    // This is language-specific and needs to be delegated
    None
}

/// Extract documentation from a node
fn extract_documentation(node: Node, source: &str) -> Option<String> {
    // Look for preceding comment nodes
    let mut current = node;
    let mut doc_lines = Vec::new();

    while let Some(prev) = current.prev_sibling() {
        if prev.kind() == "line_comment" || prev.kind() == "block_comment" {
            if let Ok(text) = node_to_text(prev, source) {
                // Check for doc comments (/// or /** */)
                let trimmed = text.trim();
                if trimmed.starts_with("///") || trimmed.starts_with("/**") {
                    let doc_text = if trimmed.starts_with("///") {
                        trimmed.strip_prefix("///").unwrap_or(trimmed).trim()
                    } else {
                        trimmed
                            .strip_prefix("/**")
                            .and_then(|s| s.strip_suffix("*/"))
                            .unwrap_or(trimmed)
                            .trim()
                    };
                    doc_lines.push(doc_text.to_string());
                }
            }
            current = prev;
        } else {
            break;
        }
    }

    if doc_lines.is_empty() {
        None
    } else {
        doc_lines.reverse();
        Some(doc_lines.join("\n"))
    }
}

// =============================================================================
// Metadata extraction functions
// =============================================================================

fn extract_function_metadata(node: Node, _source: &str) -> EntityMetadata {
    let mut metadata = EntityMetadata::default();

    // Check for async, const, unsafe modifiers
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "async" => metadata.is_async = true,
            "const" => metadata.is_const = true,
            "unsafe" => {
                metadata
                    .attributes
                    .insert("unsafe".to_string(), "true".to_string());
            }
            "type_parameters" => {
                metadata.is_generic = true;
                // TODO: Extract generic params
            }
            _ => {}
        }
    }

    metadata
}

fn extract_arrow_function_metadata(node: Node, _source: &str) -> EntityMetadata {
    let mut metadata = EntityMetadata::default();

    // Arrow functions can be async
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "async" {
            metadata.is_async = true;
        }
    }

    metadata
}

fn extract_const_metadata(
    _node: Node,
    _source: &str,
    captures: &HashMap<String, String>,
) -> EntityMetadata {
    let mut metadata = EntityMetadata {
        is_const: true,
        ..Default::default()
    };

    // Include type from captures if available
    if let Some(type_str) = captures.get("const_type") {
        metadata
            .attributes
            .insert("type".to_string(), type_str.clone());
    }

    metadata
}

fn extract_property_metadata(
    _node: Node,
    _source: &str,
    captures: &HashMap<String, String>,
) -> EntityMetadata {
    let mut metadata = EntityMetadata::default();

    // Include type from captures if available
    if let Some(type_str) = captures.get("field_type") {
        metadata
            .attributes
            .insert("type".to_string(), type_str.clone());
    }

    metadata
}

fn extract_struct_metadata(node: Node, _source: &str) -> EntityMetadata {
    let mut metadata = EntityMetadata::default();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "type_parameters" => {
                metadata.is_generic = true;
                // TODO: Extract generic params
            }
            "ordered_field_declaration_list" => {
                metadata
                    .attributes
                    .insert("is_tuple_struct".to_string(), "true".to_string());
            }
            _ => {}
        }
    }

    // Check for unit struct (no body)
    let has_body = if node.child_by_field_name("body").is_some() {
        true
    } else {
        let mut c = node.walk();
        let has_fields = node.children(&mut c).any(|child| {
            child.kind() == "field_declaration_list"
                || child.kind() == "ordered_field_declaration_list"
        });
        has_fields
    };

    if !has_body {
        metadata
            .attributes
            .insert("is_unit_struct".to_string(), "true".to_string());
    }

    metadata
}

fn extract_enum_metadata(node: Node, _source: &str) -> EntityMetadata {
    let mut metadata = EntityMetadata::default();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_parameters" {
            metadata.is_generic = true;
            // TODO: Extract generic params
        }
    }

    metadata
}

fn extract_trait_metadata(node: Node, _source: &str) -> EntityMetadata {
    let mut metadata = EntityMetadata::default();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_parameters" {
            metadata.is_generic = true;
            // TODO: Extract generic params
        }
    }

    metadata
}

fn extract_static_metadata(node: Node, _source: &str) -> EntityMetadata {
    let mut metadata = EntityMetadata::default();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "mutable_specifier" {
            metadata
                .attributes
                .insert("is_mutable".to_string(), "true".to_string());
        }
    }

    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_type_from_rule() {
        assert!(matches!(
            entity_type_from_rule("E-MOD-CRATE"),
            Ok(EntityType::Module)
        ));
        assert!(matches!(
            entity_type_from_rule("E-FN-FREE"),
            Ok(EntityType::Function)
        ));
        assert!(matches!(
            entity_type_from_rule("E-METHOD-SELF"),
            Ok(EntityType::Method)
        ));
        assert!(matches!(
            entity_type_from_rule("E-STRUCT"),
            Ok(EntityType::Struct)
        ));
    }

    #[test]
    fn test_expand_template() {
        let mut captures = HashMap::new();
        captures.insert("name".to_string(), "foo".to_string());
        captures.insert("impl_type_name".to_string(), "MyStruct".to_string());

        assert_eq!(
            expand_template("{impl_type_name}::{name}", &captures),
            "MyStruct::foo"
        );
    }
}
