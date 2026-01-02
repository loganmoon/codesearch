//! Handler for extracting TypeScript module definitions
//!
//! This module processes tree-sitter query matches for TypeScript program nodes
//! and builds Module entities with import/export tracking.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use crate::common::{
    entity_building::{build_entity, CommonEntityComponents, EntityDetails},
    module_utils::derive_module_name,
    node_to_text, require_capture_node,
};
use crate::javascript::module_path::derive_module_path;
use codesearch_core::{
    entities::{
        EntityMetadata, EntityRelationshipData, EntityType, Language, ReferenceType,
        SourceLocation, SourceReference, Visibility,
    },
    entity_id::generate_entity_id,
    error::Result,
    CodeEntity,
};
use std::path::Path;
use std::sync::OnceLock;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor, QueryMatch};

// Cached tree-sitter query for export detection
static TS_EXPORT_QUERY: OnceLock<Option<Query>> = OnceLock::new();

const TS_EXPORT_QUERY_SOURCE: &str = r#"
    (export_statement) @export
"#;

/// Get or initialize the cached export query
fn ts_export_query() -> Option<&'static Query> {
    TS_EXPORT_QUERY
        .get_or_init(|| {
            let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            match Query::new(&language, TS_EXPORT_QUERY_SOURCE) {
                Ok(query) => Some(query),
                Err(e) => {
                    tracing::error!(
                        "Failed to compile TypeScript export query: {e}. This is a bug."
                    );
                    None
                }
            }
        })
        .as_ref()
}

/// Check if a TypeScript program node has any export statements
fn has_exports(program_node: Node, source: &str) -> bool {
    let Some(query) = ts_export_query() else {
        return false;
    };

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, program_node, source.as_bytes());

    // If there's at least one export, return true
    matches.next().is_some()
}

/// Represents an imported item with its source and local name
#[derive(Debug)]
struct ImportInfo {
    /// The source module path (e.g., "./utils")
    source_path: String,
    /// The name being imported (or None for namespace import of the whole module)
    imported_name: Option<String>,
    /// Whether this is a namespace import (import * as X)
    is_namespace: bool,
}

/// Represents a re-exported item
#[derive(Debug)]
struct ReexportInfo {
    /// The source module path (e.g., "./user")
    source_path: String,
    /// The name being re-exported (or None for star export)
    exported_name: Option<String>,
}

/// Extract detailed import information from a TypeScript program node
fn extract_imports(program_node: Node, source: &str) -> Vec<ImportInfo> {
    let mut imports = Vec::new();

    for child in program_node.children(&mut program_node.walk()) {
        if child.kind() != "import_statement" {
            continue;
        }

        // Find the source string (e.g., './utils')
        let source_path = child
            .children(&mut child.walk())
            .find(|n| n.kind() == "string")
            .and_then(|n| {
                n.utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
            });

        let Some(source_path) = source_path else {
            continue;
        };

        // Find the import_clause
        let import_clause = child
            .children(&mut child.walk())
            .find(|n| n.kind() == "import_clause");

        let Some(clause) = import_clause else {
            continue;
        };

        // Process import_clause children
        for clause_child in clause.children(&mut clause.walk()) {
            match clause_child.kind() {
                // Default import: `import DefaultClass from ...`
                "identifier" => {
                    // Use the local binding name as a best-guess for the export name
                    // This works when local name matches export name (common convention)
                    if let Ok(local_name) = clause_child.utf8_text(source.as_bytes()) {
                        imports.push(ImportInfo {
                            source_path: source_path.clone(),
                            imported_name: Some(local_name.to_string()),
                            is_namespace: false,
                        });
                    }
                }
                // Named imports: `import { helper, VALUE } from ...`
                "named_imports" => {
                    for specifier in clause_child.children(&mut clause_child.walk()) {
                        if specifier.kind() == "import_specifier" {
                            // Get the imported name (first identifier in specifier)
                            if let Some(name_node) = specifier
                                .children(&mut specifier.walk())
                                .find(|n| n.kind() == "identifier")
                            {
                                if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                                    imports.push(ImportInfo {
                                        source_path: source_path.clone(),
                                        imported_name: Some(name.to_string()),
                                        is_namespace: false,
                                    });
                                }
                            }
                        }
                    }
                }
                // Namespace import: `import * as Utils from ...`
                "namespace_import" => {
                    imports.push(ImportInfo {
                        source_path: source_path.clone(),
                        imported_name: None, // Imports the whole module
                        is_namespace: true,
                    });
                }
                _ => {}
            }
        }
    }

    imports
}

