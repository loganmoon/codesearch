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
use codesearch_core::entity_id::generate_entity_id;
use codesearch_core::error::{Error, Result};
use codesearch_core::CodeEntity;
use std::collections::HashMap;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor, QueryMatch, QueryPredicateArg};

/// Context for spec-driven extraction
///
/// This context provides all the information needed for spec-driven extraction,
/// including file metadata, language configuration, and import resolution context.
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

    /// Type alias map for resolving type aliases to canonical types (Rust only)
    pub type_alias_map: &'a crate::rust::import_resolution::TypeAliasMap,
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
            tracing::trace!(
                capture = config.capture,
                entity_rule = config.entity_rule,
                "Skipping match: capture node not found"
            );
            continue;
        };

        // Extract capture values for template expansion
        let captures = extract_capture_values(query_match, &query, ctx.source);

        // Evaluate custom predicates (e.g., #not-has-child?, #not-has-ancestor?)
        // These are not evaluated automatically by tree-sitter
        if !evaluate_custom_predicates(&query, query_match, config, &captures) {
            tracing::trace!(
                entity_rule = config.entity_rule,
                node_kind = main_node.kind(),
                "Skipping match: custom predicate not satisfied"
            );
            continue;
        }

        // Evaluate name strategy to get entity name
        let name = evaluate_name_strategy(&config.name_strategy, &captures, ctx, main_node)?;

        // Determine entity type from entity_rule (needed for entity_id generation)
        let entity_type = entity_type_from_rule(config.entity_rule)?;

        // Build common components with the derived name
        let components =
            build_common_components(ctx, &name, main_node, entity_type, config.skip_scopes)?;

        // Compute qualified impl type for UFCS call alias generation (Rust methods only).
        // Must be done here while we have access to the original AST-derived parent_scope,
        // which provides the correct module context for type name resolution.
        let qualified_impl_type = resolve_impl_type_name(
            &captures,
            components.parent_scope.as_deref(),
            ctx.import_map,
            ctx.type_alias_map,
            ctx.package_name,
        );

        // Build qualified name from template if provided
        let qualified_name = if let Some(template) = config.qualified_name_template {
            let expansion_ctx = TemplateExpansionContext {
                captures: &captures,
                components: &components,
                import_map: ctx.import_map,
                type_alias_map: ctx.type_alias_map,
                package_name: ctx.package_name,
                main_node: Some(main_node),
                source: Some(ctx.source),
            };
            expand_qualified_name_template(template, &expansion_ctx)
        } else {
            components.qualified_name.clone()
        };

        // Derive parent_scope using one of three mechanisms:
        // 1. If parent_scope_template is provided, use it (for extern items where containment
        //    differs from qualified name structure)
        // 2. If qualified_name_template is provided, derive from qualified name structure
        // 3. Otherwise, use AST-derived parent_scope
        //
        // Note: We use `name` (the simple entity name from name_strategy, e.g., "handle"),
        // not `components.name` which for template strategies may contain the unexpanded
        // template (e.g., "<{impl_type_name} as {trait_name}>").
        let parent_scope = if let Some(template) = config.parent_scope_template {
            // Use explicit parent_scope template (for extern items, etc.)
            // Note: parent_scope templates don't need where clause logic
            let expansion_ctx = TemplateExpansionContext {
                captures: &captures,
                components: &components,
                import_map: ctx.import_map,
                type_alias_map: ctx.type_alias_map,
                package_name: ctx.package_name,
                main_node: None,
                source: None,
            };
            let expanded = expand_qualified_name_template(template, &expansion_ctx);
            if expanded.is_empty() {
                None
            } else {
                Some(expanded)
            }
        } else if config.qualified_name_template.is_some() {
            // Try to derive parent from the qualified name structure using the simple name
            derive_parent_from_qualified_name(&qualified_name, &name)
                // Fall back to original parent_scope if derivation returns None
                .or_else(|| components.parent_scope.clone())
        } else if components
            .parent_scope
            .as_ref()
            .is_some_and(|ps| ps == &qualified_name)
        {
            // Clear parent_scope if it equals qualified_name (entity IS the module itself)
            None
        } else {
            components.parent_scope.clone()
        };

        // Regenerate entity_id if qualified_name changed (due to template expansion).
        // This ensures unique entity_ids for entities like trait impl methods that have
        // different qualified names but the same AST-derived scope.
        let entity_id = if qualified_name != components.qualified_name {
            let file_path_str = ctx
                .file_path
                .to_str()
                .ok_or_else(|| Error::entity_extraction("Invalid file path"))?;
            generate_entity_id(
                ctx.repository_id,
                file_path_str,
                &qualified_name,
                &entity_type.to_string(),
            )
        } else {
            components.entity_id.clone()
        };

        let components = CommonEntityComponents {
            entity_id,
            qualified_name,
            parent_scope,
            ..components
        };

        // Extract metadata using the configured extractor
        let metadata =
            extract_metadata(config.metadata_extractor, main_node, ctx.source, &captures);

        // Extract relationships using the configured extractor.
        // For Property entities without explicit relationship extractors, we fall back
        // to ExtractTypeRelationships to capture type annotations as TypeUsage references.
        let mut relationships = extract_relationships_with_fallback(
            config.relationship_extractor,
            entity_type,
            main_node,
            ctx,
            Some(components.qualified_name.as_str()),
        );

        // For Rust trait impl methods, generate call aliases for UFCS resolution.
        // This allows `Type::method` to resolve to `<Type as Trait>::method`.
        // We use the pre-computed qualified_impl_type from captures rather than
        // parsing the qualified_name string, which is more robust.
        if ctx.language_str == "rust" && entity_type == EntityType::Method {
            if let Some(ref impl_type) = qualified_impl_type {
                relationships
                    .call_aliases
                    .push(format!("{impl_type}::{name}"));
            }
        }

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
                // Known limitation: signature extraction not yet implemented.
                // Function signatures are available via `content` field for now.
                signature: None,
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
    entity_type: EntityType,
    skip_scopes: Option<&'static [&'static str]>,
) -> Result<CommonEntityComponents> {
    use crate::qualified_name::{
        build_qualified_name_from_ast, build_qualified_name_with_skip,
        derive_module_path_for_language,
    };
    use codesearch_core::entities::SourceLocation;
    use codesearch_core::entity_id::generate_entity_id;

    if name.is_empty() {
        return Err(Error::entity_extraction("Empty name provided".to_string()));
    }

    // Build qualified name via parent traversal using language-specific separator
    // If skip_scopes is provided, skip those node kinds when building the scope
    let scope_result = if let Some(skip_kinds) = skip_scopes {
        build_qualified_name_with_skip(main_node, ctx.source, ctx.language_str, skip_kinds)
    } else {
        build_qualified_name_from_ast(main_node, ctx.source, ctx.language_str)
    };
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

    // Generate entity_id from repository + file_path + qualified name + entity_type
    let file_path_str = ctx
        .file_path
        .to_str()
        .ok_or_else(|| Error::entity_extraction("Invalid file path".to_string()))?;
    let entity_type_str = entity_type.to_string();
    let entity_id = generate_entity_id(
        ctx.repository_id,
        file_path_str,
        &qualified_name,
        &entity_type_str,
    );

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

/// Extract where clause from an impl_item node for blanket impl qualified names.
///
/// Handles both:
/// - Inline bounds: `impl<T: Debug> Printable for T` -> extracts from type_parameters
/// - Explicit where: `impl<T> Printable for T where T: Debug` -> extracts from where_clause
///
/// Returns formatted bounds like `[("T", ["Debug", "Clone"])]` which can be used
/// to build a where clause suffix like `where T: Debug + Clone`.
fn extract_type_parameter_bounds<'a>(
    impl_node: tree_sitter::Node<'a>,
    source: &'a str,
    import_map: &crate::common::import_map::ImportMap,
    package_name: Option<&str>,
    parent_scope: Option<&str>,
) -> Vec<(String, Vec<String>)> {
    fn node_text<'a>(node: tree_sitter::Node<'a>, source: &'a str) -> &'a str {
        node.utf8_text(source.as_bytes()).unwrap_or("")
    }

    fn resolve_trait_name(
        trait_name: &str,
        import_map: &crate::common::import_map::ImportMap,
        package_name: Option<&str>,
        parent_scope: Option<&str>,
    ) -> String {
        // Check if it's a well-known std trait or primitive
        if crate::rust::edge_case_handlers::is_std_type(trait_name) {
            return trait_name.to_string();
        }

        // Try import map resolution
        if let Some(resolved) = import_map.resolve(trait_name) {
            if let Some(stripped) = resolved.strip_prefix("crate::") {
                if let Some(pkg) = package_name {
                    return format!("{pkg}::{stripped}");
                }
            }
            return resolved.to_string();
        }

        // Prepend parent scope
        if let Some(scope) = parent_scope {
            return format!("{scope}::{trait_name}");
        }

        trait_name.to_string()
    }

    /// Extract type name and trait bounds from a node (where_predicate or type_parameter).
    /// Returns (type_name, trait_bounds) if valid bounds are found.
    fn extract_bounds_from_node(
        node: tree_sitter::Node,
        source: &str,
        import_map: &crate::common::import_map::ImportMap,
        package_name: Option<&str>,
        parent_scope: Option<&str>,
    ) -> Option<(String, Vec<String>)> {
        let mut type_name: Option<String> = None;
        let mut trait_bounds: Vec<String> = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_identifier" && type_name.is_none() {
                type_name = Some(node_text(child, source).to_string());
            } else if child.kind() == "trait_bounds" {
                let mut bounds_cursor = child.walk();
                for bound_child in child.children(&mut bounds_cursor) {
                    if bound_child.kind() == "type_identifier" {
                        let trait_text = node_text(bound_child, source);
                        let qualified =
                            resolve_trait_name(trait_text, import_map, package_name, parent_scope);
                        trait_bounds.push(qualified);
                    } else if bound_child.kind() == "generic_type" {
                        // Handle generic trait like Iterator<Item = T>
                        if let Some(type_node) = bound_child.child_by_field_name("type") {
                            let trait_text = node_text(type_node, source);
                            let qualified = resolve_trait_name(
                                trait_text,
                                import_map,
                                package_name,
                                parent_scope,
                            );
                            trait_bounds.push(qualified);
                        }
                    }
                }
            }
        }

        type_name
            .filter(|_| !trait_bounds.is_empty())
            .map(|t| (t, trait_bounds))
    }

    let mut bounds: Vec<(String, Vec<String>)> = Vec::new();

    // First check for explicit where_clause
    let mut cursor = impl_node.walk();
    for child in impl_node.children(&mut cursor) {
        if child.kind() == "where_clause" {
            let mut pred_cursor = child.walk();
            for predicate in child.children(&mut pred_cursor) {
                if predicate.kind() == "where_predicate" {
                    if let Some(bound) = extract_bounds_from_node(
                        predicate,
                        source,
                        import_map,
                        package_name,
                        parent_scope,
                    ) {
                        bounds.push(bound);
                    }
                }
            }
            return bounds;
        }
    }

    // No where_clause, check for inline bounds in type_parameters
    let mut cursor = impl_node.walk();
    for child in impl_node.children(&mut cursor) {
        if child.kind() == "type_parameters" {
            let mut param_cursor = child.walk();
            for type_param in child.children(&mut param_cursor) {
                if type_param.kind() == "type_parameter" {
                    if let Some(bound) = extract_bounds_from_node(
                        type_param,
                        source,
                        import_map,
                        package_name,
                        parent_scope,
                    ) {
                        bounds.push(bound);
                    }
                }
            }
        }
    }

    bounds
}

