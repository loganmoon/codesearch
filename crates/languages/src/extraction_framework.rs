//! Generic, data-driven entity extractor framework
//!
//! This module provides a configurable extractor that uses tree-sitter queries
//! to extract entities from source code in a language-agnostic way.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use codesearch_core::{error::Result, CodeEntity};
use std::path::{Path, PathBuf};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor, QueryMatch};

/// Handler function type for processing query matches into entities
///
/// Arguments:
/// - `query_match` - The tree-sitter query match
/// - `query` - The tree-sitter query
/// - `source` - The source code
/// - `file_path` - Path to the source file
/// - `repository_id` - Repository identifier
/// - `package_name` - Optional package/crate name from manifest
/// - `source_root` - Optional source root for module path derivation
/// - `repo_root` - Repository root for deriving repo-relative paths
pub type EntityHandler = Box<
    dyn Fn(
            &QueryMatch,
            &Query,
            &str,
            &Path,
            &str,
            Option<&str>,
            Option<&Path>,
            &Path,
        ) -> Result<Vec<CodeEntity>>
        + Send
        + Sync,
>;

/// Defines how to extract a specific type of entity
struct EntityExtractor {
    /// Name identifier for this extractor (e.g., "function", "struct")
    name: String,

    /// Tree-sitter query string for finding this entity type
    query: String,

    /// Starting index in the combined query for this extractor's captures
    capture_offset: usize,

    /// Handler function to process matches and build entities
    handler: EntityHandler,
}

/// Language-specific configuration for entity extraction
pub struct LanguageConfiguration {
    /// Tree-sitter language object
    language: Language,

    /// List of entity extractors for this language
    entity_extractors: Vec<EntityExtractor>,

    /// Compiled combined query
    compiled_query: Query,
}

/// Builder for creating a LanguageConfiguration
pub struct LanguageConfigurationBuilder {
    /// Tree-sitter language object
    language: Language,

    /// List of entity extractors being built
    entity_extractors: Vec<EntityExtractor>,
}

impl LanguageConfigurationBuilder {
    /// Create a new language configuration builder
    pub fn new(language: Language) -> Self {
        Self {
            language,
            entity_extractors: Vec::new(),
        }
    }

    /// Add an entity extractor to this configuration
    pub fn add_extractor(
        mut self,
        name: impl Into<String>,
        query: impl Into<String>,
        handler: EntityHandler,
    ) -> Self {
        let name = name.into();
        let query = query.into();
        self.entity_extractors.push(EntityExtractor {
            name,
            query,
            capture_offset: 0, // Will be calculated during build
            handler,
        });
        self
    }

    /// Build and compile the language configuration
    pub fn build(mut self) -> Result<LanguageConfiguration> {
        if self.entity_extractors.is_empty() {
            return Err(anyhow::anyhow!("No extractors added to configuration").into());
        }

        let mut combined_parts = Vec::new();
        let mut current_offset = 0;

        for extractor in &mut self.entity_extractors {
            // Store the capture offset for this extractor
            extractor.capture_offset = current_offset;

            // Parse the query to count captures
            let temp_query = Query::new(&self.language, &extractor.query).map_err(|e| {
                anyhow::anyhow!("Failed to parse query for {}: {e}", extractor.name)
            })?;
            current_offset += temp_query.capture_names().len();

            // Add to combined query with a unique pattern name
            // Remove the outer pattern capture to avoid duplicates
            let trimmed_query = extractor.query.trim();
            combined_parts.push(format!(
                "{} @__extractor_{}",
                trimmed_query,
                extractor.name.replace('-', "_")
            ));
        }

        // Join all queries with alternation
        let combined = combined_parts.join("\n");

        // Compile the combined query
        let compiled_query = Query::new(&self.language, &combined)
            .map_err(|e| anyhow::anyhow!("Failed to compile combined query: {e}"))?;

        Ok(LanguageConfiguration {
            language: self.language,
            entity_extractors: self.entity_extractors,
            compiled_query,
        })
    }
}

impl LanguageConfiguration {
    /// Get the compiled query for extraction
    pub fn query(&self) -> &Query {
        &self.compiled_query
    }

    /// Get the entity extractors
    fn extractors(&self) -> &[EntityExtractor] {
        &self.entity_extractors
    }
}

/// Generic entity extractor that uses configuration to extract entities
pub struct GenericExtractor<'a> {
    /// Language configuration
    config: &'a LanguageConfiguration,

    /// Parser instance
    parser: Parser,

    /// Repository ID for entity generation
    repository_id: String,

    /// Package/crate name from manifest (e.g., "codesearch_core")
    package_name: Option<String>,

    /// Source root for module path derivation (e.g., "/project/src")
    source_root: Option<PathBuf>,

    /// Repository root for deriving repo-relative paths
    repo_root: PathBuf,
}

impl<'a> GenericExtractor<'a> {
    /// Create a new generic extractor with the given configuration
    pub fn new(
        config: &'a LanguageConfiguration,
        repository_id: String,
        package_name: Option<&str>,
        source_root: Option<&Path>,
        repo_root: &Path,
    ) -> Result<Self> {
        let mut parser = Parser::new();
        parser
            .set_language(&config.language)
            .map_err(|e| anyhow::anyhow!("Failed to set language: {e}"))?;

        Ok(Self {
            config,
            parser,
            repository_id,
            package_name: package_name.map(String::from),
            source_root: source_root.map(PathBuf::from),
            repo_root: repo_root.to_path_buf(),
        })
    }

    /// Extract entities from source code
    pub fn extract(&mut self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        // Parse the source code
        let tree = self
            .parser
            .parse(source, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse source code"))?;

        // Get the compiled query
        let query = self.config.query();

        let mut all_entities = Vec::new();
        let mut cursor = QueryCursor::new();

        // Set resource limits to prevent excessive CPU consumption
        cursor.set_timeout_micros(5_000_000); // 5 second timeout
        cursor.set_match_limit(10_000); // Limit number of matches

        // Execute the combined query
        let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());

        while let Some(query_match) = matches.next() {
            // Determine which extractor this match belongs to
            // by checking the special __extractor_* capture
            let mut processed = false;
            for capture in query_match.captures.iter() {
                let capture_name = query
                    .capture_names()
                    .get(capture.index as usize)
                    .cloned()
                    .unwrap_or_default();

                if capture_name.starts_with("__extractor_") && !processed {
                    // Extract the extractor name
                    let extractor_name = capture_name
                        .strip_prefix("__extractor_")
                        .unwrap_or_default();

                    // Find the corresponding extractor
                    if let Some(extractor) = self
                        .config
                        .extractors()
                        .iter()
                        .find(|e| e.name == extractor_name)
                    {
                        // Call the handler with repository_id and package context
                        let entities = (extractor.handler)(
                            query_match,
                            query,
                            source,
                            file_path,
                            &self.repository_id,
                            self.package_name.as_deref(),
                            self.source_root.as_deref(),
                            &self.repo_root,
                        )?;
                        all_entities.extend(entities);
                        processed = true;
                    }
                }
            }
        }

        Ok(all_entities)
    }
}
