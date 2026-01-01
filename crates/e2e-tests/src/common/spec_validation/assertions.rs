//! Assertion functions for comparing expected vs actual graph structure

use super::schema::{
    ActualEntity, ActualRelationship, EntityKind, ExpectedEntity, ExpectedRelationship,
    RelationshipKind,
};
use anyhow::{bail, Result};
use codesearch_core::entities::Visibility;
use std::collections::{HashMap, HashSet};

/// Assert that all expected entities are present in the actual entities
///
/// Returns an error with detailed diff if any expected entities are missing.
/// Also validates visibility when expected.visibility is Some.
/// Does not fail on extra entities (subset matching).
pub fn assert_entities_match(expected: &[ExpectedEntity], actual: &[ActualEntity]) -> Result<()> {
    let expected_set: HashSet<(&str, &str)> = expected
        .iter()
        .map(|e| (e.kind.as_neo4j_label(), e.qualified_name))
        .collect();

    let actual_set: HashSet<(&str, &str)> = actual
        .iter()
        .map(|e| (e.entity_type.as_str(), e.qualified_name.as_str()))
        .collect();

    let missing: Vec<_> = expected_set.difference(&actual_set).collect();

    if !missing.is_empty() {
        let mut msg = String::from("Entity mismatch:\n\n");
        msg.push_str("MISSING ENTITIES:\n");
        for (entity_type, qname) in &missing {
            msg.push_str(&format!("  - {entity_type} {qname}\n"));
        }

        msg.push_str("\nACTUAL ENTITIES:\n");
        for (entity_type, qname) in &actual_set {
            msg.push_str(&format!("  - {entity_type} {qname}\n"));
        }

        bail!("{}", msg);
    }

    // Build a map of actual entities by (type, qualified_name) for visibility lookup
    let actual_by_key: HashMap<(&str, &str), &ActualEntity> = actual
        .iter()
        .map(|e| ((e.entity_type.as_str(), e.qualified_name.as_str()), e))
        .collect();

    // Check visibility mismatches for entities that specify expected visibility
    let mut visibility_mismatches: Vec<(&str, Option<Visibility>, Option<Visibility>)> = Vec::new();
    for exp in expected {
        if let Some(expected_vis) = exp.visibility {
            let key = (exp.kind.as_neo4j_label(), exp.qualified_name);
            if let Some(actual_entity) = actual_by_key.get(&key) {
                if actual_entity.visibility != Some(expected_vis) {
                    visibility_mismatches.push((
                        exp.qualified_name,
                        Some(expected_vis),
                        actual_entity.visibility,
                    ));
                }
            }
        }
    }

    if !visibility_mismatches.is_empty() {
        let mut msg = String::from("Visibility mismatch:\n\n");
        for (qname, expected_vis, actual_vis) in &visibility_mismatches {
            msg.push_str(&format!(
                "  - {qname}: expected {:?}, got {:?}\n",
                expected_vis, actual_vis
            ));
        }
        bail!("{}", msg);
    }

    Ok(())
}

