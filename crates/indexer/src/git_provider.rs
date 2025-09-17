use anyhow::{anyhow, Result};
use git2::Repository;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use tracing::{debug, trace};

/// Provides Git content access without holding state
#[derive(Clone)]
pub struct GitContentProvider {
    repo_root: PathBuf,
}

impl GitContentProvider {
    /// Create a new provider for the given repository path
    pub fn new(repo_path: &Path) -> Result<Self> {
        // Open repository once to validate and get workdir
        let repo = Repository::open(repo_path)?;
        let repo_root = repo
            .workdir()
            .ok_or_else(|| anyhow!("Repository has no working directory"))?
            .to_path_buf();

        Ok(Self { repo_root })
    }

    /// Get the previous version of a file from Git HEAD
    pub fn get_previous_content(&self, file_path: &Path) -> Result<String> {
        debug!("Fetching previous content for: {:?}", file_path);
        get_previous_content(&self.repo_root, file_path)
    }

    /// Get content from a specific commit
    pub fn get_content_at_commit(&self, file_path: &Path, commit_sha: &str) -> Result<String> {
        debug!(
            "Fetching content at commit {} for: {:?}",
            commit_sha, file_path
        );
        get_content_at_commit(&self.repo_root, file_path, commit_sha)
    }

    /// Check if a file has unstaged changes
    pub fn has_unstaged_changes(&self, file_path: &Path) -> Result<bool> {
        has_unstaged_changes(&self.repo_root, file_path)
    }

    /// Get the last commit that modified a file
    pub fn get_last_commit_for_file(&self, file_path: &Path) -> Result<git2::Oid> {
        get_last_commit_for_file(&self.repo_root, file_path)
    }

    /// Get relative path from repository root
    pub fn get_relative_path(&self, file_path: &Path) -> Result<PathBuf> {
        get_relative_path(&self.repo_root, file_path)
    }
}

// Pure functions that open repository on demand

/// Get the previous version of a file from Git HEAD
pub fn get_previous_content(repo_path: &Path, file_path: &Path) -> Result<String> {
    debug!("Fetching previous content for: {:?}", file_path);

    // Open repository fresh
    let repo = Repository::open(repo_path)?;

    // Get relative path from repo root
    let relative_path = get_relative_path_from_repo(&repo, file_path)?;

    // Get HEAD commit
    let head = repo.head()?;
    let oid = head.target().ok_or_else(|| anyhow!("HEAD has no target"))?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;

    // Find the file in the tree
    let entry = tree
        .get_path(&relative_path)
        .map_err(|e| anyhow!("File not found in HEAD: {}", e))?;

    // Get the blob
    let object = entry.to_object(&repo)?;
    let blob = object
        .as_blob()
        .ok_or_else(|| anyhow!("Object is not a blob"))?;

    // Convert to string
    let content = std::str::from_utf8(blob.content())
        .map_err(|e| anyhow!("Failed to decode file content: {}", e))?
        .to_string();

    trace!("Retrieved {} bytes of previous content", content.len());
    Ok(content)
}

/// Get content from a specific commit
pub fn get_content_at_commit(
    repo_path: &Path,
    file_path: &Path,
    commit_sha: &str,
) -> Result<String> {
    debug!(
        "Fetching content at commit {} for: {:?}",
        commit_sha, file_path
    );

    // Open repository fresh
    let repo = Repository::open(repo_path)?;

    // Get relative path from repo root
    let relative_path = get_relative_path_from_repo(&repo, file_path)?;

    // Find the commit
    let oid = git2::Oid::from_str(commit_sha)?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;

    // Find the file in the tree
    let entry = tree
        .get_path(&relative_path)
        .map_err(|e| anyhow!("File not found at commit {}: {}", commit_sha, e))?;

    // Get the blob
    let object = entry.to_object(&repo)?;
    let blob = object
        .as_blob()
        .ok_or_else(|| anyhow!("Object is not a blob"))?;

    // Convert to string
    let content = std::str::from_utf8(blob.content())
        .map_err(|e| anyhow!("Failed to decode file content: {}", e))?
        .to_string();

    Ok(content)
}

