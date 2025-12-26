//! Graph validation module for comparing codesearch extraction against SCIP ground truth
//!
//! This module provides tools to validate codesearch's code graph extraction by:
//! 1. Running the full indexing pipeline with mock embeddings
//! 2. Querying Neo4j for extracted relationships
//! 3. Comparing against rust-analyzer's SCIP output as ground truth
//! 4. Generating detailed precision/recall reports

pub mod comparator;
pub mod models;
pub mod report;
pub mod scip_parser;

use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::common::containers::TestNeo4j;
pub use comparator::compare;
pub use models::{
    aggregate_imports_to_module_level, ComparisonResult, EntityRef, Metrics, Relationship,
    RelationshipType,
};
pub use report::write_report;
pub use scip_parser::{
    generate_scip_index, is_internal_symbol, parse_scip_relationships, parse_scip_symbol,
    ScipSymbol,
};

// Neo4j Community Edition only supports a single database
const NEO4J_DEFAULT_DATABASE: &str = "neo4j";

/// Check if a qualified name represents an impl block entity.
///
/// Impl blocks in codesearch are represented as:
/// - `impl crate::Error` (inherent impl)
/// - `<crate::Error as core::fmt::Display>` (trait impl)
///
/// SCIP doesn't have separate impl block entities - methods are directly
/// attributed to the type. So we filter these out for comparison.
fn is_impl_block_entity(qualified_name: &str) -> bool {
    // Inherent impl: "impl Type" or "impl crate::Type"
    if qualified_name.starts_with("impl ") {
        return true;
    }

    // Trait impl: "<Type as Trait>" pattern
    // Must start with < and contain " as " but NOT be a method (no ::method at end after >)
    if qualified_name.starts_with('<') && qualified_name.contains(" as ") {
        // Check if this is the impl block itself vs a method on the impl
        // Impl block: "<Type as Trait>"
        // Method: "<Type as Trait>::method"
        if let Some(pos) = qualified_name.rfind('>') {
            let after_bracket = &qualified_name[pos + 1..];
            // If nothing after > or just whitespace, it's the impl block itself
            if after_bracket.is_empty() || !after_bracket.contains("::") {
                return true;
            }
        }
    }

    false
}