/// Format type parameter bounds as a where clause suffix.
///
/// Takes bounds like `[("T", ["Debug", "Clone"])]` and returns
/// `" where T: Debug + Clone"` or empty string if no bounds.
fn format_where_clause(bounds: &[(String, Vec<String>)]) -> String {
    if bounds.is_empty() {
        return String::new();
    }

    let clauses: Vec<String> = bounds
        .iter()
        .map(|(type_name, traits)| format!("{}: {}", type_name, traits.join(" + ")))
        .collect();

    format!(" where {}", clauses.join(", "))
}

/// Resolve impl_type_name to a fully qualified name using captures, scope, and import map.
///
/// Resolution order:
/// 1. If impl_type_path was captured (e.g., `mod::Type`), use path::type_name
/// 2. If impl_type_name is a well-known std/primitive type, don't prefix
/// 3. If import_map can resolve the type name (e.g., imported via `use crate::types::Widget`),
///    use the resolved path
/// 4. If parent_scope already ends with the type name (method context), use scope as-is
/// 5. Otherwise (impl block context), prepend the module-level scope
/// 6. If no scope available, use the simple type name
///
/// Returns `None` if impl_type_name is not present in captures.
fn resolve_impl_type_name(
    captures: &HashMap<String, String>,
    parent_scope: Option<&str>,
    import_map: &crate::common::import_map::ImportMap,
    type_alias_map: &crate::rust::import_resolution::TypeAliasMap,
    package_name: Option<&str>,
) -> Option<String> {
    let impl_type_name = captures.get("impl_type_name")?;

    // Resolve through type alias chain to get canonical type name
    // e.g., Settings -> AppConfig -> Config -> RawConfig
    let canonical_type_name =
        crate::rust::import_resolution::resolve_type_alias_chain(impl_type_name, type_alias_map);

    let qualified = if let Some(impl_type_path) = captures.get("impl_type_path") {
        // Case 1: Scoped type like `mod::Type`
        format!("{impl_type_path}::{canonical_type_name}")
    } else if crate::rust::edge_case_handlers::is_std_type(&canonical_type_name) {
        // Case 2: Well-known std/primitive type - don't add scope prefix
        canonical_type_name
    } else if let Some(resolved) = import_map.resolve(&canonical_type_name) {
        // Case 3: Type was imported (e.g., `use crate::types::Widget`)
        // The resolved path may start with "crate::" which needs to be replaced
        // with the actual package name
        if let Some(stripped) = resolved.strip_prefix("crate::") {
            if let Some(pkg) = package_name {
                format!("{pkg}::{stripped}")
            } else {
                resolved.to_string()
            }
        } else {
            resolved.to_string()
        }
    } else if let Some(scope) = parent_scope {
        if scope.ends_with(&format!("::{canonical_type_name}")) || scope == canonical_type_name {
            // Case 4: Method context - scope already is the qualified type name
            scope.to_string()
        } else if scope.ends_with(&format!("::{impl_type_name}")) {
            // Case 5a: Scope ends with the original alias name - replace it with canonical
            // e.g., "test_crate::Settings" + RawConfig -> "test_crate::RawConfig"
            let prefix_len = scope.len() - impl_type_name.len();
            format!("{}{canonical_type_name}", &scope[..prefix_len])
        } else if scope == impl_type_name {
            // Case 5b: Scope IS the original alias name - use canonical directly
            canonical_type_name
        } else {
            // Case 5c: Impl block context - prepend module scope
            format!("{scope}::{canonical_type_name}")
        }
    } else {
        // Case 6: No scope available
        canonical_type_name
    };

    Some(qualified)
}

