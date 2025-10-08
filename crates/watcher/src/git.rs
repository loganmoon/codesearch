//! Git repository integration for branch awareness and ignore patterns
//!
//! This module provides Git repository management, branch detection,
//! and `.gitignore` pattern handling.

#![allow(dead_code)]

use codesearch_core::error::{Error, Result};
use git2::{BranchType, Repository, Status, StatusOptions};
use glob::Pattern;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Wrapper around git2::Repository with additional functionality
#[derive(Clone)]
pub struct GitRepository {
    /// Path to the repository root
    repo_path: PathBuf,
    /// Cached gitignore patterns
    ignore_patterns: Arc<Vec<Pattern>>,
}

impl GitRepository {
    /// Open a Git repository at the given path
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let repo = Repository::discover(path)
            .map_err(|e| Error::watcher(format!("Failed to open Git repository: {e}")))?;

        let repo_path = repo
            .workdir()
            .ok_or_else(|| Error::watcher("Repository has no working directory"))?
            .to_path_buf();

        let ignore_patterns = Self::load_gitignore_patterns(&repo_path)?;

        Ok(Self {
            repo_path,
            ignore_patterns: Arc::new(ignore_patterns),
        })
    }

    /// Get the current branch name
    pub fn current_branch(&self) -> Result<String> {
        let repo = self.open_repo()?;
        let head = repo
            .head()
            .map_err(|e| Error::watcher(format!("Failed to get HEAD: {e}")))?;

        if head.is_branch() {
            head.shorthand()
                .map(|s| s.to_string())
                .ok_or_else(|| Error::watcher("Branch name is not UTF-8"))
        } else {
            // Detached HEAD state
            let oid = head
                .target()
                .ok_or_else(|| Error::watcher("HEAD has no target (empty repository?)"))?;
            Ok(format!("detached:{}", &oid.to_string()[..8]))
        }
    }

    /// Check if the repository is in a detached HEAD state
    pub fn is_detached_head(&self) -> Result<bool> {
        let repo = self.open_repo()?;
        let head = repo
            .head()
            .map_err(|e| Error::watcher(format!("Failed to get HEAD: {e}")))?;
        Ok(!head.is_branch())
    }

    /// Get current commit hash (full 40-char SHA-1)
    pub fn current_commit_hash(&self) -> Result<String> {
        let repo = self.open_repo()?;
        let head = repo
            .head()
            .map_err(|e| Error::watcher(format!("Failed to get HEAD: {e}")))?;
        let oid = head
            .target()
            .ok_or_else(|| Error::watcher("HEAD has no target (unborn branch)"))?;
        Ok(oid.to_string())
    }

    /// Get current commit hash (8-char abbreviated)
    pub fn current_commit_short_hash(&self) -> Result<String> {
        let hash = self.current_commit_hash()?;
        Ok(hash.chars().take(8).collect())
    }

    /// List all local branches
    pub fn list_branches(&self) -> Result<Vec<String>> {
        let repo = self.open_repo()?;
        let branches = repo
            .branches(Some(BranchType::Local))
            .map_err(|e| Error::watcher(format!("Failed to list branches: {e}")))?;

        let mut branch_names = Vec::new();
        for branch in branches {
            let (branch, _) =
                branch.map_err(|e| Error::watcher(format!("Failed to iterate branches: {e}")))?;
            if let Ok(Some(name)) = branch.name() {
                branch_names.push(name.to_string());
            }
        }

        Ok(branch_names)
    }

    /// Switch to a different branch
    pub fn checkout_branch(&self, branch_name: &str) -> Result<()> {
        let repo = self.open_repo()?;
        let obj = repo
            .revparse_single(&format!("refs/heads/{branch_name}"))
            .map_err(|e| Error::watcher(format!("Branch '{branch_name}' not found: {e}")))?;

        repo.checkout_tree(&obj, None)
            .map_err(|e| Error::watcher(format!("Failed to checkout tree: {e}")))?;

        repo.set_head(&format!("refs/heads/{branch_name}"))
            .map_err(|e| Error::watcher(format!("Failed to update HEAD: {e}")))?;

        info!("Checked out branch: {}", branch_name);
        Ok(())
    }

    /// Get the status of files in the working directory
    pub fn status(&self) -> Result<Vec<FileStatus>> {
        let repo = self.open_repo()?;
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .include_ignored(false)
            .include_unreadable(false);

        let statuses = repo
            .statuses(Some(&mut opts))
            .map_err(|e| Error::watcher(format!("Failed to get status: {e}")))?;

        let mut file_statuses = Vec::new();
        for entry in statuses.iter() {
            if let Some(path) = entry.path() {
                let status = FileStatus {
                    path: PathBuf::from(path),
                    status: entry.status(),
                };
                file_statuses.push(status);
            }
        }

        Ok(file_statuses)
    }

    /// Check if a path should be ignored according to gitignore rules
    pub fn should_ignore(&self, path: &Path) -> bool {
        // Make path relative to repo root
        let relative_path = match path.strip_prefix(&self.repo_path) {
            Ok(p) => p,
            Err(_) => {
                debug!("Path {:?} is outside repository", path);
                return false;
            }
        };

        let path_str = relative_path.to_string_lossy();

        // Check against gitignore patterns
        for pattern in self.ignore_patterns.iter() {
            if pattern.matches(&path_str) {
                debug!("Path {:?} matches gitignore pattern", path);
                return true;
            }
        }

        false
    }

    /// Load all gitignore patterns from the repository
    fn load_gitignore_patterns(repo_path: &Path) -> Result<Vec<Pattern>> {
        let mut patterns = Vec::new();

        // Load root .gitignore
        let root_gitignore = repo_path.join(".gitignore");
        if root_gitignore.exists() {
            patterns.extend(Self::parse_gitignore_file(&root_gitignore)?);
        }

        // Load .git/info/exclude
        let exclude_file = repo_path.join(".git").join("info").join("exclude");
        if exclude_file.exists() {
            patterns.extend(Self::parse_gitignore_file(&exclude_file)?);
        }

        // ISSUE: Support nested .gitignore files (would require walking the tree)

        Ok(patterns)
    }

    /// Parse a gitignore file and return glob patterns
    fn parse_gitignore_file(path: &Path) -> Result<Vec<Pattern>> {
        let content = fs::read_to_string(path)
            .map_err(|e| Error::watcher(format!("Failed to read gitignore: {e}")))?;

        let mut patterns = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // For directory patterns (ending with /), we need two patterns:
            // one for the directory itself and one for its contents
            if line.ends_with('/') {
                // Pattern for the directory itself
                let dir_name = line.trim_end_matches('/');
                let dir_glob = if let Some(stripped) = dir_name.strip_prefix('/') {
                    stripped.to_string()
                } else {
                    format!("**/{dir_name}")
                };

                if let Ok(pattern) = Pattern::new(&dir_glob) {
                    patterns.push(pattern);
                }
            }

            // Convert gitignore pattern to glob pattern (handles both files and dir contents)
            let glob_pattern = Self::gitignore_to_glob(line);
            match Pattern::new(&glob_pattern) {
                Ok(pattern) => patterns.push(pattern),
                Err(e) => {
                    warn!("Invalid gitignore pattern '{}': {}", line, e);
                }
            }
        }

        Ok(patterns)
    }

    /// Convert a gitignore pattern to a glob pattern
    fn gitignore_to_glob(pattern: &str) -> String {
        let mut glob = String::new();
        let pattern = pattern.trim();

        // Handle negation (we'll skip these for now)
        if let Some(stripped) = pattern.strip_prefix('!') {
            return stripped.to_string();
        }

        // Directory-only patterns
        let is_dir = pattern.ends_with('/');
        let pattern = if is_dir {
            &pattern[..pattern.len() - 1]
        } else {
            pattern
        };

        // Absolute patterns (start with /)
        if let Some(stripped) = pattern.strip_prefix('/') {
            glob.push_str(stripped);
        } else {
            // Relative patterns can match anywhere
            // Always add **/ prefix for relative patterns
            glob.push_str("**/");
            glob.push_str(pattern);
        }

        // Add wildcard for directories
        if is_dir {
            glob.push_str("/**");
        }

        glob
    }

    /// Check if a repository has uncommitted changes
    pub fn has_uncommitted_changes(&self) -> Result<bool> {
        let statuses = self.status()?;
        Ok(!statuses.is_empty())
    }

    /// Get the repository root path
    pub fn root_path(&self) -> &Path {
        &self.repo_path
    }

    /// Check if a path is a Git submodule
    pub fn is_submodule(&self, path: &Path) -> Result<bool> {
        let repo = self.open_repo()?;
        let relative_path = match path.strip_prefix(&self.repo_path) {
            Ok(p) => p,
            Err(_) => return Ok(false),
        };

        // Check if path contains a .git file (submodule indicator)
        let git_file = path.join(".git");
        if git_file.exists() && git_file.is_file() {
            return Ok(true);
        }

        // Check using libgit2 submodule API
        let submodules = repo
            .submodules()
            .map_err(|e| Error::watcher(format!("Failed to list submodules: {e}")))?;

        for submodule in submodules {
            let submodule_path = submodule.path();
            if relative_path.starts_with(submodule_path) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Get changed files between two commits
    ///
    /// Returns a list of file changes with their status (added, modified, deleted).
    /// If `from_commit` is None, uses the first commit in the repository.
    pub fn get_changed_files_between_commits(
        &self,
        from_commit: Option<&str>,
        to_commit: &str,
    ) -> Result<Vec<FileDiffStatus>> {
        let repo = self.open_repo()?;

        // Resolve "to" commit
        let to_oid = repo
            .revparse_single(to_commit)
            .map_err(|e| Error::watcher(format!("Failed to resolve commit {to_commit}: {e}")))?
            .id();
        let to_commit = repo
            .find_commit(to_oid)
            .map_err(|e| Error::watcher(format!("Failed to find commit: {e}")))?;
        let to_tree = to_commit
            .tree()
            .map_err(|e| Error::watcher(format!("Failed to get tree: {e}")))?;

        // Resolve "from" commit (optional)
        let from_tree = if let Some(from_commit) = from_commit {
            let from_oid = repo
                .revparse_single(from_commit)
                .map_err(|e| {
                    Error::watcher(format!("Failed to resolve commit {from_commit}: {e}"))
                })?
                .id();
            let from_commit = repo
                .find_commit(from_oid)
                .map_err(|e| Error::watcher(format!("Failed to find commit: {e}")))?;
            Some(
                from_commit
                    .tree()
                    .map_err(|e| Error::watcher(format!("Failed to get tree: {e}")))?,
            )
        } else {
            None
        };

        // Compute diff
        let diff = repo
            .diff_tree_to_tree(from_tree.as_ref(), Some(&to_tree), None)
            .map_err(|e| Error::watcher(format!("Failed to compute diff: {e}")))?;

        let mut changes = Vec::new();
        diff.foreach(
            &mut |delta, _progress| {
                let status = match delta.status() {
                    git2::Delta::Added => FileDiffChangeType::Added,
                    git2::Delta::Modified => FileDiffChangeType::Modified,
                    git2::Delta::Deleted => FileDiffChangeType::Deleted,
                    git2::Delta::Renamed => {
                        // For renamed files, treat as delete + add
                        if let Some(old_file) = delta.old_file().path() {
                            changes.push(FileDiffStatus {
                                path: self.repo_path.join(old_file),
                                change_type: FileDiffChangeType::Deleted,
                            });
                        }
                        FileDiffChangeType::Added
                    }
                    _ => return true, // Skip other types (typechange, copied, etc.)
                };

                if let Some(new_file) = delta.new_file().path() {
                    let abs_path = self.repo_path.join(new_file);
                    changes.push(FileDiffStatus {
                        path: abs_path,
                        change_type: status,
                    });
                }

                true
            },
            None,
            None,
            None,
        )
        .map_err(|e| Error::watcher(format!("Failed to iterate diff: {e}")))?;

        info!("Found {} changed files between commits", changes.len());

        Ok(changes)
    }

    /// Detect branch changes by monitoring HEAD
    pub async fn watch_for_branch_changes(&self) -> Result<BranchWatcher> {
        BranchWatcher::new(self.repo_path.clone()).await
    }

    /// Open the underlying git2 repository
    fn open_repo(&self) -> Result<Repository> {
        Repository::open(&self.repo_path)
            .map_err(|e| Error::watcher(format!("Failed to open repository: {e}")))
    }
}

/// File status information
#[derive(Debug, Clone)]
pub struct FileStatus {
    /// Path relative to repository root
    pub path: PathBuf,
    /// Git status flags
    pub status: Status,
}

impl FileStatus {
    /// Check if file is modified
    pub fn is_modified(&self) -> bool {
        self.status.contains(Status::WT_MODIFIED) || self.status.contains(Status::INDEX_MODIFIED)
    }

    /// Check if file is new/untracked
    pub fn is_new(&self) -> bool {
        self.status.contains(Status::WT_NEW)
    }

    /// Check if file is deleted
    pub fn is_deleted(&self) -> bool {
        self.status.contains(Status::WT_DELETED) || self.status.contains(Status::INDEX_DELETED)
    }

    /// Check if file is renamed
    pub fn is_renamed(&self) -> bool {
        self.status.contains(Status::WT_RENAMED) || self.status.contains(Status::INDEX_RENAMED)
    }
}

/// File diff status between commits
#[derive(Debug, Clone)]
pub struct FileDiffStatus {
    /// Absolute path to the file
    pub path: PathBuf,
    /// Type of change
    pub change_type: FileDiffChangeType,
}

/// Type of file change in a diff
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDiffChangeType {
    /// File was added
    Added,
    /// File was modified
    Modified,
    /// File was deleted
    Deleted,
}

/// Watches for branch changes in a Git repository
pub struct BranchWatcher {
    repo_path: PathBuf,
    last_branch: String,
}

impl BranchWatcher {
    /// Create a new branch watcher
    pub async fn new(repo_path: PathBuf) -> Result<Self> {
        let repo = GitRepository::open(&repo_path)?;
        let last_branch = repo.current_branch()?;

        Ok(Self {
            repo_path,
            last_branch,
        })
    }

    /// Check if the branch has changed
    pub async fn has_branch_changed(&mut self) -> Result<Option<BranchChange>> {
        let repo = GitRepository::open(&self.repo_path)?;
        let current_branch = repo.current_branch()?;

        if current_branch != self.last_branch {
            let change = BranchChange {
                from: self.last_branch.clone(),
                to: current_branch.clone(),
            };
            self.last_branch = current_branch;
            Ok(Some(change))
        } else {
            Ok(None)
        }
    }

    /// Get the current branch
    pub fn current_branch(&self) -> &str {
        &self.last_branch
    }
}

/// Information about a branch change
#[derive(Debug, Clone)]
pub struct BranchChange {
    /// Previous branch name
    pub from: String,
    /// New branch name
    pub to: String,
}

impl BranchChange {
    /// Check if this represents entering a detached HEAD state
    pub fn is_detaching(&self) -> bool {
        !self.from.starts_with("detached:") && self.to.starts_with("detached:")
    }

    /// Check if this represents leaving a detached HEAD state
    pub fn is_attaching(&self) -> bool {
        self.from.starts_with("detached:") && !self.to.starts_with("detached:")
    }
}

/// Helper to detect Git repository boundaries
pub struct GitDetector;

impl GitDetector {
    /// Find the Git repository root for a given path
    pub fn find_repository_root(path: &Path) -> Option<PathBuf> {
        let mut current = path;
        loop {
            if current.join(".git").exists() {
                return Some(current.to_path_buf());
            }
            current = current.parent()?;
        }
    }

    /// Check if a path is inside a Git repository
    pub fn is_in_repository(path: &Path) -> bool {
        Self::find_repository_root(path).is_some()
    }

    /// Find all Git repositories under a directory
    pub fn find_repositories(root: &Path) -> Result<Vec<PathBuf>> {
        let mut repos = Vec::new();
        Self::find_repositories_recursive(root, &mut repos, &mut HashSet::new())?;
        Ok(repos)
    }

    fn find_repositories_recursive(
        dir: &Path,
        repos: &mut Vec<PathBuf>,
        visited: &mut HashSet<PathBuf>,
    ) -> Result<()> {
        // Avoid infinite loops with symlinks
        if !visited.insert(dir.to_path_buf()) {
            return Ok(());
        }

        // Check if this directory is a Git repository
        if dir.join(".git").exists() {
            repos.push(dir.to_path_buf());
            // Don't recurse into Git repositories
            return Ok(());
        }

        // Recurse into subdirectories
        let entries = fs::read_dir(dir)
            .map_err(|e| Error::watcher(format!("Failed to read directory: {e}")))?;

        for entry in entries {
            let entry = entry.map_err(|e| Error::watcher(format!("Failed to read entry: {e}")))?;
            let path = entry.path();

            if path.is_dir() {
                // Skip common non-repository directories
                let name = path.file_name().and_then(|n| n.to_str());
                if let Some(name) = name {
                    if name == "node_modules" || name == "target" || name == ".git" {
                        continue;
                    }
                }

                if let Err(e) = Self::find_repositories_recursive(&path, repos, visited) {
                    // Log error but continue with other directories
                    debug!("Error scanning directory {:?}: {}", path, e);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_repo() -> (TempDir, GitRepository) {
        let temp_dir = TempDir::new().expect("test setup failed");
        let repo = Repository::init(temp_dir.path()).expect("test setup failed");

        // Configure test repo
        let mut config = repo.config().expect("test setup failed");
        config
            .set_str("user.name", "Test User")
            .expect("test setup failed");
        config
            .set_str("user.email", "test@example.com")
            .expect("test setup failed");

        // Create initial commit
        let sig = repo.signature().expect("test setup failed");
        let tree_id = {
            let mut index = repo.index().expect("test setup failed");
            index.write_tree().expect("test setup failed")
        };
        let tree = repo.find_tree(tree_id).expect("test setup failed");
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .expect("test setup failed");

        let git_repo = GitRepository::open(temp_dir.path()).expect("test setup failed");
        (temp_dir, git_repo)
    }

    #[test]
    fn test_current_branch() {
        let (_temp_dir, git_repo) = setup_test_repo();
        let branch = git_repo.current_branch().expect("test setup failed");
        // Default branch could be main or master depending on git config
        assert!(branch == "main" || branch == "master");
    }

    #[test]
    fn test_gitignore_to_glob() {
        assert_eq!(GitRepository::gitignore_to_glob("*.log"), "**/*.log");
        assert_eq!(GitRepository::gitignore_to_glob("/build"), "build");
        assert_eq!(GitRepository::gitignore_to_glob("temp/"), "**/temp/**");
        assert_eq!(GitRepository::gitignore_to_glob("/docs/"), "docs/**");
    }

    #[test]
    fn test_git_detector() {
        let temp_dir = TempDir::new().expect("test setup failed");
        let repo_path = temp_dir.path().join("repo");
        fs::create_dir(&repo_path).expect("test setup failed");
        Repository::init(&repo_path).expect("test setup failed");

        assert!(GitDetector::is_in_repository(&repo_path));
        assert_eq!(
            GitDetector::find_repository_root(&repo_path),
            Some(repo_path.clone())
        );

        let repos = GitDetector::find_repositories(temp_dir.path()).expect("test setup failed");
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0], repo_path);
    }

    #[tokio::test]
    async fn test_branch_watcher() {
        let (temp_dir, _git_repo) = setup_test_repo();
        let mut watcher = BranchWatcher::new(temp_dir.path().to_path_buf())
            .await
            .expect("test setup failed");

        // Initially no change
        assert!(watcher
            .has_branch_changed()
            .await
            .expect("test setup failed")
            .is_none());

        // Create and switch to a new branch
        let repo = Repository::open(temp_dir.path()).expect("test setup failed");
        let _sig = repo.signature().expect("test setup failed");
        let head = repo.head().expect("test setup failed");
        let oid = head.target().expect("test setup failed");
        let commit = repo.find_commit(oid).expect("test setup failed");
        repo.branch("test-branch", &commit, false)
            .expect("test setup failed");

        let obj = repo
            .revparse_single("refs/heads/test-branch")
            .expect("test setup failed");
        repo.checkout_tree(&obj, None).expect("test setup failed");
        repo.set_head("refs/heads/test-branch")
            .expect("test setup failed");

        // Now should detect change
        let change = watcher
            .has_branch_changed()
            .await
            .expect("test setup failed");
        assert!(change.is_some());
        let change = change.expect("test setup failed");
        assert!(change.from == "main" || change.from == "master");
        assert_eq!(change.to, "test-branch");
    }
}
