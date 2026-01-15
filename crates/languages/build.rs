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
    let queries_dir = Path::new("queries");

    // Track which spec files we depend on
    println!("cargo:rerun-if-changed=specs/");
    println!("cargo:rerun-if-changed=queries/");

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

    // Process .scm query files (new system for V2 architecture)
    if queries_dir.exists() {
        for entry in fs::read_dir(queries_dir).expect("Failed to read queries directory") {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();

            if path.is_dir() {
                let lang_name = path
                    .file_name()
                    .expect("No file name")
                    .to_str()
                    .expect("Invalid UTF-8 in filename");

                println!("cargo:rerun-if-changed={}", path.display());

                if let Err(e) = process_scm_queries(&path, lang_name, &out_dir) {
                    panic!("Failed to process scm queries in {}: {}", path.display(), e);
                }
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

// =============================================================================
// .scm Query File Processing (V2 Architecture)
// =============================================================================

/// Parsed handler definition from .scm annotations
#[derive(Debug, Default)]
struct ScmHandlerDef {
    /// Handler name (e.g., "rust::free_function") - REQUIRED
    handler: Option<String>,
    /// Entity type (e.g., "Function", "Method") - REQUIRED
    entity_type: Option<String>,
    /// Primary capture name from the query - REQUIRED
    capture: Option<String>,
    /// Optional description
    description: Option<String>,
    /// The tree-sitter query string - REQUIRED
    query: String,
}

impl ScmHandlerDef {
    /// Validate that all required fields are present
    fn validate(&self, file: &Path, line_num: usize) -> Result<(), String> {
        if self.handler.is_none() {
            return Err(format!(
                "{}:{}: missing @handler annotation",
                file.display(),
                line_num
            ));
        }
        if self.entity_type.is_none() {
            return Err(format!(
                "{}:{}: missing @entity_type annotation for handler '{}'",
                file.display(),
                line_num,
                self.handler.as_ref().unwrap()
            ));
        }
        if self.capture.is_none() {
            return Err(format!(
                "{}:{}: missing @capture annotation for handler '{}'",
                file.display(),
                line_num,
                self.handler.as_ref().unwrap()
            ));
        }
        if self.query.trim().is_empty() {
            return Err(format!(
                "{}:{}: empty query for handler '{}'",
                file.display(),
                line_num,
                self.handler.as_ref().unwrap()
            ));
        }
        Ok(())
    }
}

/// Parser state for .scm files
struct ScmParser {
    handlers: Vec<ScmHandlerDef>,
    current: ScmHandlerDef,
    current_start_line: usize,
    in_query: bool,
}

impl ScmParser {
    fn new() -> Self {
        Self {
            handlers: Vec::new(),
            current: ScmHandlerDef::default(),
            current_start_line: 0,
            in_query: false,
        }
    }

    /// Emit the current handler if it has content
    fn emit_current(&mut self, file: &Path, line_num: usize) -> Result<(), String> {
        if self.current.handler.is_some() || !self.current.query.trim().is_empty() {
            self.current.validate(file, self.current_start_line)?;
            self.handlers.push(std::mem::take(&mut self.current));
        }
        self.in_query = false;
        self.current_start_line = line_num;
        Ok(())
    }

    /// Parse an annotation line (starts with "; @")
    fn parse_annotation(&mut self, line: &str, file: &Path, line_num: usize) -> Result<(), String> {
        let content = line.trim().strip_prefix("; @").unwrap();

        // Split into annotation name and value
        let (name, value) = if let Some(space_pos) = content.find(' ') {
            let (n, v) = content.split_at(space_pos);
            (n, v.trim())
        } else {
            (content, "")
        };

        match name {
            "handler" => {
                // New handler starts - emit previous if any
                self.emit_current(file, line_num)?;
                self.current.handler = Some(value.to_string());
            }
            "entity_type" => {
                self.current.entity_type = Some(value.to_string());
            }
            "capture" => {
                self.current.capture = Some(value.to_string());
            }
            "description" => {
                self.current.description = Some(value.to_string());
            }
            unknown => {
                return Err(format!(
                    "{}:{}: unknown annotation '@{}'",
                    file.display(),
                    line_num,
                    unknown
                ));
            }
        }
        Ok(())
    }

    /// Parse a single line
    fn parse_line(&mut self, line: &str, file: &Path, line_num: usize) -> Result<(), String> {
        let trimmed = line.trim();

        if trimmed.starts_with("; @") {
            // Annotation line
            self.parse_annotation(line, file, line_num)?;
        } else if trimmed.starts_with(';') || trimmed.is_empty() {
            // Comment or blank line - ignore (but don't break query accumulation)
        } else {
            // Query content
            self.in_query = true;
            self.current.query.push_str(line);
            self.current.query.push('\n');
        }
        Ok(())
    }

    /// Finalize parsing and return handlers
    fn finalize(mut self, file: &Path, line_num: usize) -> Result<Vec<ScmHandlerDef>, String> {
        self.emit_current(file, line_num)?;
        Ok(self.handlers)
    }
}

/// Parse a .scm file extracting queries and their handler annotations
///
/// Expected format:
/// ```scheme
/// ; @handler rust::free_function
/// ; @entity_type Function
/// ; @capture func
/// ; @description Free functions at module level
/// ((function_item
///   name: (identifier) @name
/// ) @func
/// (#not-has-ancestor? @func impl_item))
/// ```
fn parse_scm_file(path: &Path) -> Result<Vec<ScmHandlerDef>, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let mut parser = ScmParser::new();

    for (line_num, line) in content.lines().enumerate() {
        parser
            .parse_line(line, path, line_num + 1)
            .map_err(Box::<dyn std::error::Error>::from)?;
    }

    let handlers = parser
        .finalize(path, content.lines().count())
        .map_err(Box::<dyn std::error::Error>::from)?;

    Ok(handlers)
}

/// Process all .scm files in a language directory
fn process_scm_queries(
    lang_dir: &Path,
    lang_name: &str,
    out_dir: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut all_handlers: Vec<ScmHandlerDef> = Vec::new();

    // Find all .scm files in the language directory
    for entry in fs::read_dir(lang_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "scm") {
            println!("cargo:rerun-if-changed={}", path.display());
            let handlers = parse_scm_file(&path)?;
            all_handlers.extend(handlers);
        }
    }

    if all_handlers.is_empty() {
        return Ok(());
    }

    // Generate output file
    let output_path = Path::new(out_dir).join(format!("{lang_name}_scm_queries.rs"));
    let mut output = fs::File::create(&output_path)?;

    writeln!(output, "// Auto-generated from {lang_name}/*.scm files")?;
    writeln!(output, "// DO NOT EDIT - changes will be overwritten")?;
    writeln!(output)?;

    // Generate query constants module
    writeln!(
        output,
        "/// Tree-sitter queries parsed from .scm files for {lang_name}"
    )?;
    writeln!(output, "pub mod scm_queries {{")?;

    for h in &all_handlers {
        let handler_name = h.handler.as_ref().unwrap();
        let const_name = handler_to_const_name(handler_name);
        if let Some(ref desc) = h.description {
            writeln!(output, "    /// {desc}")?;
        } else {
            writeln!(output, "    /// Query for handler: {handler_name}")?;
        }
        writeln!(output, "    pub const {const_name}: &str = r#\"")?;
        writeln!(output, "{}", h.query.trim())?;
        writeln!(output, "\"#;")?;
        writeln!(output)?;
    }

    writeln!(output, "}}")?;
    writeln!(output)?;

    // Generate handler metadata module
    writeln!(output, "/// Handler metadata for {lang_name}")?;
    writeln!(output, "pub mod scm_handlers {{")?;
    writeln!(output, "    use super::scm_queries;")?;
    writeln!(output)?;
    writeln!(output, "    /// Complete handler definition from .scm file")?;
    writeln!(output, "    #[derive(Debug, Clone, Copy)]")?;
    writeln!(output, "    pub struct ScmHandler {{")?;
    writeln!(
        output,
        "        /// Handler name (e.g., \"rust::free_function\")"
    )?;
    writeln!(output, "        pub handler_name: &'static str,")?;
    writeln!(output, "        /// Entity type (e.g., \"Function\")")?;
    writeln!(output, "        pub entity_type: &'static str,")?;
    writeln!(output, "        /// Primary capture name")?;
    writeln!(output, "        pub capture: &'static str,")?;
    writeln!(output, "        /// The tree-sitter query")?;
    writeln!(output, "        pub query: &'static str,")?;
    writeln!(output, "    }}")?;
    writeln!(output)?;
    writeln!(output, "    /// All handler definitions for {lang_name}")?;
    writeln!(output, "    pub const ALL_HANDLERS: &[ScmHandler] = &[")?;

    for h in &all_handlers {
        let handler_name = h.handler.as_ref().unwrap();
        let entity_type = h.entity_type.as_ref().unwrap();
        let capture = h.capture.as_ref().unwrap();
        let const_name = handler_to_const_name(handler_name);
        writeln!(output, "        ScmHandler {{")?;
        writeln!(output, "            handler_name: \"{handler_name}\",")?;
        writeln!(output, "            entity_type: \"{entity_type}\",")?;
        writeln!(output, "            capture: \"{capture}\",")?;
        writeln!(output, "            query: scm_queries::{const_name},")?;
        writeln!(output, "        }},")?;
    }

    writeln!(output, "    ];")?;
    writeln!(output, "}}")?;

    Ok(())
}

/// Convert a handler name to a const name (e.g., "rust::free_function" -> "RUST_FREE_FUNCTION")
fn handler_to_const_name(handler: &str) -> String {
    handler.replace("::", "_").to_uppercase()
}
