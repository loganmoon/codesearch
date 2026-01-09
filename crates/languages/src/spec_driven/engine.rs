//! Spec-driven extraction engine
//!
//! This module provides the core extraction logic that uses `HandlerConfig`
//! to extract entities from source code in a declarative way.

use super::{HandlerConfig, MetadataExtractor, NameStrategy, RelationshipExtractor};
use crate::common::edge_case_handlers::EdgeCaseRegistry;
use crate::common::entity_building::{build_entity, CommonEntityComponents, EntityDetails};
use crate::common::import_map::ImportMap;
use crate::common::path_config::PathConfig;
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

    /// Import map for resolving bare identifiers to qualified names
    pub import_map: &'a ImportMap,

    /// Language-specific path configuration for resolution
    pub path_config: &'static PathConfig,

    /// Optional edge case handlers for language-specific patterns
    pub edge_case_handlers: Option<&'a EdgeCaseRegistry>,
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
        let name = evaluate_name_strategy(&config.name_strategy, &captures, ctx, main_node)?;

        // Build common components with the derived name
        let components = build_common_components(ctx, &name, main_node)?;

        // Build qualified name from template if provided
        let qualified_name = if let Some(template) = config.qualified_name_template {
            expand_qualified_name_template(template, &captures, &components)
        } else {
            components.qualified_name.clone()
        };

        // Create modified components with custom qualified name
        // Also clear parent_scope if it equals qualified_name (entity IS the module itself)
        let parent_scope = if components
            .parent_scope
            .as_ref()
            .is_some_and(|ps| ps == &qualified_name)
        {
            None
        } else {
            components.parent_scope.clone()
        };

        let components = CommonEntityComponents {
            qualified_name,
            parent_scope,
            ..components
        };

        // Determine entity type from entity_rule
        let entity_type = entity_type_from_rule(config.entity_rule)?;

        // Extract metadata using the configured extractor
        let metadata =
            extract_metadata(config.metadata_extractor, main_node, ctx.source, &captures);

        // Extract relationships using the configured extractor.
        // For Property entities without explicit relationship extractors, we fall back
        // to ExtractTypeRelationships to capture type annotations as TypeUsage references.
        let relationships = extract_relationships_with_fallback(
            config.relationship_extractor,
            entity_type,
            main_node,
            ctx,
            Some(components.qualified_name.as_str()),
        );

        // Determine visibility
        let visibility = config
            .visibility_override
            .or_else(|| extract_visibility_from_node(main_node, ctx.source, ctx.language_str));

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
        let capture_name = match query.capture_names().get(capture.index as usize) {
            Some(name) => *name,
            None => {
                tracing::warn!(
                    capture_index = capture.index,
                    "Capture index out of bounds in query - possible query/language mismatch"
                );
                continue;
            }
        };

        match node_to_text(capture.node, source) {
            Ok(text) => {
                captures.insert(capture_name.to_string(), text);
            }
            Err(e) => {
                tracing::debug!(
                    capture_name = capture_name,
                    error = %e,
                    "Failed to extract text for capture"
                );
            }
        }
    }

    captures
}

