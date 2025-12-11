//! Integration test for TSG extraction evaluation on the codesearch codebase
//!
//! Run with: cargo test -p codesearch-languages --test tsg_evaluation_test -- --nocapture

use codesearch_languages::tsg::{
    build_intra_file_edges, categorize_unresolved, EvaluationResult, ResolutionNode,
    ResolutionNodeKind, TsgExecutor,
};
use std::collections::HashMap;
use std::path::Path;
use walkdir::WalkDir;

/// Detailed evaluation results including error files and unresolved names
struct DetailedEvaluation {
    result: EvaluationResult,
    error_count: usize,
    error_files: Vec<String>,
    unresolved_names: HashMap<String, usize>,
    #[allow(dead_code)]
    all_nodes: Vec<ResolutionNode>,
}

/// Evaluate TSG extraction and intra-file resolution on a directory
#[allow(dead_code)]
fn evaluate_directory(dir: &Path) -> (EvaluationResult, usize) {
    let detailed = evaluate_directory_detailed(dir);
    (detailed.result, detailed.error_count)
}

/// Detailed evaluation with tracking of errors and unresolved references
fn evaluate_directory_detailed(dir: &Path) -> DetailedEvaluation {
    let mut executor = TsgExecutor::new_rust().unwrap();
    let mut result = EvaluationResult::new();
    let mut error_count = 0;
    let mut error_files = Vec::new();
    let mut unresolved_names: HashMap<String, usize> = HashMap::new();
    let mut all_nodes = Vec::new();

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "rs")
                && !e.path().to_string_lossy().contains("/target/")
        })
    {
        let file_path = entry.path();
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                error_count += 1;
                error_files.push(format!("{}: read error: {e}", file_path.display()));
                continue;
            }
        };

        let nodes = match executor.extract(&source, file_path) {
            Ok(n) => n,
            Err(e) => {
                error_count += 1;
                error_files.push(format!("{}: extraction error: {e}", file_path.display()));
                continue;
            }
        };

        result.total_files += 1;
        result.total_nodes += nodes.len();

        for node in &nodes {
            match node.kind {
                ResolutionNodeKind::Definition => result.definition_count += 1,
                ResolutionNodeKind::Export => result.export_count += 1,
                ResolutionNodeKind::Import => result.import_count += 1,
                ResolutionNodeKind::Reference => result.reference_count += 1,
            }
        }

        let (resolved, unresolved) = build_intra_file_edges(&nodes);
        result.intra_file_resolved += resolved;

        for unresolved_ref in &unresolved {
            result.unresolved += 1;
            let category = categorize_unresolved(unresolved_ref);
            *result
                .unresolved_by_pattern
                .entry(category.to_string())
                .or_insert(0) += 1;
            *unresolved_names
                .entry(unresolved_ref.name.clone())
                .or_insert(0) += 1;
        }

        all_nodes.extend(nodes);
    }

    result.compute_rate();

    DetailedEvaluation {
        result,
        error_count,
        error_files,
        unresolved_names,
        all_nodes,
    }
}

