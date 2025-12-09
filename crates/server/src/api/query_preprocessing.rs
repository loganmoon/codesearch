//! Query preprocessing for improved search relevance
//!
//! This module extracts code identifiers from natural language queries
//! and infers relevant entity types from query text.

use codesearch_core::config::QueryPreprocessingConfig;
use codesearch_core::entities::EntityType;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// Query intent classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryIntent {
    /// Looking for call relationships (callers, callees)
    CallGraph,
    /// Looking for trait/interface implementations
    TraitImpl,
    /// Looking for definitions (specific entity by name)
    Definition,
    /// General semantic search
    Semantic,
    /// File-based search
    FileSearch,
}

/// Result of query preprocessing
#[derive(Debug, Clone)]
pub struct PreprocessedQuery {
    /// Original query text
    pub original: String,
    /// Extracted code identifiers
    pub identifiers: Vec<String>,
    /// Inferred entity types
    pub entity_types: Vec<EntityType>,
    /// Detected query intent
    pub intent: QueryIntent,
    /// Query optimized for fulltext search (extracted identifiers joined)
    pub fulltext_query: Option<String>,
    /// Whether to skip fulltext search based on intent
    pub skip_fulltext: bool,
}

// Compiled regex patterns for identifier extraction
// These are compile-time constant patterns, so we use infallible initialization
static PATH_PATTERN: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"\b[\w]+(?:::[\w]+)+\b").ok());
static SNAKE_PATTERN: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"\b[a-z][a-z0-9]*(?:_[a-z0-9]+)+\b").ok());
static CAMEL_PATTERN: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"\b[A-Z][a-z0-9]+(?:[A-Z][a-z0-9]+)+\b").ok());

/// Preprocess a query to extract identifiers and infer types
pub fn preprocess_query(query: &str, config: &QueryPreprocessingConfig) -> PreprocessedQuery {
    if !config.enabled {
        return PreprocessedQuery {
            original: query.to_string(),
            identifiers: vec![],
            entity_types: vec![],
            intent: QueryIntent::Semantic,
            fulltext_query: None,
            skip_fulltext: false,
        };
    }

    let identifiers = if config.extract_identifiers {
        extract_identifiers(query)
    } else {
        vec![]
    };

    let entity_types = if config.infer_entity_types {
        infer_entity_types(query)
    } else {
        vec![]
    };

    let intent = if config.detect_query_intent {
        detect_query_intent(query)
    } else {
        QueryIntent::Semantic
    };

    let skip_fulltext = matches!(intent, QueryIntent::CallGraph | QueryIntent::TraitImpl);

    let fulltext_query = if !identifiers.is_empty() {
        Some(identifiers.join(" "))
    } else {
        None
    };

    PreprocessedQuery {
        original: query.to_string(),
        identifiers,
        entity_types,
        intent,
        fulltext_query,
        skip_fulltext,
    }
}

/// Extract code identifiers from query text
fn extract_identifiers(query: &str) -> Vec<String> {
    let mut identifiers = Vec::new();

    // Pattern for path::separators (e.g., support::token, codesearch_core::config)
    if let Some(ref pattern) = *PATH_PATTERN {
        for cap in pattern.find_iter(query) {
            identifiers.push(cap.as_str().to_string());
        }
    }

    // Pattern for snake_case (e.g., search_entities_fulltext)
    if let Some(ref pattern) = *SNAKE_PATTERN {
        for cap in pattern.find_iter(query) {
            let id = cap.as_str();
            // Skip common English phrases that look like snake_case
            if !is_common_phrase(id) {
                identifiers.push(id.to_string());
            }
        }
    }

    // Pattern for CamelCase/PascalCase (e.g., CodeEntity, PostgresClient)
    if let Some(ref pattern) = *CAMEL_PATTERN {
        for cap in pattern.find_iter(query) {
            identifiers.push(cap.as_str().to_string());
        }
    }

    // Deduplicate while preserving order
    let mut seen = HashSet::new();
    identifiers.retain(|id| seen.insert(id.clone()));

    identifiers
}

/// Check if a string is a common English phrase
fn is_common_phrase(s: &str) -> bool {
    const COMMON_PHRASES: &[&str] = &[
        "as_str",
        "to_string",
        "as_ref",
        "is_empty",
        "is_some",
        "is_none",
    ];
    COMMON_PHRASES.contains(&s)
}