/// Assert that all expected relationships are present in the actual relationships
///
/// Returns an error with detailed diff if any expected relationships are missing.
/// Does not fail on extra relationships (subset matching).
pub fn assert_relationships_match(
    expected: &[ExpectedRelationship],
    actual: &[ActualRelationship],
) -> Result<()> {
    let expected_set: HashSet<(&str, &str, &str)> = expected
        .iter()
        .map(|r| (r.kind.as_neo4j_type(), r.from, r.to))
        .collect();

    let actual_set: HashSet<(&str, &str, &str)> = actual
        .iter()
        .map(|r| {
            (
                r.rel_type.as_str(),
                r.from_qualified_name.as_str(),
                r.to_qualified_name.as_str(),
            )
        })
        .collect();

    let missing: Vec<_> = expected_set.difference(&actual_set).collect();

    if !missing.is_empty() {
        let mut msg = String::from("Relationship mismatch:\n\n");
        msg.push_str("MISSING RELATIONSHIPS:\n");
        for (rel_type, from, to) in &missing {
            msg.push_str(&format!("  - {rel_type} {from} -> {to}\n"));
        }

        msg.push_str("\nACTUAL RELATIONSHIPS:\n");
        for (rel_type, from, to) in &actual_set {
            msg.push_str(&format!("  - {rel_type} {from} -> {to}\n"));
        }

        bail!("{}", msg);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entities_match_success() {
        let expected = vec![ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test::foo",
            visibility: None,
        }];
        let actual = vec![
            ActualEntity {
                entity_id: "id1".to_string(),
                entity_type: "Function".to_string(),
                qualified_name: "test::foo".to_string(),
                name: "foo".to_string(),
                visibility: Some(Visibility::Public),
            },
            ActualEntity {
                entity_id: "id2".to_string(),
                entity_type: "Module".to_string(),
                qualified_name: "test".to_string(),
                name: "test".to_string(),
                visibility: Some(Visibility::Public),
            },
        ];

        assert!(assert_entities_match(&expected, &actual).is_ok());
    }

    #[test]
    fn test_entities_match_missing() {
        let expected = vec![
            ExpectedEntity {
                kind: EntityKind::Function,
                qualified_name: "test::foo",
                visibility: None,
            },
            ExpectedEntity {
                kind: EntityKind::Struct,
                qualified_name: "test::Bar",
                visibility: None,
            },
        ];
        let actual = vec![ActualEntity {
            entity_id: "id1".to_string(),
            entity_type: "Function".to_string(),
            qualified_name: "test::foo".to_string(),
            name: "foo".to_string(),
            visibility: Some(Visibility::Public),
        }];

        let result = assert_entities_match(&expected, &actual);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Struct test::Bar"));
    }

    #[test]
    fn test_entities_match_empty_expected_passes() {
        let expected: Vec<ExpectedEntity> = vec![];
        let actual = vec![ActualEntity {
            entity_id: "id1".to_string(),
            entity_type: "Function".to_string(),
            qualified_name: "test::foo".to_string(),
            name: "foo".to_string(),
            visibility: Some(Visibility::Public),
        }];

        // Empty expected should always pass (subset matching)
        assert!(assert_entities_match(&expected, &actual).is_ok());
    }

    #[test]
    fn test_entities_match_type_mismatch() {
        let expected = vec![ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test::foo",
            visibility: None,
        }];
        let actual = vec![ActualEntity {
            entity_id: "id1".to_string(),
            entity_type: "Struct".to_string(), // Wrong type
            qualified_name: "test::foo".to_string(),
            name: "foo".to_string(),
            visibility: Some(Visibility::Public),
        }];

        let result = assert_entities_match(&expected, &actual);
        assert!(result.is_err());
    }

    #[test]
    fn test_entities_visibility_match() {
        let expected = vec![ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test::foo",
            visibility: Some(Visibility::Public),
        }];
        let actual = vec![ActualEntity {
            entity_id: "id1".to_string(),
            entity_type: "Function".to_string(),
            qualified_name: "test::foo".to_string(),
            name: "foo".to_string(),
            visibility: Some(Visibility::Public),
        }];

        assert!(assert_entities_match(&expected, &actual).is_ok());
    }

    #[test]
    fn test_entities_visibility_mismatch() {
        let expected = vec![ExpectedEntity {
            kind: EntityKind::Function,
            qualified_name: "test::foo",
            visibility: Some(Visibility::Public),
        }];
        let actual = vec![ActualEntity {
            entity_id: "id1".to_string(),
            entity_type: "Function".to_string(),
            qualified_name: "test::foo".to_string(),
            name: "foo".to_string(),
            visibility: Some(Visibility::Private),
        }];

        let result = assert_entities_match(&expected, &actual);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Visibility mismatch"));
    }

    #[test]
    fn test_relationships_match_success() {
        let expected = vec![ExpectedRelationship {
            kind: RelationshipKind::Contains,
            from: "test",
            to: "test::foo",
        }];
        let actual = vec![ActualRelationship {
            rel_type: "CONTAINS".to_string(),
            from_qualified_name: "test".to_string(),
            to_qualified_name: "test::foo".to_string(),
        }];

        assert!(assert_relationships_match(&expected, &actual).is_ok());
    }

    #[test]
    fn test_relationships_match_missing() {
        let expected = vec![ExpectedRelationship {
            kind: RelationshipKind::Calls,
            from: "test::caller",
            to: "test::callee",
        }];
        let actual = vec![];

        let result = assert_relationships_match(&expected, &actual);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("CALLS test::caller -> test::callee"));
    }
}