/// Context for template expansion, bundling all resolution-related parameters.
struct TemplateExpansionContext<'a> {
    captures: &'a HashMap<String, String>,
    components: &'a CommonEntityComponents,
    import_map: &'a crate::common::import_map::ImportMap,
    type_alias_map: &'a crate::rust::import_resolution::TypeAliasMap,
    package_name: Option<&'a str>,
    /// Optional node for where clause extraction (only needed for impl blocks)
    main_node: Option<tree_sitter::Node<'a>>,
    /// Optional source for where clause extraction (only needed for impl blocks)
    source: Option<&'a str>,
}

/// Expand a qualified name template with captures and common components
fn expand_qualified_name_template(template: &str, ctx: &TemplateExpansionContext) -> String {
    let mut result = template.to_string();

    // Replace engine-provided placeholders
    if let Some(ref scope) = ctx.components.parent_scope {
        result = result.replace("{scope}", scope);
    } else {
        result = result.replace("{scope}::", "");
        result = result.replace("{scope}.", "");
        result = result.replace("{scope}", "");
    }

    // Replace {name} with the entity's derived name
    result = result.replace("{name}", &ctx.components.name);

    // Resolve {impl_type_name} to a fully qualified name. This ensures templates
    // like `<{impl_type_name}>::{name}` produce `<my_crate::MyStruct>::foo`
    // instead of just `<MyStruct>::foo`.
    if let Some(qualified_impl_type) = resolve_impl_type_name(
        ctx.captures,
        ctx.components.parent_scope.as_deref(),
        ctx.import_map,
        ctx.type_alias_map,
        ctx.package_name,
    ) {
        result = result.replace("{impl_type_name}", &qualified_impl_type);
    }

    // Resolve {trait_name} to a fully qualified name for trait impls.
    // This ensures `<Type as {trait_name}>::method` produces `<Type as my_crate::Trait>::method`
    if let Some(trait_name) = ctx.captures.get("trait_name") {
        let qualified_trait = if let Some(trait_path) = ctx.captures.get("trait_path") {
            // Case 1: Scoped trait like `mod::Trait`
            format!("{trait_path}::{trait_name}")
        } else if let Some(resolved) = ctx.import_map.resolve(trait_name) {
            // Case 2: Trait was imported - resolve through import map
            if let Some(stripped) = resolved.strip_prefix("crate::") {
                if let Some(pkg) = ctx.package_name {
                    format!("{pkg}::{stripped}")
                } else {
                    resolved.to_string()
                }
            } else {
                resolved.to_string()
            }
        } else if let Some(ref scope) = ctx.components.parent_scope {
            // Case 3: Simple trait name - prepend module scope
            // Find the module-level scope (everything before the type in the scope)
            // For methods, scope is like "crate::Type", for impl blocks it's "crate"
            let module_scope = scope
                .rsplit_once("::")
                .map(|(prefix, _)| prefix)
                .unwrap_or(scope.as_str());
            format!("{module_scope}::{trait_name}")
        } else {
            // Case 4: No scope available
            trait_name.clone()
        };
        result = result.replace("{trait_name}", &qualified_trait);
    }

    // Replace remaining capture placeholders (skip those with special handling above)
    for (capture_name, value) in ctx.captures {
        if capture_name == "impl_type_name"
            || capture_name == "impl_type_path"
            || capture_name == "trait_name"
            || capture_name == "trait_path"
        {
            continue;
        }
        result = result.replace(&format!("{{{capture_name}}}"), value);
    }

    // For trait impl blocks (templates like `<{impl_type_name} as {trait_name}>`),
    // append where clause for blanket impls with type parameter bounds.
    // Only applies to impl block templates (not method templates which have `::` suffix).
    if template.starts_with("<{impl_type_name} as {trait_name}>") && !template.contains("::") {
        if let (Some(node), Some(src)) = (ctx.main_node, ctx.source) {
            // Only process impl_item nodes
            if node.kind() == "impl_item" {
                let bounds = extract_type_parameter_bounds(
                    node,
                    src,
                    ctx.import_map,
                    ctx.package_name,
                    ctx.components.parent_scope.as_deref(),
                );
                let where_clause = format_where_clause(&bounds);
                result.push_str(&where_clause);
            }
        }
    }

    result
}

