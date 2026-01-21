//! File ignore pattern matching
//!
//! This module provides pattern matching for file filtering,
//! supporting glob patterns, gitignore rules, and language-specific filters.

use glob::{Pattern, PatternError};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, trace};

/// Manages file ignore patterns and filtering
///
/// By default, ignores nothing. Only excludes paths that match
/// explicit ignore patterns or are in ignored directories.
#[derive(Clone)]
pub struct IgnoreFilter {
    /// Glob patterns to ignore
    patterns: Arc<Vec<Pattern>>,
    /// Directory names to always ignore
    ignored_dirs: Arc<HashSet<String>>,
    /// File extensions to explicitly exclude
    exclude_extensions: Arc<HashSet<String>>,
    /// Maximum file size to consider (bytes)
    max_file_size: u64,
    /// Base path for resolving relative patterns (typically repository root)
    base_path: Option<Arc<PathBuf>>,
}

impl Default for IgnoreFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl IgnoreFilter {
    /// Create a new ignore filter that ignores nothing by default
    pub fn new() -> Self {
        Self {
            patterns: Arc::new(Vec::new()),
            ignored_dirs: Arc::new(HashSet::new()),
            exclude_extensions: Arc::new(HashSet::new()),
            max_file_size: u64::MAX,
            base_path: None,
        }
    }

    /// Create with builder pattern
    pub fn builder() -> IgnoreFilterBuilder {
        IgnoreFilterBuilder::default()
    }

    /// Check if a path should be ignored
    pub fn should_ignore(&self, path: &Path) -> bool {
        // Check if any component in the path is an ignored directory
        for component in path.components() {
            if let Some(name) = component.as_os_str().to_str() {
                if self.ignored_dirs.contains(name) {
                    trace!("Ignoring path in ignored directory {name}: {path:?}");
                    return true;
                }
            }
        }

        // Check if extension is explicitly excluded
        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy().to_lowercase();
            if self.exclude_extensions.contains(&ext_str) {
                trace!("Ignoring excluded extension: {:?}", path);
                return true;
            }
        }

        // Check glob patterns
        // Convert absolute paths to relative paths for pattern matching
        let path_for_matching = if path.is_absolute() {
            if let Some(base) = &self.base_path {
                path.strip_prefix(base.as_ref()).unwrap_or(path)
            } else {
                path
            }
        } else {
            path
        };

        let path_str = path_for_matching.to_string_lossy();
        for pattern in self.patterns.iter() {
            if pattern.matches(&path_str) {
                debug!("Path {:?} matches ignore pattern", path);
                return true;
            }
        }

        false
    }

    /// Check if a file size exceeds the limit
    pub fn exceeds_size_limit(&self, size: u64) -> bool {
        size > self.max_file_size
    }
}

/// Builder for IgnoreFilter
pub struct IgnoreFilterBuilder {
    patterns: Vec<String>,
    exclude_extensions: Option<HashSet<String>>,
    ignored_dirs: Option<HashSet<String>>,
    max_file_size: u64,
    base_path: Option<PathBuf>,
}

impl Default for IgnoreFilterBuilder {
    fn default() -> Self {
        Self {
            patterns: Vec::new(),
            exclude_extensions: None,
            ignored_dirs: None,
            max_file_size: u64::MAX, // No limit by default
            base_path: None,
        }
    }
}

impl IgnoreFilterBuilder {
    /// Add a glob pattern to ignore
    #[cfg(test)]
    pub fn add_pattern(mut self, pattern: String) -> Self {
        self.patterns.push(pattern);
        self
    }

    /// Add multiple patterns
    pub fn patterns(mut self, patterns: Vec<String>) -> Self {
        self.patterns.extend(patterns);
        self
    }

    /// Set excluded file extensions (will be converted to lowercase)
    #[cfg(test)]
    pub fn exclude_extensions(mut self, extensions: HashSet<String>) -> Self {
        let lowercase_exts: HashSet<_> = extensions.into_iter().map(|s| s.to_lowercase()).collect();
        self.exclude_extensions = Some(lowercase_exts);
        self
    }

    /// Set ignored directory names
    #[cfg(test)]
    pub fn ignored_dirs(mut self, dirs: HashSet<String>) -> Self {
        self.ignored_dirs = Some(dirs);
        self
    }

    /// Set maximum file size
    pub fn max_file_size(mut self, size: u64) -> Self {
        self.max_file_size = size;
        self
    }