/// Evaluate a name strategy to produce an entity name
fn evaluate_name_strategy(
    strategy: &NameStrategy,
    captures: &HashMap<String, String>,
    ctx: &SpecDrivenContext,
    main_node: Node,
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
            // For JS/TS, derive module path from file path relative to source root.
            // For other languages, just use the file stem.
            if matches!(ctx.language_str, "javascript" | "typescript" | "tsx") {
                if let Some(root) = ctx.source_root {
                    if let Some(module_path) =
                        crate::common::js_ts_shared::module_path::derive_module_path(
                            ctx.file_path,
                            root,
                        )
                    {
                        return Ok(module_path);
                    }
                    // Module path derivation failed - file may be outside source root
                    tracing::debug!(
                        file_path = ?ctx.file_path,
                        source_root = ?root,
                        "Module path derivation failed for JS/TS file, falling back to file stem"
                    );
                } else {
                    // No source_root configured for JS/TS file
                    tracing::debug!(
                        file_path = ?ctx.file_path,
                        language = ctx.language_str,
                        "No source_root configured for JS/TS file, using file stem for module path"
                    );
                }
            }
            // Fallback to file stem
            ctx.file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(String::from)
                .ok_or_else(|| {
                    Error::entity_extraction("Cannot derive name from file path".to_string())
                })
        }

        NameStrategy::CrateName => ctx
            .package_name
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

    // Replace {name} with the entity's derived name
    result = result.replace("{name}", &components.name);

    // Resolve {impl_type_name} to a fully qualified name. This ensures templates
    // like `<{impl_type_name}>::{name}` produce `<my_crate::MyStruct>::foo`
    // instead of just `<MyStruct>::foo`.
    //
    // Resolution order:
    // 1. If impl_type_path was captured (e.g., `mod::Type`), use path::type_name
    // 2. If parent_scope already ends with the type name (method context), use scope as-is
    // 3. Otherwise (impl block context), prepend the module-level scope
    // 4. If no scope available, use the simple type name
    if let Some(impl_type_name) = captures.get("impl_type_name") {
        let qualified_impl_type = if let Some(impl_type_path) = captures.get("impl_type_path") {
            // Case 1: Scoped type like `mod::Type`
            format!("{impl_type_path}::{impl_type_name}")
        } else if let Some(ref scope) = components.parent_scope {
            if scope.ends_with(&format!("::{impl_type_name}")) || scope == impl_type_name {
                // Case 2: Method context - scope already is the qualified type name
                scope.clone()
            } else {
                // Case 3: Impl block context - prepend module scope
                format!("{scope}::{impl_type_name}")
            }
        } else {
            // Case 4: No scope available
            impl_type_name.clone()
        };
        result = result.replace("{impl_type_name}", &qualified_impl_type);
    }

    // Replace remaining capture placeholders (skip those with special handling above)
    for (capture_name, value) in captures {
        if capture_name == "impl_type_name" || capture_name == "impl_type_path" {
            continue;
        }
        result = result.replace(&format!("{{{capture_name}}}"), value);
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
        // Modules (including namespaces)
        r if r.starts_with("E-MOD") => Ok(EntityType::Module),

        // Functions (including extern, ambient, arrow functions)
        r if r.starts_with("E-FN") || r == "E-EXTERN-FN" || r == "E-AMBIENT-FN" => {
            Ok(EntityType::Function)
        }

        // Methods (including interface methods, abstract methods)
        r if r.starts_with("E-METHOD") || r == "E-INTERFACE-METHOD" => Ok(EntityType::Method),

        // Interface signatures (TypeScript)
        "E-INTERFACE-CALL-SIG" | "E-INTERFACE-CONSTRUCT-SIG" => Ok(EntityType::Method),
        "E-INTERFACE-INDEX-SIG" => Ok(EntityType::Property),

        // Impl blocks (Rust)
        "E-IMPL-INHERENT" | "E-IMPL-TRAIT" => Ok(EntityType::Impl),

        // Types
        "E-STRUCT" => Ok(EntityType::Struct),
        "E-ENUM" => Ok(EntityType::Enum),
        "E-ENUM-VARIANT" | "E-ENUM-MEMBER" => Ok(EntityType::EnumVariant),
        "E-TRAIT" => Ok(EntityType::Trait),
        "E-INTERFACE" => Ok(EntityType::Interface),
        "E-TYPE-ALIAS" | "E-TYPE-ALIAS-ASSOC" => Ok(EntityType::TypeAlias),
        "E-UNION" => Ok(EntityType::Union),

        // Properties (including interface properties, parameter properties, fields)
        r if r.starts_with("E-PROPERTY") || r == "E-FIELD" || r == "E-PARAM-PROPERTY" => {
            Ok(EntityType::Property)
        }
        "E-INTERFACE-PROPERTY" => Ok(EntityType::Property),

        // Constants and statics
        r if r.starts_with("E-CONST") && r != "E-CONST-VAR" => Ok(EntityType::Constant),
        "E-AMBIENT-CONST" => Ok(EntityType::Constant),
        "E-STATIC" | "E-EXTERN-STATIC" => Ok(EntityType::Static),

        // Macros
        "E-MACRO" | "E-MACRO-RULES" => Ok(EntityType::Macro),

        // Classes (JS/TS, including ambient classes)
        r if r.starts_with("E-CLASS") || r == "E-AMBIENT-CLASS" => Ok(EntityType::Class),

        // Variables (JS/TS, including ambient)
        r if r.starts_with("E-VAR")
            || r.starts_with("E-LET")
            || r == "E-CONST-VAR"
            || r == "E-AMBIENT-LET"
            || r == "E-AMBIENT-VAR" =>
        {
            Ok(EntityType::Variable)
        }

        // Extern blocks (Rust)
        "E-EXTERN-BLOCK" => Ok(EntityType::ExternBlock),

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
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> EntityRelationshipData {
    let Some(extractor) = extractor else {
        return EntityRelationshipData::default();
    };

    super::relationships::extract_relationships(extractor, node, ctx, parent_scope)
}

/// Extract relationships, with automatic fallback for Property entities.
///
/// Property entities (struct fields, class fields, interface properties) often have
/// type annotations but may not have explicit relationship extractors configured in
/// the spec. This function provides a fallback to ExtractTypeRelationships for
/// Property entities, ensuring type annotation references are captured.
fn extract_relationships_with_fallback(
    extractor: Option<RelationshipExtractor>,
    entity_type: EntityType,
    node: Node,
    ctx: &SpecDrivenContext,
    parent_scope: Option<&str>,
) -> EntityRelationshipData {
    // If an extractor is configured, use it
    if extractor.is_some() {
        return extract_relationships(extractor, node, ctx, parent_scope);
    }

    // For Property entities without explicit extractors, auto-extract type references
    if entity_type == EntityType::Property {
        tracing::trace!(
            entity_type = ?entity_type,
            "No relationship extractor configured, using type reference fallback for Property"
        );
        return super::relationships::extract_relationships(
            RelationshipExtractor::ExtractTypeRelationships,
            node,
            ctx,
            parent_scope,
        );
    }

    EntityRelationshipData::default()
}

/// Extract visibility from a node (language-aware)
fn extract_visibility_from_node(node: Node, source: &str, language: &str) -> Option<Visibility> {
    // First, check for Rust visibility modifiers (pub, pub(crate), etc.)
    if let Some(vis) = extract_rust_visibility(node, source) {
        return Some(vis);
    }

    // Check for JS/TS export-based visibility (only for JavaScript/TypeScript)
    if matches!(language, "javascript" | "typescript" | "tsx" | "jsx") {
        if let Some(vis) = extract_js_ts_visibility(node) {
            return Some(vis);
        }
    }

    // For Rust macros, check for #[macro_export] attribute
    if node.kind() == "macro_definition" {
        return Some(extract_macro_visibility(node, source));
    }

    // Apply language-specific defaults
    // In Rust, items without visibility modifier are private by default
    if language == "rust" {
        // Most Rust items (functions, structs, modules, etc.) are private by default
        let rust_item_kinds = [
            "function_item",
            "struct_item",
            "enum_item",
            "type_item",
            "mod_item",
            "const_item",
            "static_item",
            "trait_item",
            "impl_item",
            "union_item",
            "extern_crate_declaration",
            "use_declaration",
        ];
        if rust_item_kinds.contains(&node.kind()) {
            return Some(Visibility::Private);
        }
        // Module declarations are private by default
        if node.kind() == "mod_item" {
            return Some(Visibility::Private);
        }
    }

    None
}

/// Extract JS/TS visibility based on export keyword and access modifiers
fn extract_js_ts_visibility(node: Node) -> Option<Visibility> {
    // Module (program) nodes are implicitly public (can be imported)
    if node.kind() == "program" {
        return Some(Visibility::Public);
    }

    // Check if this node IS an export_statement
    if node.kind() == "export_statement" {
        return Some(Visibility::Public);
    }

    // Check if this node's parent is an export_statement
    if let Some(parent) = node.parent() {
        if parent.kind() == "export_statement" {
            return Some(Visibility::Public);
        }
    }

    // For items inside namespaces, check for export keyword
    if is_inside_namespace(node) {
        // Check if this node starts with 'export' keyword
        // The export_statement wraps the actual declaration inside namespaces
        if let Some(parent) = node.parent() {
            if parent.kind() == "export_statement" {
                return Some(Visibility::Public);
            }
        }
        // Not exported from namespace → private to namespace
        return Some(Visibility::Private);
    }

    // For JS/TS module-level declarations, check if we're at module level
    // (parent is program) - these are Private by default
    if let Some(parent) = node.parent() {
        if parent.kind() == "program" {
            return Some(Visibility::Private);
        }
    }

    // Enum members inherit visibility from parent enum (always public within enum)
    // The captured node can be either:
    // - property_identifier directly in enum_body (simple enum members)
    // - enum_assignment (enum members with explicit values)
    if node.kind() == "enum_assignment" {
        return Some(Visibility::Public);
    }
    if node.kind() == "property_identifier" {
        if let Some(parent) = node.parent() {
            if parent.kind() == "enum_body" || parent.kind() == "enum_assignment" {
                return Some(Visibility::Public);
            }
        }
    }

    // Interface members are always public
    if node.kind() == "property_signature"
        || node.kind() == "method_signature"
        || node.kind() == "call_signature"
        || node.kind() == "construct_signature"
        || node.kind() == "index_signature"
    {
        return Some(Visibility::Public);
    }

    // For class members, check for TypeScript accessibility modifiers
    if node.kind() == "public_field_definition"
        || node.kind() == "field_definition"
        || node.kind() == "method_definition"
    {
        // Check for accessibility_modifier child (public/private/protected)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "accessibility_modifier" {
                // Get the text to determine which modifier
                if let Some(first_child) = child.child(0) {
                    return match first_child.kind() {
                        "private" => Some(Visibility::Private),
                        "protected" => Some(Visibility::Protected),
                        "public" => Some(Visibility::Public),
                        _ => None,
                    };
                }
            }
            // Check for # private field syntax
            if child.kind() == "private_property_identifier" {
                return Some(Visibility::Private);
            }
        }
        // Default: public for class members without explicit modifier
        return Some(Visibility::Public);
    }

    None
}