/// Derive parent scope from a qualified name by removing the entity name suffix.
///
/// For trait impl methods like `<Type as Trait>::method`, returns `<Type as Trait>`.
/// For regular qualified names like `module::Type::method`, returns `module::Type`.
///
/// Returns None when:
/// - The qualified_name equals the entity_name (no parent in the FQN structure)
/// - The qualified_name doesn't end with a standard `::name` or `.name` suffix
///   containing the entity_name (e.g., impl blocks where the qualified_name IS
///   the full signature `<Type as Trait>`, not a suffix-based derivation)
///
/// The caller should fall back to AST-derived parent_scope when this returns None.
fn derive_parent_from_qualified_name(qualified_name: &str, entity_name: &str) -> Option<String> {
    // Find the suffix pattern (separator + entity_name)
    let suffix_patterns = [format!("::{entity_name}"), format!(".{entity_name}")];

    for suffix in &suffix_patterns {
        if qualified_name.ends_with(suffix) {
            let parent = &qualified_name[..qualified_name.len() - suffix.len()];
            if parent.is_empty() {
                tracing::trace!(
                    qualified_name = qualified_name,
                    entity_name = entity_name,
                    "Parent derivation yielded empty string"
                );
                return None;
            }
            return Some(parent.to_string());
        }
    }

    // No suffix match - return None to signal caller should use AST-derived parent.
    // This handles:
    // - Module entities where qualified_name == name
    // - Impl blocks where qualified_name is the full type signature without a parent suffix
    //   (e.g., `<Type as Trait>` - the parent is the module, not derivable from FQN)
    tracing::trace!(
        qualified_name = qualified_name,
        entity_name = entity_name,
        "Could not derive parent - qualified_name doesn't end with expected suffix"
    );
    None
}

