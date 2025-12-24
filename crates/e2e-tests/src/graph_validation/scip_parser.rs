//! SCIP (Source Code Intelligence Protocol) parsing for ground truth extraction
//!
//! SCIP symbols use a canonical format:
//! `<scheme> <manager> <package> <version> <descriptors>`
//!
//! Example: `rust-analyzer cargo anyhow 1.0.100 error/ErrorImpl#display().`
//!
//! Descriptor suffixes indicate entity kind:
//! - `/` = namespace/module
//! - `#` = type (struct, enum, trait)
//! - `.` = term (function, method, const)
//! - `!` = macro
//! - `()` = callable signature
//! - `[]` = type parameters

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use codesearch_core::entities::EntityType;
use scip::types::{Index, Occurrence, SymbolRole};

use super::models::{EntityRef, Relationship, RelationshipType};

/// Parsed components of a SCIP symbol
#[derive(Debug, Clone)]
pub struct ScipSymbol {
    /// The scheme (e.g., "rust-analyzer")
    pub scheme: String,
    /// Package manager (e.g., "cargo")
    pub manager: String,
    /// Package name (e.g., "anyhow")
    pub package: String,
    /// Package version (e.g., "1.0.100" or a URL for std lib)
    pub version: String,
    /// Descriptor path (e.g., "error/ErrorImpl#display().")
    pub descriptors: String,
    /// Rust-style qualified name (e.g., "anyhow::error::ErrorImpl::display")
    pub qualified_name: String,
    /// Inferred entity type from suffix
    pub entity_type: Option<EntityType>,
}

/// Parse a SCIP symbol string into its components
pub fn parse_scip_symbol(symbol: &str) -> Option<ScipSymbol> {
    // Format: "rust-analyzer cargo <package> <version> <descriptors>"
    // Version can be a simple string or a URL (for std lib)

    let parts: Vec<&str> = symbol.splitn(5, ' ').collect();
    if parts.len() < 5 {
        return None;
    }

    let scheme = parts[0].to_string();
    let manager = parts[1].to_string();
    let package = parts[2].to_string();
    let version = parts[3].to_string();
    let descriptors = parts[4].to_string();

    let qualified_name = descriptors_to_qualified_name(&package, &descriptors);
    let entity_type = entity_type_from_descriptors(&descriptors);

    Some(ScipSymbol {
        scheme,
        manager,
        package,
        version,
        descriptors,
        qualified_name,
        entity_type,
    })
}

