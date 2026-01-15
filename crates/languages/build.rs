//! Build script for spec-driven code generation
//!
//! Parses YAML language specs and generates:
//! - Query constants for tree-sitter queries
//! - Handler configurations for entity extraction

use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

// =============================================================================
// YAML Schema Types
// =============================================================================

#[derive(Debug, Deserialize)]
struct LanguageSpec {
    #[serde(default)]
    extraction_hints: Option<ExtractionHints>,
}

#[derive(Debug, Deserialize)]
struct ExtractionHints {
    #[serde(default)]
    queries: HashMap<String, QueryDef>,
    #[serde(default)]
    #[allow(dead_code)] // Will be used in Phase 5 for extraction engine
    extractors: HashMap<String, ExtractorDef>,
    #[serde(default)]
    handlers: HashMap<String, HandlerDef>,
}

#[derive(Debug, Deserialize)]
struct QueryDef {
    #[serde(default)]
    description: Option<String>,
    #[allow(dead_code)] // Parsed from YAML, will be used in Phase 5
    capture: String,
    query: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Will be used in Phase 5 for extraction engine
struct ExtractorDef {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    metadata_fields: Vec<serde_yaml::Value>,
    #[serde(default)]
    relationship_fields: Vec<serde_yaml::Value>,
}

#[derive(Debug, Deserialize)]
struct HandlerDef {
    entity_rule: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    capture: Option<String>,
    name_strategy: String,
    #[serde(default)]
    name_captures: Vec<String>,
    #[serde(default)]
    name_template: Option<String>,
    #[serde(default)]
    static_name: Option<String>,
    #[serde(default)]
    qualified_name_template: Option<String>,
    #[serde(default)]
    metadata: Option<String>,
    #[serde(default)]
    relationships: Option<String>,
    #[serde(default)]
    visibility_override: Option<serde_yaml::Value>,
}

// =============================================================================
// Code Generation
// =============================================================================

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let specs_dir = Path::new("specs");

    // Track which spec files we depend on
    println!("cargo:rerun-if-changed=specs/");

    // Process each language spec (YAML-based, existing system)
    for entry in fs::read_dir(specs_dir).expect("Failed to read specs directory") {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "yaml") {
            let lang_name = path
                .file_stem()
                .expect("No file stem")
                .to_str()
                .expect("Invalid UTF-8 in filename");

            println!("cargo:rerun-if-changed={}", path.display());

            if let Err(e) = process_spec(&path, lang_name, &out_dir) {
                panic!("Failed to process {}: {}", path.display(), e);
            }
        }
    }
}

fn process_spec(
    spec_path: &Path,
    lang_name: &str,
    out_dir: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = fs::read_to_string(spec_path)?;
    let spec: LanguageSpec = serde_yaml::from_str(&content)?;

    let Some(hints) = spec.extraction_hints else {
        // No extraction_hints section, nothing to generate
        return Ok(());
    };

    let output_path = Path::new(out_dir).join(format!("{lang_name}_generated.rs"));
    let mut output = fs::File::create(&output_path)?;

    writeln!(output, "// Auto-generated from {lang_name}.yaml")?;
    writeln!(output, "// DO NOT EDIT - changes will be overwritten")?;
    writeln!(output)?;

    // Generate query constants
    generate_queries(&mut output, &hints.queries, lang_name)?;

    // Generate handler configurations
    generate_handler_configs(&mut output, &hints.handlers, lang_name)?;

    Ok(())
}

fn generate_queries(
    output: &mut fs::File,
    queries: &HashMap<String, QueryDef>,
    lang_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    writeln!(output, "/// Tree-sitter queries for {lang_name}")?;
    writeln!(output, "pub mod queries {{")?;

    // Sort queries for deterministic output
    let mut query_names: Vec<_> = queries.keys().collect();
    query_names.sort();

    for name in query_names {
        let query_def = &queries[name];

        if let Some(ref desc) = query_def.description {
            writeln!(output, "    /// {desc}")?;
        }

        writeln!(output, "    pub const {name}: &str = r#\"")?;
        writeln!(output, "{}", query_def.query.trim())?;
        writeln!(output, "\"#;")?;
        writeln!(output)?;
    }

    writeln!(output, "}}")?;
    writeln!(output)?;

    Ok(())
}