/// Evaluate custom predicates for a query match.
///
/// Tree-sitter's built-in predicates (`#eq?`, `#match?`, etc.) are evaluated automatically,
/// but custom predicates like `#not-has-child?` and `#not-has-ancestor?` need manual evaluation.
///
/// Supported custom predicates:
/// - `#not-has-child? @capture field_name` - node must NOT have a child with the given field name
/// - `#not-has-ancestor? @capture node_kind` - node must NOT have an ancestor of the given kind
fn evaluate_custom_predicates(
    query: &Query,
    query_match: &QueryMatch,
    config: &HandlerConfig,
    captures: &HashMap<String, String>,
) -> bool {
    let pattern_index = query_match.pattern_index;
    let predicates = query.general_predicates(pattern_index);

    for predicate in predicates {
        let operator = predicate.operator.as_ref();

        match operator {
            "not-has-child?" => {
                if !evaluate_not_has_child(predicate.args.as_ref(), query_match) {
                    return false;
                }
            }
            "not-has-ancestor?" => {
                if !evaluate_not_has_ancestor(predicate.args.as_ref(), query_match) {
                    return false;
                }
            }
            // Ignore other predicates (they may be handled elsewhere or be informational)
            _ => {}
        }
    }

    // Additional check: handlers that expect a trait_name capture but didn't get one
    // This handles cases where the query has alternative patterns and only some match
    if config.query.contains("trait:") && !captures.contains_key("trait_name") {
        return false;
    }

    true
}