/// Extract re-export information from a TypeScript program node
fn extract_reexports(program_node: Node, source: &str) -> Vec<ReexportInfo> {
    let mut reexports = Vec::new();

    for child in program_node.children(&mut program_node.walk()) {
        if child.kind() != "export_statement" {
            continue;
        }

        // Check if this is a re-export (has "from" keyword)
        let has_from = child
            .children(&mut child.walk())
            .any(|n| n.kind() == "from");
        if !has_from {
            continue;
        }

        // Find the source string
        let source_path = child
            .children(&mut child.walk())
            .find(|n| n.kind() == "string")
            .and_then(|n| {
                n.utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
            });

        let Some(source_path) = source_path else {
            continue;
        };

        // Check for star export: `export * from './module'`
        let has_star = child.children(&mut child.walk()).any(|n| n.kind() == "*");
        if has_star {
            reexports.push(ReexportInfo {
                source_path,
                exported_name: None, // Star export
            });
            continue;
        }

        // Check for named re-exports: `export { User } from './user'`
        let export_clause = child
            .children(&mut child.walk())
            .find(|n| n.kind() == "export_clause");

        if let Some(clause) = export_clause {
            for specifier in clause.children(&mut clause.walk()) {
                if specifier.kind() == "export_specifier" {
                    // Get the exported name (first identifier)
                    if let Some(name_node) = specifier
                        .children(&mut specifier.walk())
                        .find(|n| n.kind() == "identifier")
                    {
                        if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                            reexports.push(ReexportInfo {
                                source_path: source_path.clone(),
                                exported_name: Some(name.to_string()),
                            });
                        }
                    }
                }
            }
        }
    }

    reexports
}

/// Resolve a relative import path to a module qualified name
///
/// Example: "./utils" in file "src/main.ts" with source_root "src" -> "utils"
/// Example: "./models/user" in file "src/index.ts" -> "models.user"
fn resolve_import_path(
    import_path: &str,
    current_file: &Path,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Option<String> {
    // Only handle relative imports for now
    if !import_path.starts_with('.') {
        return None;
    }

    // Get the directory of the current file
    let current_dir = current_file.parent()?;

    // Resolve the relative path
    let mut resolved = current_dir.to_path_buf();
    for component in import_path.split('/') {
        match component {
            "." => {}
            ".." => {
                resolved.pop();
            }
            name => {
                resolved.push(name);
            }
        }
    }

    // Try to derive module path from the resolved path
    // Add .ts extension for resolution
    let with_ext = resolved.with_extension("ts");

    source_root
        .and_then(|root| derive_module_path(&with_ext, root))
        .or_else(|| derive_module_path(&with_ext, repo_root))
}

/// Handle TypeScript program node as a Module entity
#[allow(clippy::too_many_arguments)]
pub fn handle_module_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let program_node = require_capture_node(query_match, query, "module")?;

    // Extract module name from file path
    let name = derive_module_name(file_path);

    // Build qualified name from file path using derive_module_path
    // This correctly handles index.ts files as directory modules (models/index.ts -> "models")
    let qualified_name = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root))
        .unwrap_or_else(|| name.clone());

    // Build path_entity_identifier (repo-relative path for import resolution)
    let path_entity_identifier =
        crate::common::module_utils::derive_path_entity_identifier(file_path, repo_root, ".");

    // Generate entity ID
    let file_path_str = file_path.to_string_lossy();
    let entity_id = generate_entity_id(repository_id, &file_path_str, &qualified_name);

    // Get location
    let location = SourceLocation::from_tree_sitter_node(program_node);

    // Create components
    let components = CommonEntityComponents {
        entity_id,
        repository_id: repository_id.to_string(),
        name,
        qualified_name: qualified_name.clone(),
        path_entity_identifier: Some(path_entity_identifier),
        parent_scope: None,
        file_path: file_path.to_path_buf(),
        location,
    };

    // Extract imports and re-exports
    let import_infos = extract_imports(program_node, source);
    let reexport_infos = extract_reexports(program_node, source);

    // Check for exports
    let has_export_statements = has_exports(program_node, source);

    // Only create a Module entity if there are imports or exports
    // Per E-MOD-FILE: "A file with import/export statements produces a Module entity"
    if import_infos.is_empty() && !has_export_statements {
        return Ok(vec![]);
    }

    // Build relationships
    let mut relationships = EntityRelationshipData::default();

    // Convert imports to SourceReference
    for import in &import_infos {
        let Some(module_qname) =
            resolve_import_path(&import.source_path, file_path, source_root, repo_root)
        else {
            continue;
        };

        // Determine the target qualified name
        let (target, simple_name) = if import.is_namespace {
            // Namespace import: target is the module itself
            (module_qname.clone(), module_qname.clone())
        } else if let Some(ref name) = import.imported_name {
            // Named import or default import: target is module.name
            // For default imports, we use the local binding name as best-guess
            (format!("{module_qname}.{name}"), name.clone())
        } else {
            continue;
        };

        if let Ok(src_ref) = SourceReference::builder()
            .target(target)
            .simple_name(simple_name)
            .is_external(false)
            .location(SourceLocation::default())
            .ref_type(ReferenceType::Import)
            .build()
        {
            relationships.imports.push(src_ref);
        }
    }

    // Convert re-exports to SourceReference
    for reexport in &reexport_infos {
        let Some(module_qname) =
            resolve_import_path(&reexport.source_path, file_path, source_root, repo_root)
        else {
            continue;
        };

        if let Some(ref name) = reexport.exported_name {
            // Named re-export: target is module.name
            let target = format!("{module_qname}.{name}");
            if let Ok(src_ref) = SourceReference::builder()
                .target(target)
                .simple_name(name.clone())
                .is_external(false)
                .location(SourceLocation::default())
                .ref_type(ReferenceType::Import) // Using Import for re-exports
                .build()
            {
                relationships.reexports.push(src_ref);
            }
        } else {
            // Star re-export: target is the source module itself
            // We create a REEXPORTS relationship to the module, which semantically
            // means "this module re-exports everything from the target module"
            if let Ok(src_ref) = SourceReference::builder()
                .target(module_qname.clone())
                .simple_name(
                    module_qname
                        .rsplit('.')
                        .next()
                        .unwrap_or(&module_qname)
                        .to_string(),
                )
                .is_external(false)
                .location(SourceLocation::default())
                .ref_type(ReferenceType::Import)
                .build()
            {
                relationships.reexports.push(src_ref);
            }
        }
    }

    // Build metadata (keep legacy format for backwards compatibility)
    let mut metadata = EntityMetadata::default();
    let import_paths: Vec<String> = import_infos.iter().map(|i| i.source_path.clone()).collect();
    if let Ok(imports_json) = serde_json::to_string(&import_paths) {
        metadata
            .attributes
            .insert("imports".to_string(), imports_json);
    }

    // Build the entity
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Module,
            language: Language::TypeScript,
            visibility: Some(Visibility::Public),
            documentation: None,
            content: node_to_text(program_node, source).ok(),
            metadata,
            signature: None,
            relationships,
        },
    )?;

    Ok(vec![entity])
}

