//! Custom assertions for E2E tests

use super::containers::TestQdrant;
use super::fixtures::ExpectedEntity;
use anyhow::{Context, Result};
use serde::Deserialize;

/// Assert that a collection exists in Qdrant
pub async fn assert_collection_exists(qdrant: &TestQdrant, collection_name: &str) -> Result<()> {
    let url = format!("{}/collections/{}", qdrant.rest_url(), collection_name);
    let response = reqwest::get(&url)
        .await
        .context("Failed to query Qdrant collections endpoint")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Collection '{collection_name}' does not exist. Status: {}",
            response.status()
        ));
    }

    Ok(())
}

/// Assert that a collection has the expected number of points
pub async fn assert_point_count(
    qdrant: &TestQdrant,
    collection_name: &str,
    expected: usize,
) -> Result<()> {
    let url = format!("{}/collections/{}", qdrant.rest_url(), collection_name);
    let response = reqwest::get(&url)
        .await
        .context("Failed to query Qdrant collection info")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to get collection info. Status: {}",
            response.status()
        ));
    }

    let info: CollectionInfo = response
        .json()
        .await
        .context("Failed to parse collection info")?;

    let actual = info.result.points_count;
    if actual != expected {
        return Err(anyhow::anyhow!(
            "Expected {expected} points but found {actual} in collection '{collection_name}'"
        ));
    }

    Ok(())
}

/// Get the current point count for a collection
pub async fn get_point_count(qdrant: &TestQdrant, collection_name: &str) -> Result<usize> {
    let url = format!("{}/collections/{}", qdrant.rest_url(), collection_name);
    let response = reqwest::get(&url)
        .await
        .context("Failed to query Qdrant collection info")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to get collection info. Status: {}",
            response.status()
        ));
    }

    let info: CollectionInfo = response
        .json()
        .await
        .context("Failed to parse collection info")?;

    Ok(info.result.points_count)
}

/// Assert that a collection has at least the minimum number of points
pub async fn assert_min_point_count(
    qdrant: &TestQdrant,
    collection_name: &str,
    minimum: usize,
) -> Result<()> {
    let actual = get_point_count(qdrant, collection_name).await?;
    if actual < minimum {
        return Err(anyhow::anyhow!(
            "Expected at least {minimum} points but found {actual} in collection '{collection_name}'"
        ));
    }

    Ok(())
}

/// Assert that an expected entity exists in Qdrant
pub async fn assert_entity_in_qdrant(
    qdrant: &TestQdrant,
    collection_name: &str,
    expected: &ExpectedEntity,
) -> Result<()> {
    // Scroll through all points to find matching entity
    let url = format!(
        "{}/collections/{}/points/scroll",
        qdrant.rest_url(),
        collection_name
    );

    let client = reqwest::Client::new();
    let mut offset: Option<serde_json::Value> = None;
    let mut found = false;

    // Scroll through points in batches
    loop {
        let mut body = serde_json::json!({
            "limit": 100,
            "with_payload": true,
            "with_vector": false,
        });

        if let Some(ref offset_val) = offset {
            body["offset"] = offset_val.clone();
        }

        let response = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to scroll points")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to scroll points. Status: {}",
                response.status()
            ));
        }

        let scroll_result: ScrollResult = response
            .json()
            .await
            .context("Failed to parse scroll result")?;

        // Check each point's payload
        for point in &scroll_result.result.points {
            if let Some(payload) = &point.payload {
                if let (Some(name), Some(entity_type), Some(file_path)) = (
                    payload.get("name").and_then(|v| v.as_str()),
                    payload.get("entity_type").and_then(|v| v.as_str()),
                    payload.get("file_path").and_then(|v| v.as_str()),
                ) {
                    // EntityType serializes as snake_case (e.g., "struct" not "Struct")
                    let expected_type = format!("{:?}", expected.entity_type).to_lowercase();
                    if name == expected.name
                        && entity_type.eq_ignore_ascii_case(&expected_type)
                        && file_path.contains(&expected.file_path_contains)
                    {
                        found = true;
                        break;
                    }
                }
            }
        }

        if found {
            break;
        }

        // Check if there are more points
        if let Some(next_offset) = scroll_result.result.next_page_offset {
            offset = Some(next_offset);
        } else {
            break;
        }
    }

    if !found {
        return Err(anyhow::anyhow!(
            "Expected entity not found: {} ({:?}) in file containing '{}'",
            expected.name,
            expected.entity_type,
            expected.file_path_contains
        ));
    }

    Ok(())
}

/// Assert that the collection has the correct vector dimensions
pub async fn assert_vector_dimensions(
    qdrant: &TestQdrant,
    collection_name: &str,
    expected_dims: usize,
) -> Result<()> {
    let url = format!("{}/collections/{}", qdrant.rest_url(), collection_name);
    let response = reqwest::get(&url)
        .await
        .context("Failed to query Qdrant collection info")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to get collection info. Status: {}",
            response.status()
        ));
    }

    let info: CollectionInfo = response
        .json()
        .await
        .context("Failed to parse collection info")?;

    let actual_dims = info.result.config.params.vectors.size;
    if actual_dims != expected_dims {
        return Err(anyhow::anyhow!(
            "Expected vector dimensions {expected_dims} but found {actual_dims}"
        ));
    }

    Ok(())
}

// =============================================================================
// Response structures for Qdrant REST API
// =============================================================================

#[derive(Debug, Deserialize)]
struct CollectionInfo {
    result: CollectionResult,
}

#[derive(Debug, Deserialize)]
struct CollectionResult {
    points_count: usize,
    config: CollectionConfig,
}

#[derive(Debug, Deserialize)]
struct CollectionConfig {
    params: CollectionParams,
}

#[derive(Debug, Deserialize)]
struct CollectionParams {
    vectors: VectorParams,
}

#[derive(Debug, Deserialize)]
struct VectorParams {
    size: usize,
}

#[derive(Debug, Deserialize)]
struct ScrollResult {
    result: ScrollResultData,
}

#[derive(Debug, Deserialize)]
struct ScrollResultData {
    points: Vec<Point>,
    next_page_offset: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Point {
    payload: Option<serde_json::Map<String, serde_json::Value>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires Docker
    async fn test_assertions_with_real_qdrant() -> Result<()> {
        // This test requires a running Qdrant instance
        let qdrant = TestQdrant::start().await?;

        // Create a test collection
        let collection_name = format!("test_collection_{}", uuid::Uuid::new_v4());
        let url = format!("{}/collections/{}", qdrant.rest_url(), collection_name);

        let client = reqwest::Client::new();
        let create_body = serde_json::json!({
            "vectors": {
                "size": 384,
                "distance": "Cosine"
            }
        });

        client.put(&url).json(&create_body).send().await?;

        // Test assert_collection_exists
        assert_collection_exists(&qdrant, &collection_name).await?;

        // Test assert_point_count (should be 0 for new collection)
        assert_point_count(&qdrant, &collection_name, 0).await?;

        // Test assert_vector_dimensions
        assert_vector_dimensions(&qdrant, &collection_name, 384).await?;

        Ok(())
    }
}
