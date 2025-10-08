//! File ignore pattern matching
//!
//! This module provides pattern matching for file filtering,
//! supporting glob patterns, gitignore rules, and language-specific filters.

#![allow(dead_code)]

use glob::{Pattern, PatternError};
use std::collections::HashSet;
use std::path::Path;
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
    /// Whether to follow symbolic links
    follow_symlinks: bool,
    /// Maximum file size to consider (bytes)
    max_file_size: u64,
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
            follow_symlinks: true,
            max_file_size: u64::MAX,
        }
    }

    /// Create a filter from patterns
    pub fn from_patterns(patterns: Vec<String>) -> Result<Self, PatternError> {
        let compiled_patterns = patterns
            .iter()
            .map(|p| Pattern::new(p))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            patterns: Arc::new(compiled_patterns),
            ignored_dirs: Arc::new(HashSet::new()),
            exclude_extensions: Arc::new(HashSet::new()),
            follow_symlinks: true,
            max_file_size: u64::MAX,
        })
    }

    /// Create with builder pattern
    pub fn builder() -> IgnoreFilterBuilder {
        IgnoreFilterBuilder::default()
    }

    /// Check if a path should be ignored
    pub fn should_ignore(&self, path: &Path) -> bool {
        // Check if it's in an ignored directory
        if let Some(file_name) = path.file_name() {
            let name = file_name.to_string_lossy();
            if self.ignored_dirs.contains(name.as_ref()) {
                trace!("Ignoring directory: {:?}", path);
                return true;
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
        let path_str = path.to_string_lossy();
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

    /// Check if we should follow a symlink
    pub fn should_follow_symlink(&self, path: &Path) -> bool {
        self.follow_symlinks && !self.should_ignore(path)
    }
}

/// Builder for IgnoreFilter
pub struct IgnoreFilterBuilder {
    patterns: Vec<String>,
    exclude_extensions: Option<HashSet<String>>,
    ignored_dirs: Option<HashSet<String>>,
    follow_symlinks: bool,
    max_file_size: u64,
}

impl Default for IgnoreFilterBuilder {
    fn default() -> Self {
        Self {
            patterns: Vec::new(),
            exclude_extensions: None,
            ignored_dirs: None,
            follow_symlinks: true,
            max_file_size: u64::MAX, // No limit by default
        }
    }
}

impl IgnoreFilterBuilder {
    /// Add a glob pattern to ignore
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
    pub fn exclude_extensions(mut self, extensions: HashSet<String>) -> Self {
        let lowercase_exts: HashSet<_> = extensions.into_iter().map(|s| s.to_lowercase()).collect();
        self.exclude_extensions = Some(lowercase_exts);
        self
    }

    /// Set ignored directory names
    pub fn ignored_dirs(mut self, dirs: HashSet<String>) -> Self {
        self.ignored_dirs = Some(dirs);
        self
    }

    /// Set whether to follow symbolic links
    pub fn follow_symlinks(mut self, follow: bool) -> Self {
        self.follow_symlinks = follow;
        self
    }

    /// Set maximum file size
    pub fn max_file_size(mut self, size: u64) -> Self {
        self.max_file_size = size;
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
            follow_symlinks: self.follow_symlinks,
            max_file_size: self.max_file_size,
        })
    }
}

/// Language-specific file filter
pub struct LanguageFilter {
    /// Language name
    language: String,
    /// File extensions for this language
    extensions: HashSet<String>,
    /// Common file patterns (e.g., "Makefile", "Dockerfile")
    patterns: Vec<Pattern>,
}

impl LanguageFilter {
    /// Create a filter for Python files
    pub fn python() -> Self {
        Self {
            language: "Python".to_string(),
            extensions: vec!["py", "pyi", "pyx", "pxd"]
                .into_iter()
                .map(String::from)
                .collect(),
            patterns: vec![
                "**/requirements.txt",
                "**/requirements*.txt",
                "**/setup.py",
                "**/pyproject.toml",
                "**/Pipfile",
            ]
            .into_iter()
            .filter_map(|p| Pattern::new(p).ok())
            .collect(),
        }
    }

    /// Create a filter for JavaScript/TypeScript files
    pub fn javascript() -> Self {
        Self {
            language: "JavaScript".to_string(),
            extensions: vec!["js", "jsx", "ts", "tsx", "mjs", "cjs"]
                .into_iter()
                .map(String::from)
                .collect(),
            patterns: vec![
                "**/package.json",
                "**/tsconfig.json",
                "**/.eslintrc*",
                "**/webpack.config.js",
            ]
            .into_iter()
            .filter_map(|p| Pattern::new(p).ok())
            .collect(),
        }
    }