/// Query all relationships from Neo4j for a repository.
///
/// Returns relationships with qualified names for accurate comparison against SCIP.
/// Filters out impl block entities and their relationships since SCIP doesn't
/// represent impl blocks as separate entities.
pub async fn query_all_relationships(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
) -> Result<Vec<Relationship>> {
    let url = format!(
        "{}/db/{}/tx/commit",
        neo4j.http_url(),
        NEO4J_DEFAULT_DATABASE
    );

    let client = reqwest::Client::new();

    // Query all relationships between Entity nodes
    // Use qualified_name for accurate symbol matching
    // Include entity_type for intelligent filtering
    let body = serde_json::json!({
        "statements": [{
            "statement": r#"
                MATCH (source:Entity {repository_id: $repo_id})-[r]->(target:Entity)
                RETURN
                    source.qualified_name AS source_qname,
                    source.entity_type AS source_type,
                    type(r) AS rel_type,
                    target.qualified_name AS target_qname,
                    target.entity_type AS target_type
            "#,
            "parameters": {
                "repo_id": repository_id
            }
        }]
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to query Neo4j for relationships")?;

    if !response.status().is_success() {
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("<failed to read body>"));
        anyhow::bail!("Neo4j query failed: {text}");
    }

    let result: Neo4jQueryResult = response
        .json()
        .await
        .context("Failed to parse Neo4j response")?;

    if !result.errors.is_empty() {
        anyhow::bail!("Neo4j query errors: {:?}", result.errors);
    }

    let mut relationships = Vec::new();
    let mut skipped_malformed = 0usize;
    let mut skipped_unknown_rel = 0usize;

    for statement_result in &result.results {
        for row_data in &statement_result.data {
            let row = &row_data.row;
            // Now expecting 5 fields: source_qname, source_type, rel_type, target_qname, target_type
            if row.len() < 5 {
                skipped_malformed += 1;
                continue;
            }

            let source_qname = row[0].as_str().unwrap_or_default();
            let source_type_str = row[1].as_str().unwrap_or_default();
            let rel_type_str = row[2].as_str().unwrap_or_default();
            let target_qname = row[3].as_str().unwrap_or_default();
            let target_type_str = row[4].as_str().unwrap_or_default();

            // Skip empty values
            if source_qname.is_empty() || target_qname.is_empty() {
                skipped_malformed += 1;
                continue;
            }

            // Skip relationships involving impl block entities
            // SCIP doesn't represent impl blocks as separate entities
            if is_impl_block_entity(source_qname) || is_impl_block_entity(target_qname) {
                continue;
            }

            // Parse entity types
            let source_type = codesearch_core::entities::EntityType::from_str(source_type_str).ok();
            let target_type = codesearch_core::entities::EntityType::from_str(target_type_str).ok();

            // Parse relationship type
            if let Some(rel_type) = RelationshipType::from_neo4j_type(rel_type_str) {
                let mut source = EntityRef::new(source_qname);
                let mut target = EntityRef::new(target_qname);

                if let Some(et) = source_type {
                    source = source.with_entity_type(et);
                }
                if let Some(et) = target_type {
                    target = target.with_entity_type(et);
                }

                relationships.push(Relationship::new(source, target, rel_type));
            } else {
                skipped_unknown_rel += 1;
            }
        }
    }

    if skipped_malformed > 0 || skipped_unknown_rel > 0 {
        tracing::debug!(
            "Skipped Neo4j rows: {skipped_malformed} malformed, {skipped_unknown_rel} unknown relationship types"
        );
    }

    Ok(relationships)
}

/// Query all relationships including those to External nodes.
///
/// External nodes represent unresolved references to stdlib/third-party code.
pub async fn query_all_relationships_including_external(
    neo4j: &Arc<TestNeo4j>,
    repository_id: &str,
) -> Result<Vec<Relationship>> {
    let url = format!(
        "{}/db/{}/tx/commit",
        neo4j.http_url(),
        NEO4J_DEFAULT_DATABASE
    );

    let client = reqwest::Client::new();

    // Query relationships to both Entity and External nodes
    let body = serde_json::json!({
        "statements": [{
            "statement": r#"
                MATCH (source:Entity {repository_id: $repo_id})-[r]->(target)
                RETURN
                    source.qualified_name AS source_qname,
                    type(r) AS rel_type,
                    COALESCE(target.qualified_name, target.entity_id) AS target_qname,
                    labels(target) AS target_labels
            "#,
            "parameters": {
                "repo_id": repository_id
            }
        }]
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to query Neo4j for relationships")?;

    if !response.status().is_success() {
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("<failed to read body>"));
        anyhow::bail!("Neo4j query failed: {text}");
    }

    let result: Neo4jQueryResult = response
        .json()
        .await
        .context("Failed to parse Neo4j response")?;

    if !result.errors.is_empty() {
        anyhow::bail!("Neo4j query errors: {:?}", result.errors);
    }

    let mut relationships = Vec::new();
    let mut skipped_malformed = 0usize;
    let mut skipped_unknown_rel = 0usize;

    for statement_result in &result.results {
        for row_data in &statement_result.data {
            let row = &row_data.row;
            if row.len() < 3 {
                skipped_malformed += 1;
                continue;
            }

            let source_qname = row[0].as_str().unwrap_or_default();
            let rel_type_str = row[1].as_str().unwrap_or_default();
            let target_qname = row[2].as_str().unwrap_or_default();

            if source_qname.is_empty() || target_qname.is_empty() {
                skipped_malformed += 1;
                continue;
            }

            if let Some(rel_type) = RelationshipType::from_neo4j_type(rel_type_str) {
                let source = EntityRef::new(source_qname);
                let target = EntityRef::new(target_qname);
                relationships.push(Relationship::new(source, target, rel_type));
            } else {
                skipped_unknown_rel += 1;
            }
        }
    }

    if skipped_malformed > 0 || skipped_unknown_rel > 0 {
        tracing::debug!(
            "Skipped Neo4j rows (including external): {skipped_malformed} malformed, {skipped_unknown_rel} unknown relationship types"
        );
    }

    Ok(relationships)
}

// Neo4j HTTP API response structures
#[derive(Debug, serde::Deserialize)]
struct Neo4jQueryResult {
    results: Vec<Neo4jStatementResult>,
    #[serde(default)]
    errors: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
struct Neo4jStatementResult {
    data: Vec<Neo4jDataRow>,
}

#[derive(Debug, serde::Deserialize)]
struct Neo4jDataRow {
    row: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_impl_block_entity_inherent() {
        // Inherent impl patterns
        assert!(is_impl_block_entity("impl Error"));
        assert!(is_impl_block_entity("impl crate::Error"));
        assert!(is_impl_block_entity("impl anyhow::Error"));

        // Should NOT match - not an impl block
        assert!(!is_impl_block_entity("implement_foo"));
        assert!(!is_impl_block_entity("implementation"));
    }

    #[test]
    fn test_is_impl_block_entity_trait_impl() {
        // Trait impl blocks (the impl itself, not methods)
        assert!(is_impl_block_entity("<Error as Display>"));
        assert!(is_impl_block_entity("<crate::Error as std::fmt::Display>"));
        assert!(is_impl_block_entity("<anyhow::Error as core::fmt::Debug>"));

        // Methods on trait impls are NOT impl blocks
        assert!(!is_impl_block_entity("<Error as Display>::fmt"));
        assert!(!is_impl_block_entity("<crate::Error as std::fmt::Display>::fmt"));
    }

    #[test]
    fn test_is_impl_block_entity_negative() {
        // Regular qualified names
        assert!(!is_impl_block_entity("Error::new"));
        assert!(!is_impl_block_entity("anyhow::Error"));
        assert!(!is_impl_block_entity("std::fmt::Display"));
        assert!(!is_impl_block_entity(""));
        assert!(!is_impl_block_entity("MyStruct"));
        assert!(!is_impl_block_entity("crate::module::function"));
    }
}