/// Parse arguments for binary predicates like `#not-has-child?` and `#not-has-ancestor?`.
///
/// Binary predicates expect exactly 2 arguments: `@capture string_value`
/// Returns `Some((node, string_arg))` if valid, `None` otherwise.
fn parse_binary_predicate_args<'a, 'b>(
    args: &'b [QueryPredicateArg],
    query_match: &'a QueryMatch,
    predicate_name: &str,
) -> Option<(Node<'a>, &'b str)> {
    if args.len() != 2 {
        tracing::warn!(
            "{predicate_name} predicate expects 2 arguments, got {}",
            args.len()
        );
        return None;
    }

    let capture_index = match &args[0] {
        QueryPredicateArg::Capture(idx) => *idx,
        _ => {
            tracing::warn!("{predicate_name} first argument must be a capture");
            return None;
        }
    };

    let string_arg = match &args[1] {
        QueryPredicateArg::String(s) => s.as_ref(),
        _ => {
            tracing::warn!("{predicate_name} second argument must be a string");
            return None;
        }
    };

    // Find the captured node
    let node = query_match
        .captures
        .iter()
        .find(|c| c.index == capture_index)
        .map(|c| c.node)?;

    Some((node, string_arg))
}

/// Evaluate `#not-has-child? @capture child_spec` predicate.
///
/// The child_spec can be either:
/// - A field name (e.g., "trait") - checks `node.child_by_field_name()`
/// - A node kind (e.g., "self_parameter") - checks for any child of that kind
///
/// Returns true if the captured node does NOT have the specified child.
fn evaluate_not_has_child(args: &[QueryPredicateArg], query_match: &QueryMatch) -> bool {
    let Some((node, child_spec)) = parse_binary_predicate_args(args, query_match, "not-has-child?")
    else {
        // Invalid or missing args - reject match to avoid false positives
        tracing::warn!("not-has-child? predicate has invalid args, rejecting match");
        return false;
    };

    // First try as a field name (e.g., "trait" field on impl_item)
    if node.child_by_field_name(child_spec).is_some() {
        return false; // Has the child field, predicate NOT satisfied
    }

    // Then check for any child of the specified node kind (e.g., "self_parameter")
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == child_spec {
            return false; // Has a child of the specified kind, predicate NOT satisfied
        }
    }

    // Node does NOT have the specified child
    true
}

/// Evaluate `#not-has-ancestor? @capture node_kind` predicate.
///
/// Returns true if the captured node does NOT have an ancestor of the specified kind.
fn evaluate_not_has_ancestor(args: &[QueryPredicateArg], query_match: &QueryMatch) -> bool {
    let Some((node, node_kind)) =
        parse_binary_predicate_args(args, query_match, "not-has-ancestor?")
    else {
        // Invalid or missing args - reject match to avoid false positives
        tracing::warn!("not-has-ancestor? predicate has invalid args, rejecting match");
        return false;
    };

    // Return true if the node does NOT have an ancestor of the specified kind
    find_ancestor_of_kind(node, node_kind).is_none()
}

/// Find an ancestor node of a specific kind
fn find_ancestor_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.kind() == kind {
            return Some(parent);
        }
        current = parent;
    }
    None
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
            "function_signature_item", // For extern block function declarations
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
    // Helper function to extract visibility from accessibility_modifier
    fn extract_from_accessibility_modifier(node: Node) -> Option<Visibility> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "accessibility_modifier" => {
                    if let Some(first_child) = child.child(0) {
                        return match first_child.kind() {
                            "private" => Some(Visibility::Private),
                            "protected" => Some(Visibility::Protected),
                            "public" => Some(Visibility::Public),
                            _ => None,
                        };
                    }
                }
                // Check for # private field syntax (class fields)
                "private_property_identifier" => return Some(Visibility::Private),
                _ => {}
            }
        }
        None
    }

    // First pass: handle nodes with their own visibility rules (before export check)
    // These types have explicit visibility modifiers that should take precedence
    match node.kind() {
        // Module (program) nodes are implicitly public (can be imported)
        // export_statement nodes are explicitly public
        "program" | "export_statement" => return Some(Visibility::Public),

        // Parameter properties (TypeScript constructor parameters with accessibility modifiers)
        "required_parameter" | "optional_parameter" => {
            if let Some(vis) = extract_from_accessibility_modifier(node) {
                return Some(vis);
            }
            // Parameter properties without explicit modifier default to public
            return Some(Visibility::Public);
        }

        // Class members: check for TypeScript accessibility modifiers BEFORE export check
        // A private/protected member inside an exported class should NOT be public
        "public_field_definition" | "field_definition" | "method_definition" => {
            if let Some(vis) = extract_from_accessibility_modifier(node) {
                return Some(vis);
            }
            // Default: public for class members without explicit modifier
            return Some(Visibility::Public);
        }

        // Interface members are always public
        "property_signature"
        | "method_signature"
        | "call_signature"
        | "construct_signature"
        | "index_signature" => return Some(Visibility::Public),

        // Enum members inherit visibility from parent enum (always public within enum)
        "enum_assignment" => return Some(Visibility::Public),

        // property_identifier in enum context is public
        "property_identifier" => {
            if let Some(parent) = node.parent() {
                if matches!(parent.kind(), "enum_body" | "enum_assignment") {
                    return Some(Visibility::Public);
                }
            }
        }

        _ => {}
    }

    // Check for ambient declarations (declare keyword) - these are public
    // Ambient declarations describe external APIs and are always accessible
    if is_ambient_declaration(node) {
        return Some(Visibility::Public);
    }

    // Check if inside a namespace - items in namespaces have their own export rules
    // Only the immediate export_statement matters, not the namespace's own export
    if is_inside_namespace(node) {
        // For namespace items, check for immediate export_statement parent
        // (not grandparent - that would be the namespace's export)
        if let Some(parent) = node.parent() {
            if parent.kind() == "export_statement" {
                return Some(Visibility::Public);
            }
        }
        // Not exported from namespace - private
        return Some(Visibility::Private);
    }

    // For non-namespace items, check if this node has an export_statement ancestor
    // This handles cases like: export const named = function() {}
    // where the structure is: export_statement -> lexical_declaration -> variable_declarator -> function_expression
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "export_statement" {
            return Some(Visibility::Public);
        }
        // Stop at program/module level to avoid excessive traversal
        if parent.kind() == "program" {
            break;
        }
        current = parent.parent();
    }

    // For JS/TS module-level declarations, check if we're at module level
    // (parent is program) - these are Private by default
    if let Some(parent) = node.parent() {
        if parent.kind() == "program" {
            return Some(Visibility::Private);
        }
    }

    None
}