    /// Create a filter for Rust files
    pub fn rust() -> Self {
        Self {
            language: "Rust".to_string(),
            extensions: vec!["rs"].into_iter().map(String::from).collect(),
            patterns: vec!["**/Cargo.toml", "**/Cargo.lock", "**/build.rs"]
                .into_iter()
                .filter_map(|p| Pattern::new(p).ok())
                .collect(),
        }
    }

    /// Create a filter for Go files
    pub fn go() -> Self {
        Self {
            language: "Go".to_string(),
            extensions: vec!["go"].into_iter().map(String::from).collect(),
            patterns: vec!["**/go.mod", "**/go.sum"]
                .into_iter()
                .filter_map(|p| Pattern::new(p).ok())
                .collect(),
        }
    }

    /// Check if a file matches this language filter
    pub fn matches(&self, path: &Path) -> bool {
        // Check extension
        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy().to_lowercase();
            if self.extensions.contains(&ext_str) {
                return true;
            }
        }

        // Check patterns
        let path_str = path.to_string_lossy();
        for pattern in &self.patterns {
            if pattern.matches(&path_str) {
                return true;
            }
        }

        false
    }

    /// Get the language name
    pub fn language_name(&self) -> &str {
        &self.language
    }
}

/// Composite filter combining multiple language filters
pub struct CompositeLanguageFilter {
    filters: Vec<LanguageFilter>,
}

impl CompositeLanguageFilter {
    /// Create a new composite filter
    pub fn new(filters: Vec<LanguageFilter>) -> Self {
        Self { filters }
    }

    /// Create a filter for common programming languages
    pub fn common() -> Self {
        Self {
            filters: vec![
                LanguageFilter::python(),
                LanguageFilter::javascript(),
                LanguageFilter::rust(),
                LanguageFilter::go(),
            ],
        }
    }

    /// Check if a file matches any language filter
    pub fn matches(&self, path: &Path) -> Option<&str> {
        for filter in &self.filters {
            if filter.matches(path) {
                return Some(filter.language_name());
            }
        }
        None
    }

    /// Add a language filter
    pub fn add_filter(&mut self, filter: LanguageFilter) {
        self.filters.push(filter);
    }
}

/// Helper to check if a path is hidden (starts with .)
pub fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.'))
        .unwrap_or(false)
}

/// Helper to check if a path is likely a binary file
pub fn is_likely_binary(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext_str = ext.to_string_lossy().to_lowercase();
        matches!(
            ext_str.as_str(),
            "exe"
                | "dll"
                | "so"
                | "dylib"
                | "a"
                | "o"
                | "obj"
                | "lib"
                | "pdb"
                | "class"
                | "jar"
                | "war"
                | "ear"
                | "pyc"
                | "pyo"
                | "beam"
                | "wasm"
                | "elc"
                | "fasl"
                | "rlib"
                | "rmeta"
        )
    } else {
        false
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
    fn test_language_filter_python() {
        let filter = LanguageFilter::python();

        assert!(filter.matches(Path::new("main.py")));
        assert!(filter.matches(Path::new("test.pyi")));
        assert!(filter.matches(Path::new("requirements.txt")));
        assert!(filter.matches(Path::new("setup.py")));
        assert!(!filter.matches(Path::new("main.rs")));
    }

    #[test]
    fn test_language_filter_rust() {
        let filter = LanguageFilter::rust();

        assert!(filter.matches(Path::new("main.rs")));
        assert!(filter.matches(Path::new("Cargo.toml")));
        assert!(filter.matches(Path::new("build.rs")));
        assert!(!filter.matches(Path::new("main.py")));
    }

    #[test]
    fn test_composite_language_filter() {
        let filter = CompositeLanguageFilter::common();

        assert_eq!(filter.matches(Path::new("main.py")), Some("Python"));
        assert_eq!(filter.matches(Path::new("app.js")), Some("JavaScript"));
        assert_eq!(filter.matches(Path::new("main.rs")), Some("Rust"));
        assert_eq!(filter.matches(Path::new("main.go")), Some("Go"));
        assert_eq!(filter.matches(Path::new("main.c")), None);
    }

    #[test]
    fn test_is_hidden() {
        assert!(is_hidden(Path::new(".git")));
        assert!(is_hidden(Path::new(".gitignore")));
        assert!(!is_hidden(Path::new("main.rs")));
        assert!(!is_hidden(Path::new("src/.hidden/file.txt"))); // Only checks filename
    }

    #[test]
    fn test_is_likely_binary() {
        assert!(is_likely_binary(Path::new("app.exe")));
        assert!(is_likely_binary(Path::new("lib.so")));
        assert!(is_likely_binary(Path::new("module.pyc")));
        assert!(is_likely_binary(Path::new("app.jar")));
        assert!(!is_likely_binary(Path::new("main.rs")));
        assert!(!is_likely_binary(Path::new("script.py")));
    }
}