#[test]
fn test_evaluate_codesearch_codebase() {
    // Get the crates directory
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    println!("\nEvaluating TSG extraction on codesearch crates...\n");

    let detailed = evaluate_directory_detailed(crates_dir);
    let result = &detailed.result;

    println!("=== TSG Extraction & Resolution Evaluation ===\n");
    println!("Files processed: {}", result.total_files);
    println!("Parse/extraction errors: {}", detailed.error_count);
    println!("Total nodes extracted: {}", result.total_nodes);
    println!();
    println!("Node counts by type:");
    println!("  Definitions: {}", result.definition_count);
    println!("  Exports: {}", result.export_count);
    println!("  Imports: {}", result.import_count);
    println!("  References: {}", result.reference_count);
    println!();
    println!("Intra-file resolution:");
    println!("  Resolved: {}", result.intra_file_resolved);
    println!("  Unresolved: {}", result.unresolved);
    println!(
        "  Resolution rate: {:.1}%",
        result.intra_file_resolution_rate * 100.0
    );
    println!();

    if !result.unresolved_by_pattern.is_empty() {
        println!("Unresolved by category:");
        let mut categories: Vec<_> = result.unresolved_by_pattern.iter().collect();
        categories.sort_by(|a, b| b.1.cmp(a.1));
        for (category, count) in categories {
            let pct = if result.unresolved > 0 {
                (*count as f64 / result.unresolved as f64) * 100.0
            } else {
                0.0
            };
            println!("  {category}: {count} ({pct:.1}%)");
        }
    }

    // Show top unresolved names
    if !detailed.unresolved_names.is_empty() {
        println!("\nTop 20 unresolved references:");
        let mut names: Vec<_> = detailed.unresolved_names.iter().collect();
        names.sort_by(|a, b| b.1.cmp(a.1));
        for (name, count) in names.iter().take(20) {
            println!("  {name}: {count}");
        }
    }

    // Show error files
    if !detailed.error_files.is_empty() {
        println!("\nFirst 10 error files:");
        for err in detailed.error_files.iter().take(10) {
            println!("  {err}");
        }
    }

    // Basic sanity checks
    assert!(result.total_files > 0, "Should process at least one file");
    assert!(result.total_nodes > 0, "Should extract at least one node");
    assert!(
        result.definition_count > 0,
        "Should extract at least one definition"
    );
    assert!(
        result.import_count > 0,
        "Should extract at least one import"
    );
    assert!(
        result.reference_count > 0,
        "Should extract at least one reference"
    );

    // Print target status
    let target_rate = 0.80;
    if result.intra_file_resolution_rate >= target_rate {
        println!(
            "\nSUCCESS: Achieved {:.1}% resolution rate (target: {:.0}%)",
            result.intra_file_resolution_rate * 100.0,
            target_rate * 100.0
        );
    } else {
        println!(
            "\nPROGRESS: {:.1}% resolution rate (target: {:.0}%)",
            result.intra_file_resolution_rate * 100.0,
            target_rate * 100.0
        );
        println!(
            "Need to resolve {} more references to hit target.",
            ((target_rate * result.reference_count as f64) as usize)
                .saturating_sub(result.intra_file_resolved)
        );
    }
}

#[test]
fn test_sample_unresolved_references() {
    // Extract from a sample file and show unresolved references
    let mut executor = TsgExecutor::new_rust().unwrap();

    let source = r#"
use std::collections::HashMap;
use anyhow::Result;

pub struct MyStruct {
    data: HashMap<String, i32>,
}

impl MyStruct {
    pub fn new() -> Result<Self> {
        Ok(Self {
            data: HashMap::new(),
        })
    }

    pub fn process(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }
}

fn helper() -> Option<MyStruct> {
    MyStruct::new().ok()
}
"#;

    let nodes = executor.extract(source, Path::new("sample.rs")).unwrap();

    println!("\n=== Sample File Analysis ===\n");

    println!("Definitions:");
    for node in nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Definition)
    {
        println!(
            "  {} ({})",
            node.name,
            node.definition_kind.as_deref().unwrap_or("?")
        );
    }

    println!("\nImports:");
    for node in nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Import)
    {
        println!(
            "  {} <- {}",
            node.name,
            node.import_path.as_deref().unwrap_or("?")
        );
    }

    println!("\nReferences:");
    for node in nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Reference)
    {
        println!(
            "  {} (context: {})",
            node.name,
            node.reference_context.as_deref().unwrap_or("?")
        );
    }

    let (resolved, unresolved) = build_intra_file_edges(&nodes);
    println!(
        "\nResolution: {} resolved, {} unresolved",
        resolved,
        unresolved.len()
    );

    if !unresolved.is_empty() {
        println!("\nUnresolved references:");
        for node in &unresolved {
            println!(
                "  {} (line {}, context: {})",
                node.name,
                node.start_line,
                node.reference_context.as_deref().unwrap_or("?")
            );
        }
    }
}