/// Check if a node is an ambient declaration (has 'declare' modifier)
fn is_ambient_declaration(node: Node) -> bool {
    const AMBIENT_TYPES: &[&str] = &[
        "ambient_declaration",
        "ambient_class_declaration",
        "ambient_function_declaration",
        "ambient_variable_declaration",
    ];

    // Check node or parent for ambient type
    if AMBIENT_TYPES.contains(&node.kind()) {
        return true;
    }
    if node
        .parent()
        .is_some_and(|p| AMBIENT_TYPES.contains(&p.kind()))
    {
        return true;
    }

    // For declarations, check for "declare" keyword as first child
    const DECL_TYPES: &[&str] = &[
        "lexical_declaration",
        "variable_declaration",
        "function_declaration",
        "class_declaration",
    ];
    if DECL_TYPES.contains(&node.kind()) && node.child(0).is_some_and(|c| c.kind() == "declare") {
        return true;
    }

    false
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
            return parse_visibility_modifier(child, source);
        }
    }

    // For tuple struct fields: the visibility modifier is a preceding sibling
    // within the ordered_field_declaration_list, not a child of the type node
    if let Some(parent) = node.parent() {
        if parent.kind() == "ordered_field_declaration_list" {
            // Look for a visibility_modifier that immediately precedes this type node
            if let Some(prev) = node.prev_sibling() {
                if prev.kind() == "visibility_modifier" {
                    return parse_visibility_modifier(prev, source);
                }
            }
        }
    }

    None
}

