//! Neo4j query functions for retrieving graph data for validation

use super::super::containers::TestNeo4j;
use super::schema::{ActualEntity, ActualRelationship};
use anyhow::{anyhow, Context, Result};
use codesearch_core::entities::Visibility;
use neo4rs::{query, Graph};
use std::str::FromStr;

/// Query all entities from Neo4j for a given repository
pub async fn get_all_entities(neo4j: &TestNeo4j, repository_id: &str) -> Result<Vec<ActualEntity>> {
    let graph: Graph = Graph::new(neo4j.bolt_url(), "", "")
        .await
        .context("Failed to connect to Neo4j")?;

    let cypher = query(
        r#"
        MATCH (n:Entity)
        WHERE n.repository_id = $repository_id
        RETURN n.id AS entity_id,
               labels(n) AS labels,
               n.qualified_name AS qualified_name,
               n.name AS name,
               n.visibility AS visibility
        "#,
    )
    .param("repository_id", repository_id);

    let mut result = graph.execute(cypher).await?;
    let mut entities = Vec::new();

    loop {
        match result.next().await {
            Ok(Some(row)) => {
                let entity_id: String = row
                    .get::<String>("entity_id")
                    .context("Failed to get entity_id from Neo4j row")?;
                let labels: Vec<String> = row
                    .get::<Vec<String>>("labels")
                    .with_context(|| format!("Failed to get labels for entity {entity_id}"))?;
                let qualified_name: String =
                    row.get::<String>("qualified_name").with_context(|| {
                        format!("Failed to get qualified_name for entity {entity_id}")
                    })?;
                let name: String = row
                    .get::<String>("name")
                    .with_context(|| format!("Failed to get name for entity {entity_id}"))?;
                let visibility_str: Option<String> = row.get::<String>("visibility").ok();
                let visibility = visibility_str
                    .as_ref()
                    .and_then(|s| Visibility::from_str(s).ok());

                // Extract entity type from labels (skip "Entity" label)
                let entity_type = labels.into_iter().find(|l| l != "Entity").ok_or_else(|| {
                    anyhow!(
                        "Entity {entity_id} has no type label (only 'Entity'). \
                         This indicates a bug in entity creation."
                    )
                })?;

                entities.push(ActualEntity {
                    entity_id,
                    entity_type,
                    qualified_name,
                    name,
                    visibility,
                });
            }
            Ok(None) => break, // Normal end of results
            Err(e) => {
                return Err(anyhow!(
                    "Neo4j result streaming failed after processing {} entities: {e}",
                    entities.len()
                ));
            }
        }
    }

    Ok(entities)
}

/// Query all relationships from Neo4j for a given repository
pub async fn get_all_relationships(
    neo4j: &TestNeo4j,
    repository_id: &str,
) -> Result<Vec<ActualRelationship>> {
    let graph: Graph = Graph::new(neo4j.bolt_url(), "", "")
        .await
        .context("Failed to connect to Neo4j")?;

    // Require both endpoints to be Entity nodes with matching repository_id
    let cypher = query(
        r#"
        MATCH (from:Entity)-[r]->(to:Entity)
        WHERE from.repository_id = $repository_id
          AND to.repository_id = $repository_id
        RETURN from.qualified_name AS from_qname,
               to.qualified_name AS to_qname,
               type(r) AS rel_type
        "#,
    )
    .param("repository_id", repository_id);

    let mut result = graph.execute(cypher).await?;
    let mut relationships = Vec::new();

    loop {
        match result.next().await {
            Ok(Some(row)) => {
                let from_qualified_name: String = row
                    .get::<String>("from_qname")
                    .context("Failed to get from_qname from Neo4j row")?;
                let to_qualified_name: String = row
                    .get::<String>("to_qname")
                    .context("Failed to get to_qname from Neo4j row")?;
                let rel_type: String = row
                    .get::<String>("rel_type")
                    .context("Failed to get rel_type from Neo4j row")?;

                relationships.push(ActualRelationship {
                    rel_type,
                    from_qualified_name,
                    to_qualified_name,
                });
            }
            Ok(None) => break, // Normal end of results
            Err(e) => {
                return Err(anyhow!(
                    "Neo4j result streaming failed after processing {} relationships: {e}",
                    relationships.len()
                ));
            }
        }
    }

    Ok(relationships)
}