/// Known external crate prefixes
const EXTERNAL_CRATE_PREFIXES: &[&str] = &[
    "std::",
    "core::",
    "alloc::",
    "tokio::",
    "async_trait::",
    "serde::",
    "serde_json::",
    "uuid::",
    "anyhow::",
    "thiserror::",
    "tracing::",
    "neo4rs::",
    "sqlx::",
    "qdrant_client::",
    "reqwest::",
    "axum::",
    "tower::",
    "tower_http::",
    "hyper::",
    "bytes::",
    "futures::",
    "tempfile::",
    "walkdir::",
    "notify::",
    "tree_sitter::",
    "tree_sitter_graph::",
    "criterion::",
    "streaming_iterator::",
    "im::",
    "bm25::",
    "inventory::",
    "bollard::",
    "chrono::",
    "dotenvy::",
    "once_cell::",
    "testcontainers::",
    "testcontainers_modules::",
    "utoipa::",
    "utoipa_swagger_ui::",
    "twox_hash::",
    "fs2::",
    "regex::",
    "dashmap::",
    "ignore::",
    "proc_macro::",
    "ordered_float::",
    "schemars::",
    "quote::",
    "syn::",
    "moka::",
    "unicode_segmentation::",
    "derive_builder::",
    "git2::",
    "dialoguer::",
    "strum_macros::",
    "clap::",
    "rmcp::",
    "glob::",
    "async_openai::",
    "config::",
];

/// Known external crate names (without ::) for detecting qualified references like `chrono::Utc`
#[allow(dead_code)]
const EXTERNAL_CRATE_NAMES: &[&str] = &[
    "std",
    "core",
    "alloc",
    "tokio",
    "async_trait",
    "serde",
    "serde_json",
    "uuid",
    "anyhow",
    "thiserror",
    "tracing",
    "neo4rs",
    "sqlx",
    "qdrant_client",
    "reqwest",
    "axum",
    "tower",
    "tower_http",
    "hyper",
    "bytes",
    "futures",
    "tempfile",
    "walkdir",
    "notify",
    "tree_sitter",
    "tree_sitter_graph",
    "criterion",
    "streaming_iterator",
    "im",
    "bm25",
    "inventory",
    "bollard",
    "chrono",
    "dotenvy",
    "once_cell",
    "testcontainers",
    "testcontainers_modules",
    "utoipa",
    "utoipa_swagger_ui",
    "twox_hash",
    "fs2",
    "regex",
    "dashmap",
    "ignore",
    "proc_macro",
    "ordered_float",
    "schemars",
    "quote",
    "syn",
    "moka",
    "unicode_segmentation",
    "derive_builder",
    "git2",
    "dialoguer",
    "strum_macros",
    "clap",
    "rmcp",
    "glob",
    "async_openai",
    "config",
];

/// Check if an import path is to an external crate (not our codebase)
fn is_external_import(path: &str) -> bool {
    // Internal crate imports start with these
    let internal_prefixes = [
        "crate::",
        "super::",
        "self::",
        // Our workspace crate names
        "codesearch_core::",
        "codesearch_languages::",
        "codesearch_storage::",
        "codesearch_embeddings::",
        "codesearch_indexer::",
        "codesearch_watcher::",
        "codesearch_cli::",
    ];

    for prefix in internal_prefixes {
        if path.starts_with(prefix) {
            return false; // Internal import
        }
    }

    for prefix in EXTERNAL_CRATE_PREFIXES {
        if path.starts_with(prefix) {
            return true;
        }
    }

    // If no matching prefix, assume external (most bare paths are external crates)
    // unless it starts with a lowercase letter (could be local module)
    !path.chars().next().is_some_and(|c| c.is_lowercase())
}