fn generate_handler_configs(
    output: &mut fs::File,
    handlers: &HashMap<String, HandlerDef>,
    lang_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check if any handler uses visibility_override
    let needs_visibility = handlers.values().any(|h| h.visibility_override.is_some());

    writeln!(output, "/// Handler configurations for {lang_name}")?;
    writeln!(output, "pub mod handler_configs {{")?;
    writeln!(output, "    use super::queries;")?;
    writeln!(output, "    use crate::spec_driven::{{HandlerConfig, NameStrategy, MetadataExtractor, RelationshipExtractor}};")?;
    if needs_visibility {
        writeln!(output, "    use codesearch_core::entities::Visibility;")?;
    }
    writeln!(output)?;

    // Sort handlers for deterministic output
    let mut handler_names: Vec<_> = handlers.keys().collect();
    handler_names.sort();

    for name in &handler_names {
        let handler = &handlers[*name];

        // Skip handlers without queries (e.g., CrateRoot)
        let Some(ref query) = handler.query else {
            continue;
        };
        let capture = handler.capture.as_deref().unwrap_or("unknown");

        writeln!(output, "    /// Configuration for {name} handler")?;
        writeln!(
            output,
            "    pub const {}: HandlerConfig = HandlerConfig {{",
            to_screaming_snake(name)
        )?;
        writeln!(output, "        entity_rule: \"{}\",", handler.entity_rule)?;
        writeln!(output, "        query: queries::{query},")?;
        writeln!(output, "        capture: \"{capture}\",")?;
        writeln!(
            output,
            "        name_strategy: NameStrategy::{},",
            name_strategy_variant(
                &handler.name_strategy,
                &handler.name_captures,
                &handler.name_template,
                &handler.static_name
            )
        )?;

        if let Some(ref template) = handler.qualified_name_template {
            let escaped = escape_string(template);
            writeln!(
                output,
                "        qualified_name_template: Some(\"{escaped}\"),"
            )?;
        } else {
            writeln!(output, "        qualified_name_template: None,")?;
        }

        if let Some(ref metadata) = handler.metadata {
            writeln!(
                output,
                "        metadata_extractor: Some(MetadataExtractor::{}),",
                to_pascal_case(metadata)
            )?;
        } else {
            writeln!(output, "        metadata_extractor: None,")?;
        }

        if let Some(ref rel) = handler.relationships {
            writeln!(
                output,
                "        relationship_extractor: Some(RelationshipExtractor::{}),",
                to_pascal_case(rel)
            )?;
        } else {
            writeln!(output, "        relationship_extractor: None,")?;
        }

        if let Some(ref vis) = handler.visibility_override {
            let vis_str = match vis {
                serde_yaml::Value::String(s) => format!("Some(Visibility::{s})"),
                serde_yaml::Value::Null => "None".to_string(),
                _ => "None".to_string(),
            };
            writeln!(output, "        visibility_override: {vis_str},")?;
        } else {
            writeln!(output, "        visibility_override: None,")?;
        }

        writeln!(output, "    }};")?;
        writeln!(output)?;
    }

    // Generate the list of all handler configs (only those with queries)
    writeln!(output, "    /// All handler configurations for {lang_name}")?;
    writeln!(output, "    pub const ALL_HANDLERS: &[&HandlerConfig] = &[")?;
    for name in &handler_names {
        // Only include handlers that have queries (were generated above)
        if handlers[*name].query.is_some() {
            writeln!(output, "        &{},", to_screaming_snake(name))?;
        }
    }
    writeln!(output, "    ];")?;

    writeln!(output, "}}")?;
    writeln!(output)?;

    Ok(())
}

fn name_strategy_variant(
    strategy: &str,
    captures: &[String],
    template: &Option<String>,
    static_name: &Option<String>,
) -> String {
    match strategy {
        "capture" => "Capture { name: \"name\" }".to_string(),
        "fallback" => {
            let captures_str = captures
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Fallback {{ captures: &[{captures_str}] }}")
        }
        "template" => {
            let tmpl = template.as_deref().unwrap_or("");
            let escaped = escape_string(tmpl);
            format!("Template {{ template: \"{escaped}\" }}")
        }
        "static" => {
            let name = static_name.as_deref().unwrap_or("");
            let escaped = escape_string(name);
            format!("Static {{ name: \"{escaped}\" }}")
        }
        "file_path" => "FilePath".to_string(),
        "crate_name" => "CrateName".to_string(),
        "positional_index" => "PositionalIndex".to_string(),
        _ => format!("Capture {{ name: \"name\" }} /* unknown strategy: {strategy} */"),
    }
}

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn to_screaming_snake(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_ascii_uppercase());
    }
    result
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect()
}