/// Convert SCIP descriptors to Rust-style qualified name
///
/// Examples:
/// - `error/ErrorImpl#display().` -> `anyhow::error::ErrorImpl::display`
/// - `crate/` -> `anyhow`
/// - `impl#[`Error`]new().` -> `anyhow::Error::new` (inherent impl)
/// - `context/impl#[`ContextError<C, E>`][Display]fmt().` -> `anyhow::context::<ContextError as Display>::fmt`
fn descriptors_to_qualified_name(package: &str, descriptors: &str) -> String {
    let mut parts = Vec::new();

    // Start with package name
    parts.push(package.to_string());

    // Check for impl block pattern first
    if let Some(impl_result) = parse_impl_descriptor(descriptors) {
        // Add module path
        if !impl_result.module_path.is_empty() {
            parts.push(impl_result.module_path);
        }

        // Add the impl representation
        if let Some(trait_name) = impl_result.trait_name {
            // Trait impl: <Type as Trait>::method
            let type_name = clean_type_name(&impl_result.type_name);
            parts.push(format!("<{type_name} as {trait_name}>"));
        } else {
            // Inherent impl: Type::method
            parts.push(clean_type_name(&impl_result.type_name));
        }

        // Add method name if present
        if let Some(method) = impl_result.method_name {
            parts.push(method);
        }

        return parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("::");
    }

    // Non-impl path: parse segments normally
    let mut current = String::new();
    let mut in_brackets: i32 = 0;

    for c in descriptors.chars() {
        match c {
            '[' | '`' => {
                in_brackets += 1;
            }
            ']' => {
                in_brackets = in_brackets.saturating_sub(1);
            }
            '/' => {
                if in_brackets == 0 {
                    let segment = current.trim().to_string();
                    if !segment.is_empty() && segment != "crate" {
                        parts.push(segment);
                    }
                    current.clear();
                } else {
                    current.push(c);
                }
            }
            '#' | '.' => {
                if in_brackets == 0 {
                    let segment = current.trim().to_string();
                    if !segment.is_empty() {
                        parts.push(segment);
                    }
                    current.clear();
                } else {
                    current.push(c);
                }
            }
            '!' => {
                // Macro suffix - flush the macro name without the !
                // Codesearch stores macros without the ! suffix
                if in_brackets == 0 {
                    let segment = current.trim().to_string();
                    if !segment.is_empty() {
                        parts.push(segment);
                    }
                    current.clear();
                } else {
                    current.push(c);
                }
            }
            '(' | ')' => {
                if in_brackets > 0 {
                    current.push(c);
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    // Flush remaining
    let segment = current.trim().to_string();
    if !segment.is_empty() {
        parts.push(segment);
    }

    parts.join("::")
}

/// Parsed impl block descriptor
struct ImplDescriptor {
    module_path: String,
    type_name: String,
    trait_name: Option<String>,
    method_name: Option<String>,
}

/// Parse an impl block descriptor
///
/// Format: `module/impl#[`Type`][Trait]method().` or `module/impl#[`Type`]method().`
fn parse_impl_descriptor(descriptors: &str) -> Option<ImplDescriptor> {
    // Find impl# marker
    let impl_idx = descriptors.find("impl#")?;

    // Extract module path before impl#
    let module_path = descriptors[..impl_idx]
        .trim_end_matches('/')
        .replace('/', "::");

    let rest = &descriptors[impl_idx + 5..]; // Skip "impl#"

    // Parse [Type] and optionally [Trait]
    // Format: [`Type`][Trait]method(). or [`Type`]method().
    let mut bracket_depth = 0;
    let mut segments: Vec<String> = Vec::new();
    let mut current = String::new();

    for c in rest.chars() {
        match c {
            '[' => {
                if bracket_depth == 0 && !current.is_empty() {
                    segments.push(current.clone());
                    current.clear();
                }
                bracket_depth += 1;
            }
            ']' => {
                bracket_depth -= 1;
                if bracket_depth == 0 {
                    segments.push(current.clone());
                    current.clear();
                }
            }
            '(' | ')' | '.' | '#' => {
                if bracket_depth == 0 {
                    if !current.is_empty() {
                        segments.push(current.clone());
                        current.clear();
                    }
                } else {
                    current.push(c);
                }
            }
            '`' => {
                // Skip backticks
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        segments.push(current);
    }

    // segments should be: [Type, Trait?, method?]
    if segments.is_empty() {
        return None;
    }

    let type_name = segments.first()?.clone();

    // Check if second segment looks like a trait (capitalized, not a method)
    let (trait_name, method_name) = if segments.len() >= 2 {
        let second = &segments[1];
        if second.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
            && !second.contains('<')
            && segments.len() >= 3
        {
            // Trait impl with method
            (Some(second.clone()), segments.get(2).cloned())
        } else if second.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
            && segments.len() == 2
        {
            // Could be trait impl without method, or inherent impl with type method
            // If it's all caps or common trait name, treat as trait
            if is_likely_trait_name(second) {
                (Some(second.clone()), None)
            } else {
                (None, Some(second.clone()))
            }
        } else {
            // Inherent impl with method
            (None, Some(second.clone()))
        }
    } else {
        (None, None)
    };

    Some(ImplDescriptor {
        module_path,
        type_name,
        trait_name,
        method_name,
    })
}

/// Check if a name is likely a trait (common std traits)
fn is_likely_trait_name(name: &str) -> bool {
    matches!(
        name,
        "Display" | "Debug" | "Clone" | "Copy" | "Default" | "Hash"
            | "Eq" | "PartialEq" | "Ord" | "PartialOrd"
            | "From" | "Into" | "TryFrom" | "TryInto"
            | "Iterator" | "IntoIterator"
            | "Deref" | "DerefMut"
            | "Drop" | "Send" | "Sync"
            | "Error" | "Write" | "Read"
            | "AsRef" | "AsMut" | "Borrow" | "BorrowMut"
            | "Add" | "Sub" | "Mul" | "Div"
    )
}

/// Clean a type name by removing generic parameters for simpler matching
fn clean_type_name(name: &str) -> String {
    // For now, keep the full name including generics
    // This matches what codesearch does
    name.to_string()
}

/// Infer entity type from SCIP descriptor suffix
fn entity_type_from_descriptors(descriptors: &str) -> Option<EntityType> {
    // Check the final suffix to determine type
    let trimmed = descriptors.trim_end_matches(|c| c == '`' || c == ']');

    if trimmed.ends_with('!') {
        // Macro definition or invocation
        Some(EntityType::Macro)
    } else if trimmed.ends_with("().") {
        // Function or method call
        Some(EntityType::Function)
    } else if trimmed.ends_with('#') {
        // Type (struct, enum, trait)
        Some(EntityType::Struct)
    } else if trimmed.ends_with('/') {
        // Module/namespace
        Some(EntityType::Module)
    } else if trimmed.ends_with('.') {
        // Term (could be const, static, or method)
        if trimmed.contains("()") {
            Some(EntityType::Function)
        } else {
            Some(EntityType::Constant)
        }
    } else if trimmed.contains("impl#") {
        Some(EntityType::Impl)
    } else {
        None
    }
}

/// Check if a SCIP symbol belongs to the target package (internal)
pub fn is_internal_symbol(symbol: &str, package_name: &str) -> bool {
    // Internal symbols: "rust-analyzer cargo <package_name> <version> ..."
    // where version is NOT a URL (external deps have URLs as versions)
    if let Some(parsed) = parse_scip_symbol(symbol) {
        parsed.package == package_name && !parsed.version.starts_with("http")
    } else {
        false
    }
}

/// Check if a SCIP symbol is from an external package
pub fn is_external_symbol(symbol: &str) -> bool {
    if let Some(parsed) = parse_scip_symbol(symbol) {
        // External packages have URLs as versions (e.g., https://github.com/rust-lang/rust/...)
        parsed.version.starts_with("http")
    } else {
        false
    }
}

/// Generate a SCIP index for a Rust repository using rust-analyzer.
///
/// Runs `rust-analyzer scip .` in the repository directory.
/// Returns the path to the generated `index.scip` file.
///
/// Note: rust-analyzer may print warnings/errors to stderr but still generate
/// a valid index.scip file. We check for file existence rather than exit status.
pub fn generate_scip_index(repo_path: &Path) -> Result<std::path::PathBuf> {
    let output = Command::new("rust-analyzer")
        .args(["scip", "."])
        .current_dir(repo_path)
        .output()
        .context("Failed to run rust-analyzer. Is it installed?")?;

    let scip_path = repo_path.join("index.scip");

    // Check for file existence first - rust-analyzer may exit with error
    // but still produce a valid index (e.g., duplicate symbol warnings)
    if scip_path.exists() {
        return Ok(scip_path);
    }

    // No file generated - report the actual error
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("rust-analyzer scip failed: {stderr}");
    }

    anyhow::bail!(
        "SCIP index not found at expected location: {}",
        scip_path.display()
    )
}

/// Load a SCIP index from a file.
pub fn load_scip_index(scip_path: &Path) -> Result<Index> {
    let bytes = std::fs::read(scip_path)
        .with_context(|| format!("Failed to read SCIP file: {}", scip_path.display()))?;

    let index: Index = protobuf::Message::parse_from_bytes(&bytes)
        .context("Failed to parse SCIP protobuf")?;

    Ok(index)
}

/// Parse relationships from a SCIP index.
///
/// Extracts CALLS, USES, CONTAINS, and IMPLEMENTS relationships by analyzing
/// symbol definitions and references.
///
/// # Arguments
/// * `scip_path` - Path to the SCIP index file
/// * `package_name` - Name of the package to filter for (only internal symbols)
///
/// Returns relationships with normalized Rust-style qualified names.
pub fn parse_scip_relationships(scip_path: &Path, package_name: &str) -> Result<Vec<Relationship>> {
    let index = load_scip_index(scip_path)?;
    let mut relationships = Vec::new();

    // Build a map of symbol -> definition info
    let symbol_info = build_symbol_info_map(&index);

    // Process each document
    for document in &index.documents {
        let file_path = &document.relative_path;

        // Group occurrences by location to find scope context
        let _occurrences_by_line = group_occurrences_by_line(&document.occurrences);

        // Find all definitions in this document (only internal)
        let definitions = find_definitions(&document.occurrences, package_name);

        // For each reference, determine relationships
        for occurrence in &document.occurrences {
            if is_definition(occurrence) {
                continue; // Skip definitions, we're looking for references
            }

            let symbol = &occurrence.symbol;
            if symbol.is_empty() || symbol.starts_with("local ") {
                continue; // Skip empty or local symbols
            }

            // Skip external symbols (references to std, core, other crates)
            if !is_internal_symbol(symbol, package_name) {
                continue;
            }

            // Find the enclosing scope (function/method that contains this reference)
            let enclosing_scope = find_enclosing_scope(
                occurrence.range.first().copied().unwrap_or(0) as u32,
                &definitions,
            );

            // Determine relationship type based on context
            if let Some(rel_type) = classify_reference(occurrence, &symbol_info) {
                // Parse target symbol and normalize to qualified name
                let target = if let Some(parsed) = parse_scip_symbol(symbol) {
                    let mut entity_ref = EntityRef::new(parsed.qualified_name)
                        .with_file_path(file_path.clone());
                    if let Some(entity_type) = parsed.entity_type {
                        entity_ref = entity_ref.with_entity_type(entity_type);
                    }
                    entity_ref
                } else {
                    EntityRef::new(symbol.clone()).with_file_path(file_path.clone())
                };

                // Parse source symbol and normalize
                let source = if let Some(scope_symbol) = enclosing_scope {
                    if let Some(parsed) = parse_scip_symbol(scope_symbol) {
                        let mut entity_ref = EntityRef::new(parsed.qualified_name)
                            .with_file_path(file_path.clone());
                        if let Some(entity_type) = parsed.entity_type {
                            entity_ref = entity_ref.with_entity_type(entity_type);
                        }
                        entity_ref
                    } else {
                        EntityRef::new(scope_symbol.clone()).with_file_path(file_path.clone())
                    }
                } else {
                    // Module-level reference - use file path as module
                    let module_name = file_path_to_module_name(file_path, package_name);
                    EntityRef::new(module_name)
                        .with_file_path(file_path.clone())
                        .with_entity_type(EntityType::Module)
                };

                relationships.push(Relationship::new(source, target, rel_type));
            }
        }

        // Extract CONTAINS relationships from nested definitions
        relationships.extend(extract_contains_relationships(&definitions, file_path, package_name));
    }

    // Deduplicate relationships
    deduplicate_relationships(relationships)
}

/// Convert a file path to a module name
fn file_path_to_module_name(file_path: &str, package_name: &str) -> String {
    // Convert "src/error.rs" -> "anyhow::error"
    // Convert "src/lib.rs" -> "anyhow"
    let path = file_path
        .trim_start_matches("src/")
        .trim_end_matches(".rs");

    if path == "lib" || path == "main" {
        package_name.to_string()
    } else {
        format!("{}::{}", package_name, path.replace('/', "::"))
    }
}

/// Information about a symbol from SCIP
#[derive(Debug, Clone)]
struct SymbolInfo {
    symbol: String,
    kind: SymbolKind,
    #[allow(dead_code)]
    file_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SymbolKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Module,
    Constant,
    TypeAlias,
    Impl,
    Macro,
    Unknown,
}

fn build_symbol_info_map(index: &Index) -> HashMap<String, SymbolInfo> {
    let mut map = HashMap::new();

    for document in &index.documents {
        for symbol_info in &document.symbols {
            let kind = parse_symbol_kind(&symbol_info.symbol);
            map.insert(
                symbol_info.symbol.clone(),
                SymbolInfo {
                    symbol: symbol_info.symbol.clone(),
                    kind,
                    file_path: document.relative_path.clone(),
                },
            );
        }
    }

    map
}

fn parse_symbol_kind(symbol: &str) -> SymbolKind {
    // SCIP symbols have a descriptor suffix indicating kind
    // e.g., "rust-analyzer ... `function_name`." for functions
    // Macros end with "!" e.g., "macros/anyhow!"
    if symbol.ends_with('!') {
        SymbolKind::Macro
    } else if symbol.contains("impl#") {
        SymbolKind::Impl
    } else if symbol.ends_with("().") {
        SymbolKind::Function
    } else if symbol.ends_with('#') {
        SymbolKind::Struct
    } else if symbol.ends_with('.') && symbol.contains("()") {
        SymbolKind::Method
    } else if symbol.ends_with('/') {
        SymbolKind::Module
    } else {
        SymbolKind::Unknown
    }
}

#[derive(Debug, Clone)]
struct DefinitionInfo {
    symbol: String,
    start_line: u32,
    end_line: u32,
    #[allow(dead_code)]
    kind: SymbolKind,
}

fn find_definitions(occurrences: &[Occurrence], package_name: &str) -> Vec<DefinitionInfo> {
    occurrences
        .iter()
        .filter(|o| is_definition(o))
        .filter_map(|o| {
            if o.symbol.is_empty() || o.symbol.starts_with("local ") {
                return None;
            }

            // Only include internal symbols
            if !is_internal_symbol(&o.symbol, package_name) {
                return None;
            }

            let range = &o.range;
            if range.len() < 2 {
                return None;
            }

            Some(DefinitionInfo {
                symbol: o.symbol.clone(),
                start_line: range[0] as u32,
                end_line: range.get(2).copied().unwrap_or(range[0]) as u32,
                kind: parse_symbol_kind(&o.symbol),
            })
        })
        .collect()
}

fn is_definition(occurrence: &Occurrence) -> bool {
    occurrence.symbol_roles & (SymbolRole::Definition as i32) != 0
}

#[allow(dead_code)]
fn group_occurrences_by_line(occurrences: &[Occurrence]) -> HashMap<u32, Vec<&Occurrence>> {
    let mut map: HashMap<u32, Vec<&Occurrence>> = HashMap::new();
    for occ in occurrences {
        if let Some(&line) = occ.range.first() {
            map.entry(line as u32).or_default().push(occ);
        }
    }
    map
}

fn find_enclosing_scope(line: u32, definitions: &[DefinitionInfo]) -> Option<&String> {
    // Find the innermost function/method that contains this line
    let mut best_match: Option<&DefinitionInfo> = None;

    for def in definitions {
        if def.start_line <= line && line <= def.end_line {
            // Prefer functions/methods over other scopes
            match def.kind {
                SymbolKind::Function | SymbolKind::Method => {
                    if best_match.map_or(true, |b| def.start_line > b.start_line) {
                        best_match = Some(def);
                    }
                }
                _ => {
                    if best_match.is_none() {
                        best_match = Some(def);
                    }
                }
            }
        }
    }

    best_match.map(|d| &d.symbol)
}

fn classify_reference(
    occurrence: &Occurrence,
    symbol_info: &HashMap<String, SymbolInfo>,
) -> Option<RelationshipType> {
    let symbol = &occurrence.symbol;

    // Look up symbol info to determine kind
    if let Some(info) = symbol_info.get(symbol) {
        match info.kind {
            SymbolKind::Function | SymbolKind::Method => Some(RelationshipType::Calls),
            SymbolKind::Macro => Some(RelationshipType::Calls), // Macro invocations are like calls
            SymbolKind::Struct | SymbolKind::Enum | SymbolKind::TypeAlias => {
                Some(RelationshipType::Uses)
            }
            SymbolKind::Trait => {
                // Could be USES or IMPLEMENTS depending on context
                // For now, treat as USES
                Some(RelationshipType::Uses)
            }
            SymbolKind::Module => Some(RelationshipType::Imports),
            _ => None,
        }
    } else {
        // Try to infer from symbol name pattern
        if symbol.ends_with('!') {
            // Macro invocation
            Some(RelationshipType::Calls)
        } else if symbol.contains("()") {
            Some(RelationshipType::Calls)
        } else if symbol.ends_with('#') {
            Some(RelationshipType::Uses)
        } else {
            None
        }
    }
}

fn extract_contains_relationships(
    definitions: &[DefinitionInfo],
    file_path: &str,
    package_name: &str,
) -> Vec<Relationship> {
    let mut relationships = Vec::new();

    for def in definitions {
        // Find parent scope
        for potential_parent in definitions {
            if potential_parent.symbol == def.symbol {
                continue;
            }

            // Check if def is contained within potential_parent
            if potential_parent.start_line < def.start_line
                && def.end_line < potential_parent.end_line
            {
                // Check this is the immediate parent (no other def in between)
                let is_immediate = !definitions.iter().any(|other| {
                    other.symbol != def.symbol
                        && other.symbol != potential_parent.symbol
                        && other.start_line > potential_parent.start_line
                        && other.end_line < potential_parent.end_line
                        && other.start_line < def.start_line
                        && def.end_line < other.end_line
                });

                if is_immediate {
                    // Parse and normalize parent symbol
                    let parent = if let Some(parsed) = parse_scip_symbol(&potential_parent.symbol) {
                        let mut entity_ref = EntityRef::new(parsed.qualified_name)
                            .with_file_path(file_path);
                        if let Some(entity_type) = parsed.entity_type {
                            entity_ref = entity_ref.with_entity_type(entity_type);
                        }
                        entity_ref
                    } else {
                        EntityRef::new(potential_parent.symbol.clone()).with_file_path(file_path)
                    };

                    // Parse and normalize child symbol
                    let child = if let Some(parsed) = parse_scip_symbol(&def.symbol) {
                        let mut entity_ref = EntityRef::new(parsed.qualified_name)
                            .with_file_path(file_path);
                        if let Some(entity_type) = parsed.entity_type {
                            entity_ref = entity_ref.with_entity_type(entity_type);
                        }
                        entity_ref
                    } else {
                        EntityRef::new(def.symbol.clone()).with_file_path(file_path)
                    };

                    // Skip if either symbol couldn't be normalized properly
                    if parent.qualified_name.is_empty() || child.qualified_name.is_empty() {
                        continue;
                    }

                    // Skip self-references (can happen with impl blocks)
                    if parent.qualified_name == child.qualified_name {
                        continue;
                    }

                    relationships.push(Relationship::new(
                        parent,
                        child,
                        RelationshipType::Contains,
                    ));
                    break;
                }
            }
        }
    }

    // Filter out relationships with package_name as a parent that just contains the package itself
    relationships
        .into_iter()
        .filter(|rel| {
            // Skip if parent is just the package name and child is also just package name
            !(rel.source.qualified_name == package_name
                && rel.target.qualified_name == package_name)
        })
        .collect()
}

fn deduplicate_relationships(relationships: Vec<Relationship>) -> Result<Vec<Relationship>> {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for rel in relationships {
        let key = rel.to_key();
        if seen.insert(key) {
            result.push(rel);
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_symbol_kind() {
        assert_eq!(
            parse_symbol_kind("rust-analyzer cargo `foo`()."),
            SymbolKind::Function
        );
        assert_eq!(parse_symbol_kind("rust-analyzer cargo `Bar`#"), SymbolKind::Struct);
    }

    #[test]
    fn test_parse_scip_symbol() {
        // Basic function
        let sym = parse_scip_symbol("rust-analyzer cargo anyhow 1.0.100 error/ErrorImpl#display().").unwrap();
        assert_eq!(sym.package, "anyhow");
        assert_eq!(sym.version, "1.0.100");
        assert_eq!(sym.qualified_name, "anyhow::error::ErrorImpl::display");
        assert_eq!(sym.entity_type, Some(EntityType::Function));

        // Struct
        let sym = parse_scip_symbol("rust-analyzer cargo anyhow 1.0.100 error/Error#").unwrap();
        assert_eq!(sym.qualified_name, "anyhow::error::Error");
        assert_eq!(sym.entity_type, Some(EntityType::Struct));

        // Module (crate root)
        let sym = parse_scip_symbol("rust-analyzer cargo anyhow 1.0.100 crate/").unwrap();
        assert_eq!(sym.qualified_name, "anyhow");
        assert_eq!(sym.entity_type, Some(EntityType::Module));
    }

    #[test]
    fn test_is_internal_symbol() {
        // Internal symbol (version is simple string)
        assert!(is_internal_symbol(
            "rust-analyzer cargo anyhow 1.0.100 error/Error#",
            "anyhow"
        ));

        // External symbol (version is URL)
        assert!(!is_internal_symbol(
            "rust-analyzer cargo core https://github.com/rust-lang/rust/library/core result/Result#",
            "anyhow"
        ));

        // Different package
        assert!(!is_internal_symbol(
            "rust-analyzer cargo serde 1.0.0 de/Deserialize#",
            "anyhow"
        ));
    }

    #[test]
    fn test_descriptors_to_qualified_name() {
        assert_eq!(
            descriptors_to_qualified_name("anyhow", "error/ErrorImpl#display()."),
            "anyhow::error::ErrorImpl::display"
        );
        assert_eq!(
            descriptors_to_qualified_name("anyhow", "crate/"),
            "anyhow"
        );
        assert_eq!(
            descriptors_to_qualified_name("anyhow", "backtrace/Backtrace#"),
            "anyhow::backtrace::Backtrace"
        );
    }

    #[test]
    fn test_descriptors_to_qualified_name_trait_impl() {
        // Trait impl with method
        assert_eq!(
            descriptors_to_qualified_name("anyhow", "context/impl#[`ContextError<C, E>`][Display]fmt()."),
            "anyhow::context::<ContextError<C, E> as Display>::fmt"
        );

        // Inherent impl with method
        assert_eq!(
            descriptors_to_qualified_name("anyhow", "error/impl#[`Error`]new()."),
            "anyhow::error::Error::new"
        );
    }

    #[test]
    fn test_parse_impl_descriptor() {
        // Trait impl
        let result = parse_impl_descriptor("context/impl#[`ContextError<C, E>`][Display]fmt().").unwrap();
        assert_eq!(result.module_path, "context");
        assert_eq!(result.type_name, "ContextError<C, E>");
        assert_eq!(result.trait_name, Some("Display".to_string()));
        assert_eq!(result.method_name, Some("fmt".to_string()));

        // Inherent impl
        let result = parse_impl_descriptor("error/impl#[`Error`]new().").unwrap();
        assert_eq!(result.module_path, "error");
        assert_eq!(result.type_name, "Error");
        assert_eq!(result.trait_name, None);
        assert_eq!(result.method_name, Some("new".to_string()));

        // No impl marker
        assert!(parse_impl_descriptor("error/Error#").is_none());
    }

    #[test]
    fn test_file_path_to_module_name() {
        assert_eq!(
            file_path_to_module_name("src/lib.rs", "anyhow"),
            "anyhow"
        );
        assert_eq!(
            file_path_to_module_name("src/error.rs", "anyhow"),
            "anyhow::error"
        );
        assert_eq!(
            file_path_to_module_name("src/backtrace/capture.rs", "anyhow"),
            "anyhow::backtrace::capture"
        );
    }

    #[test]
    fn test_parse_symbol_kind_macro() {
        assert_eq!(
            parse_symbol_kind("rust-analyzer cargo anyhow 1.0.100 macros/anyhow!"),
            SymbolKind::Macro
        );
        assert_eq!(
            parse_symbol_kind("rust-analyzer cargo anyhow 1.0.100 macros/ensure!"),
            SymbolKind::Macro
        );
        assert_eq!(
            parse_symbol_kind("rust-analyzer cargo anyhow 1.0.100 macros/bail!"),
            SymbolKind::Macro
        );
    }

    #[test]
    fn test_descriptors_to_qualified_name_macro() {
        // Macro without ! in qualified name (to match codesearch format)
        assert_eq!(
            descriptors_to_qualified_name("anyhow", "macros/anyhow!"),
            "anyhow::macros::anyhow"
        );
        assert_eq!(
            descriptors_to_qualified_name("anyhow", "macros/ensure!"),
            "anyhow::macros::ensure"
        );
        assert_eq!(
            descriptors_to_qualified_name("anyhow", "ensure/__fancy_ensure!"),
            "anyhow::ensure::__fancy_ensure"
        );
    }

    #[test]
    fn test_entity_type_from_descriptors_macro() {
        assert_eq!(
            entity_type_from_descriptors("macros/anyhow!"),
            Some(EntityType::Macro)
        );
        assert_eq!(
            entity_type_from_descriptors("ensure/__fancy_ensure!"),
            Some(EntityType::Macro)
        );
    }

    #[test]
    fn test_parse_scip_symbol_macro() {
        let sym = parse_scip_symbol("rust-analyzer cargo anyhow 1.0.100 macros/anyhow!").unwrap();
        assert_eq!(sym.package, "anyhow");
        assert_eq!(sym.qualified_name, "anyhow::macros::anyhow");
        assert_eq!(sym.entity_type, Some(EntityType::Macro));
    }
}
