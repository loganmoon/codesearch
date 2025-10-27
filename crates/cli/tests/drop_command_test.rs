//! Integration tests for the drop command
//!
//! These tests verify the drop command logic using mock storage backends.
//! Interactive TUI parts are not tested here as they require terminal interaction.

use codesearch_core::error::Result;
use codesearch_storage::{MockPostgresClient, PostgresClientTrait};
use std::path::PathBuf;
use uuid::Uuid;

/// Test helper to create a mock repository entry
fn create_mock_repo(name: &str, path: &str) -> (Uuid, String, PathBuf) {
    (Uuid::new_v4(), name.to_string(), PathBuf::from(path))
}

#[tokio::test]
async fn test_mock_storage_list_repositories() -> Result<()> {
    // Create mock storage client
    let postgres = MockPostgresClient::new();

    // Add some test repositories
    let repo1_path = PathBuf::from("/tmp/test-repo-1");
    let collection1 = "test-collection-1";
    postgres
        .ensure_repository(
            uuid::Uuid::new_v4(),
            &repo1_path,
            collection1,
            Some("test-repo-1"),
        )
        .await?;

    let repo2_path = PathBuf::from("/tmp/test-repo-2");
    let collection2 = "test-collection-2";
    postgres
        .ensure_repository(
            uuid::Uuid::new_v4(),
            &repo2_path,
            collection2,
            Some("test-repo-2"),
        )
        .await?;

    // List all repositories
    let repos = postgres.list_all_repositories().await?;
    assert_eq!(repos.len(), 2, "Should have 2 repositories");

    // Verify repository data
    assert!(repos.iter().any(|(_, name, _)| name == collection1));
    assert!(repos.iter().any(|(_, name, _)| name == collection2));

    Ok(())
}

#[tokio::test]
async fn test_drop_single_repository_with_mock() -> Result<()> {
    // Create mock storage client
    let postgres = MockPostgresClient::new();

    // Create two test repositories
    let repo1_path = PathBuf::from("/tmp/test-repo-drop-1");
    let collection1 = "drop_test_1";
    let repo1_id = postgres
        .ensure_repository(
            uuid::Uuid::new_v4(),
            &repo1_path,
            collection1,
            Some("test-repo-1"),
        )
        .await?;

    let repo2_path = PathBuf::from("/tmp/test-repo-drop-2");
    let collection2 = "drop_test_2";
    let repo2_id = postgres
        .ensure_repository(
            uuid::Uuid::new_v4(),
            &repo2_path,
            collection2,
            Some("test-repo-2"),
        )
        .await?;

    // Verify both exist
    let repos = postgres.list_all_repositories().await?;
    assert_eq!(repos.len(), 2, "Should have 2 repositories before drop");

    // Drop repo1
    postgres.drop_repository(repo1_id).await?;

    // Verify only repo2 remains
    let repos = postgres.list_all_repositories().await?;
    assert_eq!(repos.len(), 1, "Should have 1 repository after drop");
    assert_eq!(repos[0].0, repo2_id, "Remaining repository should be repo2");
    assert_eq!(repos[0].1, collection2, "Collection name should match");

    Ok(())
}

#[tokio::test]
async fn test_drop_all_repositories_with_mock() -> Result<()> {
    // Create mock storage clients
    let postgres = MockPostgresClient::new();

    // Create three test repositories
    let repos_to_create = vec![
        ("/tmp/test-repo-1", "collection-1", "repo-1"),
        ("/tmp/test-repo-2", "collection-2", "repo-2"),
        ("/tmp/test-repo-3", "collection-3", "repo-3"),
    ];

    let mut repo_ids = Vec::new();
    for (path, collection, name) in repos_to_create {
        let id = postgres
            .ensure_repository(
                uuid::Uuid::new_v4(),
                &PathBuf::from(path),
                collection,
                Some(name),
            )
            .await?;
        repo_ids.push(id);
    }

    // Verify all exist
    let repos = postgres.list_all_repositories().await?;
    assert_eq!(repos.len(), 3, "Should have 3 repositories");

    // Drop all repositories one by one (simulating "All" selection)
    for repo_id in repo_ids {
        postgres.drop_repository(repo_id).await?;
    }

    // Verify all are gone
    let repos = postgres.list_all_repositories().await?;
    assert_eq!(repos.len(), 0, "Should have 0 repositories after drop all");

    Ok(())
}

#[tokio::test]
async fn test_drop_nonexistent_repository_returns_error() -> Result<()> {
    // Create mock storage client
    let postgres = MockPostgresClient::new();

    // Try to drop a repository that doesn't exist
    let fake_id = Uuid::new_v4();
    let result = postgres.drop_repository(fake_id).await;

    assert!(
        result.is_err(),
        "Should return error for nonexistent repository"
    );
    assert!(
        result.unwrap_err().to_string().contains("not found"),
        "Error message should indicate repository not found"
    );

    Ok(())
}

#[tokio::test]
async fn test_empty_repository_list() -> Result<()> {
    // Create mock storage client with no repositories
    let postgres = MockPostgresClient::new();

    // List repositories
    let repos = postgres.list_all_repositories().await?;
    assert_eq!(repos.len(), 0, "Should have 0 repositories initially");

    Ok(())
}

#[test]
fn test_repository_tuple_structure() {
    // Verify the repository tuple structure we use throughout the codebase
    let repo = create_mock_repo("test-collection", "/tmp/test-repo");

    assert_eq!(repo.1, "test-collection");
    assert_eq!(repo.2, PathBuf::from("/tmp/test-repo"));
}
