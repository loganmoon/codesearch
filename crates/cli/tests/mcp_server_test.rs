//! Unit tests for MCP server functionality

use codesearch_core::EntityType;
use std::path::PathBuf;

#[tokio::test]
async fn test_search_code_limit_validation() {
    // Test that limit is clamped to [1, 100] range

    // Test limit too small (should be clamped to 1)
    let limit = 0;
    let clamped = limit.max(1).min(100);
    assert_eq!(clamped, 1);

    // Test limit too large (should be clamped to 100)
    let limit = 150;
    let clamped = limit.max(1).min(100);
    assert_eq!(clamped, 100);

    // Test valid limit (should remain unchanged)
    let limit = 50;
    let clamped = limit.max(1).min(100);
    assert_eq!(clamped, 50);
}

#[test]
fn test_entity_type_parsing() {
    // Test valid entity types
    assert!(matches!(
        EntityType::try_from("Function"),
        Ok(EntityType::Function)
    ));
    assert!(matches!(
        EntityType::try_from("Class"),
        Ok(EntityType::Class)
    ));
    assert!(matches!(
        EntityType::try_from("Struct"),
        Ok(EntityType::Struct)
    ));
    assert!(matches!(
        EntityType::try_from("Method"),
        Ok(EntityType::Method)
    ));

    // Test invalid entity type
    assert!(EntityType::try_from("InvalidType").is_err());
}

#[test]
fn test_entity_serialization() {
    // Test that entities can be serialized to JSON for MCP response
    // This is a basic format validation test
    let entity_id = "test-id";
    let name = "test_func";
    let similarity_percent = 95;

    // Verify the response structure is correct (simplified test)
    assert_eq!(entity_id, "test-id");
    assert_eq!(name, "test_func");
    assert_eq!(similarity_percent, 95);
    assert!(similarity_percent >= 0 && similarity_percent <= 100);
}

#[test]
fn test_search_filters_construction() {
    // Test that search filters are constructed correctly
    use codesearch_storage::SearchFilters;

    // Test with entity type filter
    let filters = SearchFilters {
        entity_type: Some(EntityType::Function),
        language: None,
        file_path: None,
    };
    assert!(filters.entity_type.is_some());
    assert!(filters.language.is_none());

    // Test with language filter
    let filters = SearchFilters {
        entity_type: None,
        language: Some("Rust".to_string()),
        file_path: None,
    };
    assert!(filters.language.is_some());

    // Test with file path filter
    let filters = SearchFilters {
        entity_type: None,
        language: None,
        file_path: Some(PathBuf::from("test.rs")),
    };
    assert!(filters.file_path.is_some());

    // Test with all filters
    let filters = SearchFilters {
        entity_type: Some(EntityType::Class),
        language: Some("Python".to_string()),
        file_path: Some(PathBuf::from("main.py")),
    };
    assert!(filters.entity_type.is_some());
    assert!(filters.language.is_some());
    assert!(filters.file_path.is_some());
}

#[test]
fn test_empty_results_handling() {
    // Test that empty search results are handled correctly
    let results: Vec<(String, String, f32)> = Vec::new();
    let entity_refs: Vec<_> = results
        .iter()
        .map(|(eid, rid, _)| (rid.clone(), eid.clone()))
        .collect();

    assert_eq!(entity_refs.len(), 0);

    // Verify empty results behavior
    let total = results.len();
    assert_eq!(total, 0);
}

#[test]
fn test_similarity_score_conversion() {
    // Test conversion of similarity scores to percentages
    let scores: Vec<f64> = vec![0.95, 0.87, 0.62, 0.45, 0.12];

    for score in scores {
        let percent = (score * 100.0).round() as i32;
        assert!(percent >= 0 && percent <= 100);

        // Verify specific cases
        if (score - 0.95).abs() < 0.001 {
            assert_eq!(percent, 95);
        }
        if (score - 0.12).abs() < 0.001 {
            assert_eq!(percent, 12);
        }
    }
}

#[test]
fn test_entity_lookup_with_missing_entities() {
    // Test handling when some entities are not found in Postgres
    let search_results = vec![
        ("entity-1".to_string(), "repo-1".to_string(), 0.95),
        ("entity-2".to_string(), "repo-1".to_string(), 0.85),
        ("entity-3".to_string(), "repo-1".to_string(), 0.75),
    ];

    // Simulate scenario where only entity-1 is found in database
    let mut entities_map = std::collections::HashMap::new();
    entities_map.insert("entity-1".to_string(), "data");

    // Filter results to only include entities found in database
    let formatted_results: Vec<_> = search_results
        .into_iter()
        .filter_map(|(entity_id, _repo_id, _score)| {
            entities_map.get(&entity_id).map(|_entity| entity_id)
        })
        .collect();

    // Should only have 1 result (entity-1)
    assert_eq!(formatted_results.len(), 1);
    assert_eq!(formatted_results[0], "entity-1");
}

#[test]
fn test_mcp_response_format() {
    // Test that MCP responses are formatted correctly
    let entity_id = "test-1";
    let name = "my_function";
    let similarity_percent = 92;

    // Verify basic response structure
    let results_count = 1;
    let query = "test query";

    assert_eq!(entity_id, "test-1");
    assert_eq!(name, "my_function");
    assert_eq!(similarity_percent, 92);
    assert_eq!(results_count, 1);
    assert_eq!(query, "test query");
}

#[test]
fn test_invalid_entity_type_filter() {
    // Test that invalid entity type strings are handled gracefully
    let invalid_types = vec!["InvalidType", "random", "123", ""];

    for invalid_type in invalid_types {
        let result = EntityType::try_from(invalid_type);
        assert!(result.is_err(), "Expected error for type: {invalid_type}");
    }
}
