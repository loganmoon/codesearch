//! Assertion functions for comparing expected vs actual graph structure

use super::schema::{ActualEntity, ActualRelationship, ExpectedEntity, ExpectedRelationship};
use anyhow::{bail, Result};
use std::collections::HashSet;

/// Assert that all expected entities are present in the actual entities
///
/// Returns an error with detailed diff if any expected entities are missing.
/// Does not fail on extra entities (subset matching).
pub fn assert_entities_match(
    expected: &[ExpectedEntity],
    actual: &[ActualEntity],
) -> Result<()> {
    let expected_set: HashSet<(&str, &str)> = expected
        .iter()
        .map(|e| (e.entity_type, e.qualified_name))
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
            msg.push_str(&format!("  - {} {}\n", entity_type, qname));
        }

        msg.push_str("\nACTUAL ENTITIES:\n");
        for (entity_type, qname) in &actual_set {
            msg.push_str(&format!("  - {} {}\n", entity_type, qname));
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
        .map(|r| (r.rel_type, r.from, r.to))
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
            msg.push_str(&format!("  - {} {} -> {}\n", rel_type, from, to));
        }

        msg.push_str("\nACTUAL RELATIONSHIPS:\n");
        for (rel_type, from, to) in &actual_set {
            msg.push_str(&format!("  - {} {} -> {}\n", rel_type, from, to));
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
        let expected = vec![
            ExpectedEntity {
                entity_type: "Function",
                qualified_name: "test::foo",
            },
        ];
        let actual = vec![
            ActualEntity {
                entity_id: "id1".to_string(),
                entity_type: "Function".to_string(),
                qualified_name: "test::foo".to_string(),
                name: "foo".to_string(),
            },
            ActualEntity {
                entity_id: "id2".to_string(),
                entity_type: "Module".to_string(),
                qualified_name: "test".to_string(),
                name: "test".to_string(),
            },
        ];

        assert!(assert_entities_match(&expected, &actual).is_ok());
    }

    #[test]
    fn test_entities_match_missing() {
        let expected = vec![
            ExpectedEntity {
                entity_type: "Function",
                qualified_name: "test::foo",
            },
            ExpectedEntity {
                entity_type: "Struct",
                qualified_name: "test::Bar",
            },
        ];
        let actual = vec![ActualEntity {
            entity_id: "id1".to_string(),
            entity_type: "Function".to_string(),
            qualified_name: "test::foo".to_string(),
            name: "foo".to_string(),
        }];

        let result = assert_entities_match(&expected, &actual);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Struct test::Bar"));
    }

    #[test]
    fn test_relationships_match_success() {
        let expected = vec![ExpectedRelationship {
            rel_type: "CONTAINS",
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
            rel_type: "CALLS",
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