/// Handle TypeScript namespace declaration as a Module entity
/// NOTE: Currently disabled - causes timeout issues
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub fn handle_namespace_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    _package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let namespace_node = require_capture_node(query_match, query, "namespace")?;
    let name_node = require_capture_node(query_match, query, "name")?;

    // Extract namespace name
    let name = node_to_text(name_node, source)?;

    // Derive module path from file
    let module_path = source_root
        .and_then(|root| derive_module_path(file_path, root))
        .or_else(|| derive_module_path(file_path, repo_root));

    // Build qualified name from AST (includes parent namespace scope)
    let scope_result =
        crate::qualified_name::build_qualified_name_from_ast(namespace_node, source, "typescript");

    // Compose full qualified name: module.parent_namespace.name
    let full_qualified_name = match (&module_path, scope_result.parent_scope.is_empty()) {
        (Some(module), false) => format!("{module}.{}.{name}", scope_result.parent_scope),
        (Some(module), true) => format!("{module}.{name}"),
        (None, false) => format!("{}.{name}", scope_result.parent_scope),
        (None, true) => name.clone(),
    };

    // Parent scope includes module path
    let parent_scope = match (&module_path, scope_result.parent_scope.is_empty()) {
        (Some(module), false) => Some(format!("{module}.{}", scope_result.parent_scope)),
        (Some(module), true) => Some(module.clone()),
        (None, false) => Some(scope_result.parent_scope.clone()),
        (None, true) => None,
    };

    // Check if exported
    let is_exported = is_namespace_exported(namespace_node);

    // Generate entity ID
    let file_path_str = file_path.to_string_lossy();
    let entity_id = generate_entity_id(repository_id, &file_path_str, &full_qualified_name);

    // Get location
    let location = SourceLocation::from_tree_sitter_node(namespace_node);

    // Create components
    let components = CommonEntityComponents {
        entity_id,
        repository_id: repository_id.to_string(),
        name,
        qualified_name: full_qualified_name,
        path_entity_identifier: None,
        parent_scope,
        file_path: file_path.to_path_buf(),
        location,
    };

    // Build the entity
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Module,
            language: Language::TypeScript,
            visibility: Some(if is_exported {
                Visibility::Public
            } else {
                Visibility::Private
            }),
            documentation: None,
            content: node_to_text(namespace_node, source).ok(),
            metadata: EntityMetadata::default(),
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

/// Check if a namespace is exported (has an export_statement ancestor)
#[allow(dead_code)]
fn is_namespace_exported(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "export_statement" {
            return true;
        }
        current = parent.parent();
    }
    false
}