/// Check if a node is inside a TypeScript namespace declaration
fn is_inside_namespace(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        // Check for TypeScript namespace/module declaration
        // The AST structure is: namespace_declaration > statement_block > declarations
        if parent.kind() == "namespace_declaration"
            || parent.kind() == "module_declaration"
            || parent.kind() == "internal_module"
        {
            return true;
        }
        current = parent.parent();
    }
    false
}

/// Extract Rust visibility modifier from a node
fn extract_rust_visibility(node: Node, source: &str) -> Option<Visibility> {
    // Look for a visibility_modifier child
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            if let Ok(text) = node_to_text(child, source) {
                let trimmed = text.trim();
                if trimmed == "pub" {
                    return Some(Visibility::Public);
                } else if trimmed.starts_with("pub(crate)")
                    || trimmed.starts_with("pub(super)")
                    || trimmed.starts_with("pub(in")
                {
                    // pub(crate), pub(super), and pub(in path) are internal visibility
                    // They're accessible beyond the immediate scope but not publicly
                    return Some(Visibility::Internal);
                } else if trimmed.starts_with("pub(self)") {
                    // pub(self) is effectively private to the current module
                    return Some(Visibility::Private);
                }
            }
        }
    }
    None
}

/// Extract visibility for Rust macros based on #[macro_export] attribute
fn extract_macro_visibility(node: Node, source: &str) -> Visibility {
    // Check preceding siblings for attribute_item with macro_export
    let mut current = node;
    while let Some(prev) = current.prev_sibling() {
        if prev.kind() == "attribute_item" {
            if let Ok(text) = node_to_text(prev, source) {
                if text.contains("macro_export") {
                    return Visibility::Public;
                }
            }
            current = prev;
        } else if prev.kind() == "line_comment" || prev.kind() == "block_comment" {
            // Skip comments
            current = prev;
        } else {
            // Stop at non-attribute, non-comment node
            break;
        }
    }
    // Default to Private for macros without #[macro_export]
    Visibility::Private
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