/// Parse a visibility_modifier node into a Visibility value
fn parse_visibility_modifier(node: Node, source: &str) -> Option<Visibility> {
    if let Ok(text) = node_to_text(node, source) {
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
                // Known limitation: generic parameter details not extracted.
                // We track is_generic=true but don't parse individual params yet.
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
                // Known limitation: generic parameter details not extracted.
                // We track is_generic=true but don't parse individual params yet.
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
            // Known limitation: generic parameter details not extracted.
            // We track is_generic=true but don't parse individual params yet.
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
            // Known limitation: generic parameter details not extracted.
            // We track is_generic=true but don't parse individual params yet.
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

    #[test]
    fn test_extract_type_parameter_bounds_inline() {
        let source = "impl<T: Debug> Foo for T {}";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).expect("parse failed");

        // Find the impl_item node
        let impl_node = tree.root_node().child(0).expect("expected impl_item node");
        assert_eq!(impl_node.kind(), "impl_item");

        let import_map = crate::common::import_map::ImportMap::new("::");
        let bounds = extract_type_parameter_bounds(impl_node, source, &import_map, None, None);

        assert_eq!(bounds.len(), 1);
        assert_eq!(bounds[0].0, "T");
        assert_eq!(bounds[0].1, vec!["Debug"]);
    }

    #[test]
    fn test_extract_type_parameter_bounds_where_clause() {
        let source = "impl<T> Foo for T where T: Clone {}";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).expect("parse failed");

        let impl_node = tree.root_node().child(0).expect("expected impl_item node");
        assert_eq!(impl_node.kind(), "impl_item");

        let import_map = crate::common::import_map::ImportMap::new("::");
        let bounds = extract_type_parameter_bounds(impl_node, source, &import_map, None, None);

        assert_eq!(bounds.len(), 1);
        assert_eq!(bounds[0].0, "T");
        assert_eq!(bounds[0].1, vec!["Clone"]);
    }

    #[test]
    fn test_extract_type_parameter_bounds_multiple() {
        let source = "impl<T: Debug + Clone> Foo for T {}";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).expect("parse failed");

        let impl_node = tree.root_node().child(0).expect("expected impl_item node");

        let import_map = crate::common::import_map::ImportMap::new("::");
        let bounds = extract_type_parameter_bounds(impl_node, source, &import_map, None, None);

        assert_eq!(bounds.len(), 1);
        assert_eq!(bounds[0].0, "T");
        assert!(bounds[0].1.contains(&"Debug".to_string()));
        assert!(bounds[0].1.contains(&"Clone".to_string()));
    }

    #[test]
    fn test_extract_type_parameter_bounds_no_bounds() {
        let source = "impl<T> Foo for T {}";
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let tree = parser.parse(source, None).expect("parse failed");

        let impl_node = tree.root_node().child(0).expect("expected impl_item node");

        let import_map = crate::common::import_map::ImportMap::new("::");
        let bounds = extract_type_parameter_bounds(impl_node, source, &import_map, None, None);

        assert!(bounds.is_empty());
    }

    #[test]
    fn test_resolve_impl_type_name_with_path() {
        let mut captures = HashMap::new();
        captures.insert("impl_type_name".to_string(), "Widget".to_string());
        captures.insert("impl_type_path".to_string(), "crate::ui".to_string());

        let import_map = crate::common::import_map::ImportMap::new("::");
        let type_alias_map = crate::rust::import_resolution::TypeAliasMap::new();

        let result = resolve_impl_type_name(
            &captures,
            Some("test_crate"),
            &import_map,
            &type_alias_map,
            Some("test_crate"),
        );

        assert_eq!(result, Some("crate::ui::Widget".to_string()));
    }

    #[test]
    fn test_resolve_impl_type_name_std_type() {
        let mut captures = HashMap::new();
        captures.insert("impl_type_name".to_string(), "String".to_string());

        let import_map = crate::common::import_map::ImportMap::new("::");
        let type_alias_map = crate::rust::import_resolution::TypeAliasMap::new();

        let result = resolve_impl_type_name(
            &captures,
            Some("test_crate"),
            &import_map,
            &type_alias_map,
            Some("test_crate"),
        );

        // String is a std type, should not be prefixed
        assert_eq!(result, Some("String".to_string()));
    }

    #[test]
    fn test_resolve_impl_type_name_with_scope() {
        let mut captures = HashMap::new();
        captures.insert("impl_type_name".to_string(), "MyStruct".to_string());

        let import_map = crate::common::import_map::ImportMap::new("::");
        let type_alias_map = crate::rust::import_resolution::TypeAliasMap::new();

        let result = resolve_impl_type_name(
            &captures,
            Some("test_crate::module"),
            &import_map,
            &type_alias_map,
            Some("test_crate"),
        );

        // Should prepend scope
        assert_eq!(result, Some("test_crate::module::MyStruct".to_string()));
    }

    #[test]
    fn test_resolve_impl_type_name_with_type_alias() {
        let mut captures = HashMap::new();
        captures.insert("impl_type_name".to_string(), "Settings".to_string());

        let import_map = crate::common::import_map::ImportMap::new("::");
        let mut type_alias_map = crate::rust::import_resolution::TypeAliasMap::new();
        type_alias_map.insert("Settings".to_string(), "Config".to_string());
        type_alias_map.insert("Config".to_string(), "RawConfig".to_string());

        let result = resolve_impl_type_name(
            &captures,
            Some("test_crate"),
            &import_map,
            &type_alias_map,
            Some("test_crate"),
        );

        // Should resolve through the alias chain to RawConfig
        assert_eq!(result, Some("test_crate::RawConfig".to_string()));
    }

    #[test]
    fn test_resolve_impl_type_name_none_without_capture() {
        let captures = HashMap::new(); // no impl_type_name

        let import_map = crate::common::import_map::ImportMap::new("::");
        let type_alias_map = crate::rust::import_resolution::TypeAliasMap::new();

        let result = resolve_impl_type_name(
            &captures,
            Some("test_crate"),
            &import_map,
            &type_alias_map,
            Some("test_crate"),
        );

        assert_eq!(result, None);
    }
}