    /// Set base path for resolving relative patterns
    pub fn base_path(mut self, path: PathBuf) -> Self {
        self.base_path = Some(path);
        self
    }

    /// Build the ignore filter
    pub fn build(self) -> Result<IgnoreFilter, PatternError> {
        let compiled_patterns = self
            .patterns
            .iter()
            .map(|p| Pattern::new(p))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(IgnoreFilter {
            patterns: Arc::new(compiled_patterns),
            ignored_dirs: Arc::new(self.ignored_dirs.unwrap_or_default()),
            exclude_extensions: Arc::new(self.exclude_extensions.unwrap_or_default()),
            max_file_size: self.max_file_size,
            base_path: self.base_path.map(Arc::new),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ignore_filter_patterns() {
        let filter = IgnoreFilter::builder()
            .add_pattern("*.log".to_string())
            .add_pattern("**/target/**".to_string())
            .build()
            .expect("test setup failed");

        assert!(filter.should_ignore(Path::new("debug.log")));
        assert!(filter.should_ignore(Path::new("path/to/target/debug/app")));
        assert!(!filter.should_ignore(Path::new("main.rs")));
    }

    #[test]
    fn test_ignore_filter_extensions() {
        let mut exclude = HashSet::new();
        exclude.insert("log".to_string());
        exclude.insert("tmp".to_string());

        let filter = IgnoreFilter::builder()
            .exclude_extensions(exclude)
            .build()
            .expect("test setup failed");

        assert!(!filter.should_ignore(Path::new("main.rs")));
        assert!(!filter.should_ignore(Path::new("Cargo.toml")));
        assert!(filter.should_ignore(Path::new("debug.log")));
        assert!(filter.should_ignore(Path::new("temp.tmp")));
    }

    #[test]
    fn test_ignore_filter_directories() {
        let mut dirs = HashSet::new();
        dirs.insert("node_modules".to_string());
        dirs.insert(".git".to_string());
        dirs.insert("target".to_string());

        let filter = IgnoreFilter::builder()
            .ignored_dirs(dirs)
            .build()
            .expect("test setup failed");

        assert!(filter.should_ignore(Path::new("node_modules")));
        assert!(filter.should_ignore(Path::new(".git")));
        assert!(filter.should_ignore(Path::new("target")));
        assert!(!filter.should_ignore(Path::new("src")));
    }

    #[test]
    fn test_ignore_filter_with_base_path() {
        use std::path::PathBuf;

        let base_path = PathBuf::from("/home/user/project");

        let filter = IgnoreFilter::builder()
            .add_pattern("target/**".to_string())
            .add_pattern("node_modules/**".to_string())
            .base_path(base_path.clone())
            .build()
            .expect("test setup failed");

        // Absolute paths should be converted to relative and match
        assert!(filter.should_ignore(Path::new("/home/user/project/target/debug/app")));
        assert!(filter.should_ignore(Path::new("/home/user/project/target/release/lib.so")));
        assert!(filter.should_ignore(Path::new(
            "/home/user/project/node_modules/package/index.js"
        )));

        // Paths not matching the pattern should not be ignored
        assert!(!filter.should_ignore(Path::new("/home/user/project/src/main.rs")));
        assert!(!filter.should_ignore(Path::new("/home/user/project/Cargo.toml")));

        // Relative paths should also work
        assert!(filter.should_ignore(Path::new("target/debug/app")));
        assert!(!filter.should_ignore(Path::new("src/main.rs")));
    }

    #[test]
    fn test_ignore_filter_directory_component_check() {
        let filter = IgnoreFilter::builder()
            .ignored_dirs(
                vec!["target".to_string(), "node_modules".to_string()]
                    .into_iter()
                    .collect(),
            )
            .build()
            .expect("test setup failed");

        // Should ignore if any path component matches ignored directory name
        assert!(filter.should_ignore(Path::new("target")));
        assert!(filter.should_ignore(Path::new("target/debug")));
        assert!(filter.should_ignore(Path::new("project/target/debug/app")));
        assert!(filter.should_ignore(Path::new("/home/user/project/target/debug/app")));
        assert!(filter.should_ignore(Path::new("node_modules")));
        assert!(filter.should_ignore(Path::new("project/node_modules/pkg")));

        // Should not ignore paths that don't contain these directories
        assert!(!filter.should_ignore(Path::new("src")));
        assert!(!filter.should_ignore(Path::new("src/main.rs")));
    }
}