/// Infer entity types from query text
fn infer_entity_types(query: &str) -> Vec<EntityType> {
    let lower = query.to_lowercase();
    let mut types = Vec::new();

    // Function-related keywords
    if lower.contains("function")
        || lower.contains(" fn ")
        || lower.contains("method")
        || lower.contains(" def ")
        || lower.contains("callable")
    {
        types.push(EntityType::Function);
        types.push(EntityType::Method);
    }

    // Type-related keywords
    if lower.contains("struct")
        || lower.contains("type ")
        || lower.contains("class")
        || lower.contains("data structure")
    {
        types.push(EntityType::Struct);
        types.push(EntityType::Class);
    }

    // Trait/Interface keywords
    if lower.contains("trait")
        || lower.contains("interface")
        || lower.contains("implement")
        || lower.contains("implementation")
    {
        types.push(EntityType::Trait);
        types.push(EntityType::Interface);
        types.push(EntityType::Impl);
    }

    // Enum keywords
    if lower.contains("enum") || lower.contains("enumeration") || lower.contains("variant") {
        types.push(EntityType::Enum);
    }

    // Module keywords
    if lower.contains("module") || lower.contains(" mod ") || lower.contains("package") {
        types.push(EntityType::Module);
        types.push(EntityType::Package);
    }

    // Constant keywords
    if lower.contains("constant") || lower.contains("const ") {
        types.push(EntityType::Constant);
    }

    // Macro keywords
    if lower.contains("macro") {
        types.push(EntityType::Macro);
    }

    // Deduplicate
    let mut seen = HashSet::new();
    types.retain(|t| seen.insert(*t));

    types
}

/// Detect query intent to route search appropriately
fn detect_query_intent(query: &str) -> QueryIntent {
    let lower = query.to_lowercase();

    // Call graph patterns
    if lower.contains("call")
        || lower.contains("caller")
        || lower.contains("callee")
        || lower.contains("invok")
        || lower.contains("who uses")
    {
        return QueryIntent::CallGraph;
    }

    // Trait implementation patterns
    if lower.contains("implement")
        || lower.contains("implementor")
        || lower.contains("implements trait")
    {
        return QueryIntent::TraitImpl;
    }

    // Definition patterns (looking for specific entity)
    if lower.contains("definition of")
        || lower.contains("where is")
        || lower.contains("find the")
        || lower.starts_with("what is")
    {
        return QueryIntent::Definition;
    }

    // File search patterns
    if lower.contains("file")
        || lower.contains(".rs")
        || lower.contains(".py")
        || lower.contains(".ts")
        || lower.contains(".js")
    {
        return QueryIntent::FileSearch;
    }

    QueryIntent::Semantic
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> QueryPreprocessingConfig {
        QueryPreprocessingConfig {
            enabled: true,
            extract_identifiers: true,
            infer_entity_types: true,
            detect_query_intent: true,
        }
    }

    #[test]
    fn test_extract_identifiers_path_separator() {
        let ids = extract_identifiers("What functions call support::token?");
        assert!(ids.contains(&"support::token".to_string()));
    }

    #[test]
    fn test_extract_identifiers_snake_case() {
        let ids = extract_identifiers("Find the search_entities_fulltext function");
        assert!(ids.contains(&"search_entities_fulltext".to_string()));
    }

    #[test]
    fn test_extract_identifiers_camel_case() {
        let ids = extract_identifiers("How does CodeEntity work?");
        assert!(ids.contains(&"CodeEntity".to_string()));
    }

    #[test]
    fn test_extract_identifiers_skips_common_phrases() {
        let ids = extract_identifiers("Check if is_empty returns true");
        assert!(!ids.contains(&"is_empty".to_string()));
    }

    #[test]
    fn test_infer_entity_types_function() {
        let types = infer_entity_types("Find all functions that handle errors");
        assert!(types.contains(&EntityType::Function));
    }

    #[test]
    fn test_infer_entity_types_struct() {
        let types = infer_entity_types("What structs are defined in this module?");
        assert!(types.contains(&EntityType::Struct));
    }

    #[test]
    fn test_infer_entity_types_trait() {
        let types = infer_entity_types("What types implement this trait?");
        assert!(types.contains(&EntityType::Trait));
        assert!(types.contains(&EntityType::Impl));
    }

    #[test]
    fn test_detect_query_intent_call_graph() {
        let intent = detect_query_intent("What functions call this method?");
        assert_eq!(intent, QueryIntent::CallGraph);
    }

    #[test]
    fn test_detect_query_intent_trait_impl() {
        let intent = detect_query_intent("What types implement HasSource?");
        assert_eq!(intent, QueryIntent::TraitImpl);
    }

    #[test]
    fn test_detect_query_intent_semantic() {
        let intent = detect_query_intent("How does error handling work?");
        assert_eq!(intent, QueryIntent::Semantic);
    }

    #[test]
    fn test_preprocess_query_full() {
        let config = test_config();
        let result = preprocess_query("What functions call support::token?", &config);

        assert!(result.identifiers.contains(&"support::token".to_string()));
        assert!(result.entity_types.contains(&EntityType::Function));
        assert_eq!(result.intent, QueryIntent::CallGraph);
        assert!(result.skip_fulltext);
        assert!(result.fulltext_query.is_some());
    }

    #[test]
    fn test_preprocess_query_disabled() {
        let config = QueryPreprocessingConfig {
            enabled: false,
            ..test_config()
        };
        let result = preprocess_query("What functions call support::token?", &config);

        assert!(result.identifiers.is_empty());
        assert!(result.entity_types.is_empty());
        assert_eq!(result.intent, QueryIntent::Semantic);
        assert!(!result.skip_fulltext);
    }
}