/// Check if a file has unstaged changes
pub fn has_unstaged_changes(repo_path: &Path, file_path: &Path) -> Result<bool> {
    // Open repository fresh
    let repo = Repository::open(repo_path)?;

    let relative_path = get_relative_path_from_repo(&repo, file_path)?;

    // Get HEAD tree
    let head = repo.head()?;
    let head_commit = head.peel_to_commit()?;
    let head_tree = head_commit.tree()?;

    // Get diff between HEAD and working directory
    let mut opts = git2::DiffOptions::new();
    opts.pathspec(&relative_path);

    let diff = repo.diff_tree_to_workdir(Some(&head_tree), Some(&mut opts))?;

    Ok(diff.stats()?.files_changed() > 0)
}

/// Get the last commit that modified a file
pub fn get_last_commit_for_file(repo_path: &Path, file_path: &Path) -> Result<git2::Oid> {
    // Open repository fresh
    let repo = Repository::open(repo_path)?;

    let relative_path = get_relative_path_from_repo(&repo, file_path)?;

    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;

    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;

        // Check if this commit modified the file
        if let Ok(parent) = commit.parent(0) {
            let mut opts = git2::DiffOptions::new();
            opts.pathspec(&relative_path);

            let diff = repo.diff_tree_to_tree(
                Some(&parent.tree()?),
                Some(&commit.tree()?),
                Some(&mut opts),
            )?;

            if diff.stats()?.files_changed() > 0 {
                return Ok(oid);
            }
        } else {
            // This is the initial commit, check if it contains the file
            if commit.tree()?.get_path(&relative_path).is_ok() {
                return Ok(oid);
            }
        }
    }

    Err(anyhow!("No commits found for file: {:?}", file_path))
}

/// Get relative path from repository root
fn get_relative_path(repo_root: &Path, file_path: &Path) -> Result<PathBuf> {
    file_path
        .strip_prefix(repo_root)
        .map(|p| p.to_path_buf())
        .or_else(|_| {
            // Try with canonicalized paths
            let canonical_file = file_path.canonicalize()?;
            let canonical_root = repo_root.canonicalize()?;
            canonical_file
                .strip_prefix(&canonical_root)
                .map(|p| p.to_path_buf())
                .map_err(|e| anyhow!("Failed to get relative path: {}", e))
        })
}

/// Get relative path from repository using the repo's workdir
fn get_relative_path_from_repo(repo: &Repository, file_path: &Path) -> Result<PathBuf> {
    let repo_root = repo
        .workdir()
        .ok_or_else(|| anyhow!("Repository has no working directory"))?;

    get_relative_path(repo_root, file_path)
}

/// LRU Cache for storing file contents to avoid repeated Git operations
pub struct ContentCache {
    cache: LruCache<(PathBuf, Option<String>), String>,
}

impl ContentCache {
    pub fn new(max_size: usize) -> Self {
        // Create LRU cache with specified capacity
        // Default to 100 if max_size is 0
        // Safety: We ensure the value is at least 1 using clamp
        let safe_size = max_size.clamp(1, 10000); // Limit to reasonable max
        let capacity = NonZeroUsize::new(safe_size).unwrap_or(NonZeroUsize::MIN);

        Self {
            cache: LruCache::new(capacity),
        }
    }

    pub fn get(&mut self, file_path: &Path, commit: Option<&str>) -> Option<String> {
        let key = (file_path.to_path_buf(), commit.map(String::from));
        self.cache.get(&key).cloned()
    }

    pub fn insert(&mut self, file_path: PathBuf, commit: Option<String>, content: String) {
        // LRU cache automatically handles eviction
        self.cache.put((file_path, commit), content);
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_cache() {
        let mut cache = ContentCache::new(2);

        cache.insert(PathBuf::from("file1.rs"), None, "content1".to_string());

        cache.insert(
            PathBuf::from("file2.rs"),
            Some("abc123".to_string()),
            "content2".to_string(),
        );

        assert_eq!(
            cache.get(&PathBuf::from("file1.rs"), None),
            Some("content1".to_string())
        );

        // Test eviction - LRU cache will evict least recently used
        cache.insert(PathBuf::from("file3.rs"), None, "content3".to_string());

        assert_eq!(cache.len(), 2);

        // file2.rs should be evicted (least recently used)
        assert_eq!(cache.get(&PathBuf::from("file2.rs"), Some("abc123")), None);

        // file1.rs should still be there (was accessed recently)
        assert_eq!(
            cache.get(&PathBuf::from("file1.rs"), None),
            Some("content1".to_string())
        );
    }
}