/// Check if a reference name looks like it's from an external crate
/// This catches things like types from external crates that are used via qualified paths
fn is_likely_external_type(name: &str) -> bool {
    // Common external crate types that might not have explicit imports
    const EXTERNAL_TYPES: &[&str] = &[
        // chrono
        "Utc",
        "DateTime",
        "NaiveDateTime",
        "Duration",
        "TimeZone",
        // sqlx
        "Transaction",
        "Pool",
        "PgPool",
        "Row",
        "FromRow",
        // tokio
        "Sender",
        "Receiver",
        "JoinHandle",
        "Runtime",
        "TokioMutex",
        // std::fmt
        "Formatter",
        "Arguments",
        // serde
        "Serializer",
        "Deserializer",
        // tree-sitter
        "Node",
        "Tree",
        "Query",
        "QueryMatch",
        "QueryCursor",
        "Parser",
        // axum/http
        "HeaderValue",
        "StatusCode",
        "Response",
        "Request",
        "Json",
        "post",
        "get",
        "put",
        "delete",
        "patch",
        "handler",
        // qdrant
        "PointStruct",
        "ScoredPoint",
        "QdrantValue",
        "VectorParamsMap",
        "VectorsConfig",
        "SparseIndexConfig",
        // neo4rs
        "Graph",
        "BoltType",
        "Attributes",
        // External AI SDK types
        "Anthropic",
        "Model",
        // json/serde
        "Value",
        "Map",
        // uuid
        "Uuid",
        // notify
        "NotifyEvent",
        // testcontainers
        "Postgres",
        // im (immutable collections)
        "ImHashMap",
        // Common error types
        "ErrorData",
        // Other external types
        "ServerInfo",
        "Parse",
        "ParseStream",
        "Output",
        // rmcp
        "ToolRouter",
        "CallToolResult",
        // tokio_util
        "CancellationToken",
        // qdrant additional
        "Parameters",
        "SparseVectorParams",
        // serde_json
        "JsonValue",
        // grpc/tower
        "Implementation",
        // anyhow
        "WithContext",
        // tokenizers
        "SequenceTooLong",
    ];

    EXTERNAL_TYPES.contains(&name)
}

