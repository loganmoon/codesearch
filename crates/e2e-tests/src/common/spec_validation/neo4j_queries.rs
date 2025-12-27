//! Neo4j query functions for retrieving graph data for validation

use super::super::containers::TestNeo4j;
use super::schema::{ActualEntity, ActualRelationship};
use anyhow::{Context, Result};
use neo4rs::{query, Graph};

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
               n.name AS name
        "#,
    )
    .param("repository_id", repository_id);

    let mut result = graph.execute(cypher).await?;
    let mut entities = Vec::new();

    while let Ok(Some(row)) = result.next().await {
        let entity_id: String = row.get::<String>("entity_id").unwrap_or_default();
        let labels: Vec<String> = row.get::<Vec<String>>("labels").unwrap_or_default();
        let qualified_name: String = row.get::<String>("qualified_name").unwrap_or_default();
        let name: String = row.get::<String>("name").unwrap_or_default();

        // Extract entity type from labels (skip "Entity" label)
        let entity_type = labels
            .into_iter()
            .find(|l| l != "Entity")
            .unwrap_or_else(|| "Unknown".to_string());

        entities.push(ActualEntity {
            entity_id,
            entity_type,
            qualified_name,
            name,
        });
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

    let cypher = query(
        r#"
        MATCH (from:Entity)-[r]->(to)
        WHERE from.repository_id = $repository_id
        RETURN from.qualified_name AS from_qname,
               to.qualified_name AS to_qname,
               type(r) AS rel_type
        "#,
    )
    .param("repository_id", repository_id);

    let mut result = graph.execute(cypher).await?;
    let mut relationships = Vec::new();

    while let Ok(Some(row)) = result.next().await {
        let from_qualified_name: String = row.get::<String>("from_qname").unwrap_or_default();
        let to_qualified_name: String = row.get::<String>("to_qname").unwrap_or_default();
        let rel_type: String = row.get::<String>("rel_type").unwrap_or_default();

        relationships.push(ActualRelationship {
            rel_type,
            from_qualified_name,
            to_qualified_name,
        });
    }

    Ok(relationships)
}
