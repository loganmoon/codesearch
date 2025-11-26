//! Repository inference from current working directory
//!
//! Matches CWD against indexed repositories to automatically select
//! the appropriate repository for search queries.

use crate::error::{McpError, Result};
use std::path::Path;
use uuid::Uuid;

/// Repository info from database
#[derive(Debug, Clone)]
pub struct IndexedRepository {
    pub id: Uuid,
    pub name: String,
    pub path: String,
}

/// Infer repository from current working directory
///
/// Walks up the directory tree from CWD looking for a match
/// against indexed repository paths.
pub fn infer_repository_from_cwd(
    cwd: &Path,
    indexed_repos: &[IndexedRepository],
) -> Result<Vec<String>> {
    if indexed_repos.is_empty() {
        return Err(McpError::RepositoryInference(
            "No indexed repositories found. Run 'codesearch index' first.".to_string(),
        ));
    }

    // Try to find a repository that contains the CWD
    let cwd_canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());

    for repo in indexed_repos {
        let repo_path = Path::new(&repo.path);
        let repo_canonical = repo_path
            .canonicalize()
            .unwrap_or_else(|_| repo_path.to_path_buf());

        // Check if CWD is within this repository
        if cwd_canonical.starts_with(&repo_canonical) {
            return Ok(vec![repo.id.to_string()]);
        }
    }

    // No match found - return error with helpful message
    let repo_list: Vec<_> = indexed_repos
        .iter()
        .map(|r| format!("  - {} ({})", r.name, r.path))
        .collect();

    Err(McpError::RepositoryInference(format!(
        "Current directory '{}' is not within any indexed repository.\n\nIndexed repositories:\n{}",
        cwd.display(),
        repo_list.join("\n")
    )))
}

/// Parse repository specification from request
///
/// Handles:
/// - Empty/None: infer from CWD
/// - `["all"]`: return all repository IDs
/// - Specific names/UUIDs: resolve to repository IDs
pub fn resolve_repositories(
    requested: &Option<Vec<String>>,
    cwd: &Path,
    indexed_repos: &[IndexedRepository],
) -> Result<Vec<String>> {
    match requested {
        None => infer_repository_from_cwd(cwd, indexed_repos),
        Some(repos) if repos.is_empty() => infer_repository_from_cwd(cwd, indexed_repos),
        Some(repos) if repos.len() == 1 && repos[0].to_lowercase() == "all" => {
            Ok(indexed_repos.iter().map(|r| r.id.to_string()).collect())
        }
        Some(repos) => {
            let mut resolved = Vec::new();
            for repo_spec in repos {
                // Try to parse as UUID first
                if let Ok(uuid) = Uuid::parse_str(repo_spec) {
                    if indexed_repos.iter().any(|r| r.id == uuid) {
                        resolved.push(uuid.to_string());
                        continue;
                    }
                }

                // Try to match by name
                if let Some(repo) = indexed_repos.iter().find(|r| r.name == *repo_spec) {
                    resolved.push(repo.id.to_string());
                    continue;
                }

                // Try to match by path
                if let Some(repo) = indexed_repos.iter().find(|r| r.path == *repo_spec) {
                    resolved.push(repo.id.to_string());
                    continue;
                }

                return Err(McpError::RepositoryInference(format!(
                    "Repository '{repo_spec}' not found in indexed repositories"
                )));
            }
            Ok(resolved)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_repos() -> Vec<IndexedRepository> {
        vec![
            IndexedRepository {
                id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
                name: "repo-a".to_string(),
                path: "/home/user/projects/repo-a".to_string(),
            },
            IndexedRepository {
                id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
                name: "repo-b".to_string(),
                path: "/home/user/projects/repo-b".to_string(),
            },
        ]
    }

    #[test]
    fn test_resolve_all() {
        let repos = make_repos();
        let result =
            resolve_repositories(&Some(vec!["all".to_string()]), Path::new("/tmp"), &repos);
        assert!(result.is_ok());
        let ids = result.unwrap();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_resolve_by_name() {
        let repos = make_repos();
        let result =
            resolve_repositories(&Some(vec!["repo-a".to_string()]), Path::new("/tmp"), &repos);
        assert!(result.is_ok());
        let ids = result.unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], "11111111-1111-1111-1111-111111111111");
    }

    #[test]
    fn test_resolve_by_uuid() {
        let repos = make_repos();
        let result = resolve_repositories(
            &Some(vec!["22222222-2222-2222-2222-222222222222".to_string()]),
            Path::new("/tmp"),
            &repos,
        );
        assert!(result.is_ok());
        let ids = result.unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], "22222222-2222-2222-2222-222222222222");
    }

    #[test]
    fn test_resolve_unknown_repo() {
        let repos = make_repos();
        let result = resolve_repositories(
            &Some(vec!["unknown".to_string()]),
            Path::new("/tmp"),
            &repos,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_repos_error() {
        let result = resolve_repositories(&None, Path::new("/tmp"), &[]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No indexed repositories"));
    }
}