/// Cross-file resolution evaluation - simulates Neo4j resolution with in-memory lookups
#[test]
fn test_cross_file_resolution() {
    use std::collections::HashSet;

    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut executor = TsgExecutor::new_rust().unwrap();

    println!("\n=== Cross-File Resolution Evaluation ===\n");

    // Phase 1: Extract all nodes from all files
    let mut all_nodes: Vec<ResolutionNode> = Vec::new();
    let mut nodes_by_file: HashMap<String, Vec<ResolutionNode>> = HashMap::new();
    let mut file_count = 0;
    let mut error_count = 0;

    for entry in WalkDir::new(crates_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "rs")
                && !e.path().to_string_lossy().contains("/target/")
        })
    {
        let file_path = entry.path();
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(_) => {
                error_count += 1;
                continue;
            }
        };

        let nodes = match executor.extract(&source, file_path) {
            Ok(n) => n,
            Err(_) => {
                error_count += 1;
                continue;
            }
        };

        file_count += 1;
        let file_key = file_path.to_string_lossy().to_string();
        nodes_by_file.insert(file_key, nodes.clone());
        all_nodes.extend(nodes);
    }

    println!("Files processed: {file_count}");
    println!("Errors: {error_count}");

    // Phase 2: Build global definition lookup by qualified_name
    // This simulates what Neo4j Entity nodes provide
    let mut definitions_by_qname: HashMap<&str, &ResolutionNode> = HashMap::new();
    let mut definitions_by_name: HashMap<&str, Vec<&ResolutionNode>> = HashMap::new();
    let mut total_definitions = 0;

    for node in &all_nodes {
        if node.kind == ResolutionNodeKind::Definition {
            total_definitions += 1;
            definitions_by_qname.insert(&node.qualified_name, node);
            definitions_by_name
                .entry(&node.name)
                .or_default()
                .push(node);
        }
    }

    println!("Total definitions indexed: {total_definitions}");

    // Phase 3: For each file, evaluate resolution
    let mut total_imports = 0;
    let mut internal_imports = 0;
    let mut external_imports = 0;
    let mut internal_imports_resolved = 0;
    let mut glob_imports = 0;

    let mut total_references = 0;
    let mut resolved_via_local_definition = 0;
    let mut resolved_via_internal_import = 0;
    let mut resolved_via_external_import = 0;
    let mut resolved_via_glob_import = 0;
    let mut references_unresolved = 0;

    let mut unresolved_import_paths: HashMap<String, usize> = HashMap::new();
    let mut unresolved_reference_names: HashMap<String, usize> = HashMap::new();

    // Primitives and prelude types to skip
    let skip_names: HashSet<&str> = [
        // Underscore (unused pattern)
        "_",
        // Primitives
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "isize",
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "usize",
        "f32",
        "f64",
        "bool",
        "char",
        "str",
        "Self",
        "self",
        "super",
        "crate",
        // Common prelude types
        "Vec",
        "Option",
        "Result",
        "Some",
        "None",
        "Ok",
        "Err",
        "Box",
        "String",
        "HashMap",
        "HashSet",
        "BTreeMap",
        "BTreeSet",
        "Arc",
        "Rc",
        "Mutex",
        "RwLock",
        "RefCell",
        "Cell",
        "Pin",
        "Cow",
        "PhantomData",
        "Default",
        "Clone",
        "Copy",
        "Debug",
        "Display",
        "Error",
        "Send",
        "Sync",
        "Sized",
        "Drop",
        "Iterator",
        "IntoIterator",
        "FromIterator",
        "Extend",
        "PartialEq",
        "Eq",
        "PartialOrd",
        "Ord",
        "Hash",
        "From",
        "Into",
        "TryFrom",
        "TryInto",
        "AsRef",
        "AsMut",
        "Deref",
        "DerefMut",
        "Path",
        "PathBuf",
        // Single-letter identifiers (generics, parameters)
        "T",
        "U",
        "V",
        "K",
        "E",
        "F",
        "R",
        "S",
        "N",
        "M",
        "A",
        "B",
        "C",
        "D",
        "I",
        "O",
        "P",
        "Q",
        "W",
        "X",
        "Y",
        "Z",
        "a",
        "b",
        "c",
        "d",
        "e",
        "f",
        "g",
        "h",
        "i",
        "j",
        "k",
        "l",
        "m",
        "n",
        "o",
        "p",
        "q",
        "r",
        "s",
        "t",
        "u",
        "v",
        "w",
        "x",
        "y",
        "z",
        // Prelude functions
        "drop",
        "panic",
        "print",
        "println",
        "eprint",
        "eprintln",
        "dbg",
        "format",
        "vec",
        "todo",
        "unimplemented",
        "unreachable",
        // Fn traits (when used standalone)
        "Fn",
        "FnMut",
        "FnOnce",
        "FnPtr",
        // Common derive/attribute macros that appear as references
        "derive",
        "cfg",
        "test",
        "allow",
        "deny",
        "warn",
    ]
    .into_iter()
    .collect();

    for nodes in nodes_by_file.values() {
        // Build file-local lookups
        let local_definitions: HashSet<&str> = nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Definition)
            .map(|n| n.name.as_str())
            .collect();

        // Categorize imports by type
        let mut local_internal_imports: HashMap<&str, &ResolutionNode> = HashMap::new();
        let mut local_external_imports: HashSet<&str> = HashSet::new();
        let mut local_glob_imports: Vec<&ResolutionNode> = Vec::new();

        for node in nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Import)
        {
            total_imports += 1;

            if let Some(import_path) = &node.import_path {
                if node.is_glob {
                    glob_imports += 1;
                    local_glob_imports.push(node);
                } else if is_external_import(import_path) {
                    external_imports += 1;
                    local_external_imports.insert(&node.name);
                } else {
                    internal_imports += 1;
                    local_internal_imports.insert(&node.name, node);

                    // Check if internal import resolves
                    // For internal imports (crate::, super::, etc.), try:
                    // 1. Exact qualified_name match
                    // 2. Simple name match (since our qualified_names are simplified)
                    let resolved = definitions_by_qname.contains_key(import_path.as_str())
                        || definitions_by_name.contains_key(node.name.as_str());

                    if resolved {
                        internal_imports_resolved += 1;
                    } else {
                        *unresolved_import_paths
                            .entry(import_path.clone())
                            .or_insert(0) += 1;
                    }
                }
            }
        }

        // Count references and resolution
        for node in nodes
            .iter()
            .filter(|n| n.kind == ResolutionNodeKind::Reference)
        {
            // Skip primitives and prelude
            if skip_names.contains(node.name.as_str()) {
                continue;
            }

            // Skip known external types (like chrono::Utc used without explicit import)
            if is_likely_external_type(&node.name) {
                resolved_via_external_import += 1;
                total_references += 1;
                continue;
            }

            total_references += 1;

            // Try to resolve: local definitions first
            if local_definitions.contains(node.name.as_str()) {
                resolved_via_local_definition += 1;
            }
            // Then internal imports (these are what we want to resolve to FQNs)
            else if local_internal_imports.contains_key(node.name.as_str()) {
                resolved_via_internal_import += 1;
            }
            // External imports (these resolve to external crates - expected not in our index)
            else if local_external_imports.contains(node.name.as_str()) {
                resolved_via_external_import += 1;
            }
            // Check if might come from glob import (internal definitions)
            else if !local_glob_imports.is_empty()
                && definitions_by_name.contains_key(node.name.as_str())
            {
                resolved_via_glob_import += 1;
            }
            // Check if this is a re-exported type from our workspace crates
            // (e.g., PostgresClientTrait re-exported from codesearch_storage)
            else if definitions_by_name.contains_key(node.name.as_str()) {
                // Found a definition with this name somewhere in the codebase
                // This handles re-exports: use codesearch_storage::PostgresClientTrait
                resolved_via_internal_import += 1;
            } else {
                references_unresolved += 1;
                *unresolved_reference_names
                    .entry(node.name.clone())
                    .or_insert(0) += 1;
            }
        }
    }

    println!("\nImport breakdown:");
    println!("  Total imports: {total_imports}");
    println!("  Internal (crate) imports: {internal_imports}");
    println!("  External (std/deps) imports: {external_imports}");
    println!("  Glob imports: {glob_imports}");
    println!("\nInternal import resolution:");
    println!("  Resolved: {internal_imports_resolved}");
    if internal_imports > 0 {
        println!(
            "  Resolution rate: {:.1}%",
            (internal_imports_resolved as f64 / internal_imports as f64) * 100.0
        );
    }

    println!("\nReference resolution:");
    println!("  Total references (excluding primitives): {total_references}");
    println!("  Via local definition: {resolved_via_local_definition}");
    println!("  Via internal import: {resolved_via_internal_import}");
    println!("  Via external import: {resolved_via_external_import}");
    println!("  Via glob import: {resolved_via_glob_import}");
    println!("  Unresolved: {references_unresolved}");

    // Calculate resolution rates
    // "Resolvable" = references that can reach a definition (internal only)
    let resolvable_refs =
        resolved_via_local_definition + resolved_via_internal_import + resolved_via_glob_import;
    let resolvable_rate = if total_references > 0 {
        resolvable_refs as f64 / total_references as f64
    } else {
        0.0
    };

    // "Bound" = references that have some binding (including external)
    let bound_refs = resolvable_refs + resolved_via_external_import;
    let bound_rate = if total_references > 0 {
        bound_refs as f64 / total_references as f64
    } else {
        0.0
    };

    println!("\n=== Resolution Summary ===");
    println!(
        "  References bound (any import): {bound_refs} ({:.1}%)",
        bound_rate * 100.0
    );
    println!(
        "  References resolvable (internal): {resolvable_refs} ({:.1}%)",
        resolvable_rate * 100.0
    );
    println!("  Unbound references: {references_unresolved}");

    // Show top unresolved internal import paths
    if !unresolved_import_paths.is_empty() {
        println!("\nTop 15 unresolved INTERNAL import paths:");
        let mut paths: Vec<_> = unresolved_import_paths.iter().collect();
        paths.sort_by(|a, b| b.1.cmp(a.1));
        for (path, count) in paths.iter().take(15) {
            println!("  {path}: {count}");
        }
    }

    // Show top unresolved reference names
    if !unresolved_reference_names.is_empty() {
        println!("\nTop 20 unresolved reference names:");
        let mut names: Vec<_> = unresolved_reference_names.iter().collect();
        names.sort_by(|a, b| b.1.cmp(a.1));
        for (name, count) in names.iter().take(20) {
            println!("  {name}: {count}");
        }
    }

    // Target is 80% bound rate (references that have some import)
    let target_rate = 0.80;
    if bound_rate >= target_rate {
        println!(
            "\nSUCCESS: Achieved {:.1}% bound rate (target: {:.0}%)",
            bound_rate * 100.0,
            target_rate * 100.0
        );
    } else {
        println!(
            "\nPROGRESS: {:.1}% bound rate (target: {:.0}%)",
            bound_rate * 100.0,
            target_rate * 100.0
        );
        let needed = ((target_rate * total_references as f64) as usize).saturating_sub(bound_refs);
        println!("Need to bind {needed} more references to hit target.");
    }

    // Sanity assertions
    assert!(total_references > 0, "Should have references");
    assert!(total_definitions > 0, "Should have definitions");
}
