//! Project manifest detection and parsing
//!
//! This module provides functionality for detecting and parsing project manifests
//! (Cargo.toml, pyproject.toml, package.json) to extract package names and source roots.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use tracing::debug;

/// Detected project metadata from manifest files
#[derive(Debug, Clone)]
pub struct ProjectManifest {
    pub project_type: ProjectType,
    pub packages: PackageMap,
}

/// Type of project detected
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectType {
    RustCrate,
    RustWorkspace,
    PythonPackage,
    NodePackage,
    NodeWorkspace,
    Unknown,
}

/// Information about a single package/crate
#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub name: String,
    pub source_root: PathBuf,
}

/// Maps file paths to their containing package
///
/// Packages are stored sorted by path depth (deepest first) to enable
/// longest-prefix matching when looking up packages for files.
#[derive(Debug, Clone, Default)]
pub struct PackageMap {
    /// Sorted by path depth (deepest first) for longest-prefix matching
    packages: Vec<(PathBuf, PackageInfo)>,
}

impl PackageMap {
    /// Create a new empty PackageMap
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a package to the map
    ///
    /// The `package_dir` is the directory containing the package (used for file matching).
    /// The `info.source_root` is where source files are located (used for module path derivation).
    pub fn add(&mut self, package_dir: PathBuf, info: PackageInfo) {
        self.packages.push((package_dir, info));
        // Sort by path component count (descending) for longest-prefix matching
        self.packages
            .sort_by(|a, b| b.0.components().count().cmp(&a.0.components().count()));
    }

    /// Find the package containing the given file path
    ///
    /// Returns the package with the longest matching package directory prefix.
    pub fn find_package_for_file(&self, file_path: &Path) -> Option<&PackageInfo> {
        self.packages
            .iter()
            .find(|(pkg_dir, _)| file_path.starts_with(pkg_dir))
            .map(|(_, info)| info)
    }

    /// Check if the map is empty
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    /// Get the number of packages
    pub fn len(&self) -> usize {
        self.packages.len()
    }

    /// Check if a package exists at exactly the given directory
    ///
    /// This differs from `find_package_for_file` which uses prefix matching.
    /// This checks for exact directory match.
    pub fn has_package_at(&self, dir: &Path) -> bool {
        self.packages.iter().any(|(pkg_dir, _)| pkg_dir == dir)
    }
}

/// Detect and parse project manifest in repository root
///
/// Tries to detect the project type by looking for manifest files in order:
/// 1. Cargo.toml (Rust)
/// 2. pyproject.toml (Python)
/// 3. package.json (Node.js)
pub fn detect_manifest(repo_root: &Path) -> Result<Option<ProjectManifest>> {
    // Try Rust first
    if let Some(manifest) = try_parse_cargo(repo_root)? {
        debug!("Detected Rust project: {:?}", manifest.project_type);
        return Ok(Some(manifest));
    }

    // Try Python
    if let Some(manifest) = try_parse_pyproject(repo_root)? {
        debug!("Detected Python project");
        return Ok(Some(manifest));
    }

    // Try Node.js
    if let Some(manifest) = try_parse_package_json(repo_root)? {
        debug!("Detected Node.js project");
        return Ok(Some(manifest));
    }

    Ok(None)
}

/// Parse Cargo.toml for Rust projects
fn try_parse_cargo(repo_root: &Path) -> Result<Option<ProjectManifest>> {
    let cargo_path = repo_root.join("Cargo.toml");
    if !cargo_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&cargo_path).map_err(|e| {
        Error::config(format!(
            "Failed to read Cargo.toml at {}: {e}",
            cargo_path.display()
        ))
    })?;

    let cargo: toml::Value = content.parse().map_err(|e| {
        Error::config(format!(
            "Failed to parse Cargo.toml at {}: {e}",
            cargo_path.display()
        ))
    })?;

    let mut packages = PackageMap::new();

    // Check for workspace
    if let Some(workspace) = cargo.get("workspace") {
        // Workspace: expand member patterns and parse each member's Cargo.toml
        if let Some(members) = workspace.get("members").and_then(|m| m.as_array()) {
            let member_patterns: Vec<String> = members
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();

            for pattern in member_patterns {
                let full_pattern = repo_root.join(&pattern);
                let pattern_str = full_pattern.to_string_lossy();

                // Expand glob pattern
                let glob_results = glob::glob(&pattern_str)
                    .map_err(|e| Error::config(format!("Invalid glob pattern '{pattern}': {e}")))?;

                for entry in glob_results {
                    let entry = match entry {
                        Ok(path) => path,
                        Err(e) => {
                            debug!("Glob error while scanning workspace members: {e}");
                            continue;
                        }
                    };
                    if let Some(pkg_info) = parse_member_cargo_toml(&entry)? {
                        // Use crate directory as map key (for file matching)
                        // pkg_info.source_root contains the actual source root
                        packages.add(entry, pkg_info);
                    }
                }
            }
        }

        // Also check if the workspace root has a [package] section
        if let Some(pkg_info) = parse_package_section(&cargo, repo_root)? {
            packages.add(repo_root.to_path_buf(), pkg_info);
        }

        return Ok(Some(ProjectManifest {
            project_type: ProjectType::RustWorkspace,
            packages,
        }));
    }

    // Single crate: extract package name
    if let Some(pkg_info) = parse_package_section(&cargo, repo_root)? {
        // Use crate directory as map key (for file matching)
        // pkg_info.source_root contains the actual source root
        packages.add(repo_root.to_path_buf(), pkg_info);

        return Ok(Some(ProjectManifest {
            project_type: ProjectType::RustCrate,
            packages,
        }));
    }

    Ok(None)
}

/// Parse a member crate's Cargo.toml
fn parse_member_cargo_toml(member_path: &Path) -> Result<Option<PackageInfo>> {
    let cargo_path = member_path.join("Cargo.toml");
    if !cargo_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&cargo_path).map_err(|e| {
        Error::config(format!(
            "Failed to read member Cargo.toml at {}: {e}",
            cargo_path.display()
        ))
    })?;

    let cargo: toml::Value = content.parse().map_err(|e| {
        Error::config(format!(
            "Failed to parse member Cargo.toml at {}: {e}",
            cargo_path.display()
        ))
    })?;

    parse_package_section(&cargo, member_path)
}

/// Parse the [package] section from a Cargo.toml
fn parse_package_section(cargo: &toml::Value, crate_root: &Path) -> Result<Option<PackageInfo>> {
    let package = match cargo.get("package") {
        Some(p) => p,
        None => return Ok(None),
    };

    let name = match package.get("name").and_then(|n| n.as_str()) {
        Some(n) => n.to_string(),
        None => return Ok(None),
    };

    // Normalize the crate name (replace hyphens with underscores)
    // This matches how Rust normalizes crate names in the module system
    let normalized_name = name.replace('-', "_");

    Ok(Some(PackageInfo {
        name: normalized_name,
        source_root: crate_root.join("src"),
    }))
}

/// Parse pyproject.toml for Python projects
fn try_parse_pyproject(repo_root: &Path) -> Result<Option<ProjectManifest>> {
    let pyproject_path = repo_root.join("pyproject.toml");
    if !pyproject_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&pyproject_path).map_err(|e| {
        Error::config(format!(
            "Failed to read pyproject.toml at {}: {e}",
            pyproject_path.display()
        ))
    })?;

    let pyproject: toml::Value = content.parse().map_err(|e| {
        Error::config(format!(
            "Failed to parse pyproject.toml at {}: {e}",
            pyproject_path.display()
        ))
    })?;

    // Try [project] section first (PEP 621)
    let name = pyproject
        .get("project")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        // Fall back to [tool.poetry] section
        .or_else(|| {
            pyproject
                .get("tool")
                .and_then(|t| t.get("poetry"))
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
        });

    let name = match name {
        Some(n) => n.to_string(),
        None => return Ok(None),
    };

    let mut packages = PackageMap::new();

    // Common Python source roots to check
    let source_roots = ["src", &name, "."];
    let mut found_source_root = None;
    for root in source_roots {
        let source_root = repo_root.join(root);
        if source_root.exists() && source_root.is_dir() {
            found_source_root = Some(source_root);
            break;
        }
    }

    // Use repo_root as map key (for file matching)
    // source_root is stored in PackageInfo for module path derivation
    let source_root = found_source_root.unwrap_or_else(|| repo_root.to_path_buf());
    packages.add(
        repo_root.to_path_buf(),
        PackageInfo {
            name: name.clone(),
            source_root,
        },
    );

    Ok(Some(ProjectManifest {
        project_type: ProjectType::PythonPackage,
        packages,
    }))
}

/// Parse package.json for Node.js projects
///
/// Supports:
/// - npm workspaces: `"workspaces": ["packages/*", "apps/*"]`
/// - yarn workspaces (array): `"workspaces": ["packages/*"]`
/// - yarn workspaces (object): `"workspaces": { "packages": ["packages/*"] }`
/// - pnpm workspaces: `pnpm-workspace.yaml` file
/// - lerna.json: `"packages": ["packages/*"]`
/// - All subdirectories with package.json containing a "name" field
fn try_parse_package_json(repo_root: &Path) -> Result<Option<ProjectManifest>> {
    let package_json_path = repo_root.join("package.json");
    if !package_json_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&package_json_path).map_err(|e| {
        Error::config(format!(
            "Failed to read package.json at {}: {e}",
            package_json_path.display()
        ))
    })?;

    let package: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        Error::config(format!(
            "Failed to parse package.json at {}: {e}",
            package_json_path.display()
        ))
    })?;

    let root_name = package
        .get("name")
        .and_then(|n| n.as_str())
        .map(String::from);

    let mut packages = PackageMap::new();
    let mut is_workspace = false;

    // Check for workspace patterns (npm/yarn/pnpm/lerna)
    let workspace_patterns = get_workspace_patterns(&package, repo_root)?;

    if !workspace_patterns.is_empty() {
        is_workspace = true;

        for pattern in workspace_patterns {
            let full_pattern = repo_root.join(&pattern);
            let pattern_str = full_pattern.to_string_lossy();

            // Expand glob pattern
            let glob_results = glob::glob(&pattern_str)
                .map_err(|e| Error::config(format!("Invalid glob pattern '{pattern}': {e}")))?;

            for entry in glob_results {
                let entry = match entry {
                    Ok(path) => path,
                    Err(e) => {
                        debug!("Glob error while scanning workspace packages: {e}");
                        continue;
                    }
                };
                if entry.is_dir() {
                    if let Some(pkg_info) = parse_member_package_json(&entry)? {
                        // Use package directory as map key (for file matching)
                        // pkg_info.source_root contains the actual source root
                        packages.add(entry, pkg_info);
                    }
                }
            }
        }
    }

    // Also scan subdirectories for package.json files not declared in workspaces
    scan_for_all_packages(repo_root, &mut packages)?;

    // Also add the root package if it has a name
    if let Some(name) = root_name {
        let source_root = determine_node_source_root(repo_root);
        packages.add(repo_root.to_path_buf(), PackageInfo { name, source_root });
    }

    if packages.is_empty() {
        return Ok(None);
    }

    let project_type = if is_workspace || packages.len() > 1 {
        ProjectType::NodeWorkspace
    } else {
        ProjectType::NodePackage
    };

    Ok(Some(ProjectManifest {
        project_type,
        packages,
    }))
}

/// Scan all subdirectories for package.json files with a "name" field
///
/// This catches packages that aren't declared in workspace configurations,
/// like companion apps (website, docs) or examples.
fn scan_for_all_packages(repo_root: &Path, packages: &mut PackageMap) -> Result<()> {
    // Directories to skip
    const SKIP_DIRS: &[&str] = &[
        "node_modules",
        ".git",
        "dist",
        "build",
        "target",
        ".next",
        ".nuxt",
        "coverage",
        "__pycache__",
    ];

    fn scan_recursive(dir: &Path, packages: &mut PackageMap) -> Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                debug!("Skipping unreadable directory {}: {e}", dir.display());
                return Ok(());
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            // Skip excluded directories
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if SKIP_DIRS.contains(&name) || name.starts_with('.') {
                    continue;
                }
            }

            // Check if this directory has a package.json with a name
            let package_json = path.join("package.json");
            if package_json.exists() {
                match parse_member_package_json(&path) {
                    Ok(Some(pkg_info)) => {
                        // Don't add if we already have this exact directory as a package
                        // (Note: we use exact path match, not prefix match, so child dirs can be packages)
                        if !packages.has_package_at(&path) {
                            packages.add(path.clone(), pkg_info);
                        }
                    }
                    Ok(None) => {
                        // No name field - expected for packages without explicit names
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse package.json in {}: {e}", path.display());
                    }
                }
            }

            // Continue scanning subdirectories
            scan_recursive(&path, packages)?;
        }

        Ok(())
    }

    scan_recursive(repo_root, packages)
}

/// Get workspace patterns from package.json or pnpm-workspace.yaml
fn get_workspace_patterns(package: &serde_json::Value, repo_root: &Path) -> Result<Vec<String>> {
    // Check package.json "workspaces" field
    if let Some(workspaces) = package.get("workspaces") {
        // npm/yarn array format: "workspaces": ["packages/*", "apps/*"]
        if let Some(arr) = workspaces.as_array() {
            return Ok(arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect());
        }

        // yarn object format: "workspaces": { "packages": ["packages/*"] }
        if let Some(obj) = workspaces.as_object() {
            if let Some(packages) = obj.get("packages").and_then(|p| p.as_array()) {
                return Ok(packages
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect());
            }
        }
    }

    // Check for pnpm-workspace.yaml
    let pnpm_workspace_path = repo_root.join("pnpm-workspace.yaml");
    if pnpm_workspace_path.exists() {
        let patterns = parse_pnpm_workspace_yaml(&pnpm_workspace_path)?;
        if !patterns.is_empty() {
            return Ok(patterns);
        }
    }

    // Check for lerna.json
    let lerna_path = repo_root.join("lerna.json");
    if lerna_path.exists() {
        let patterns = parse_lerna_json(&lerna_path)?;
        if !patterns.is_empty() {
            return Ok(patterns);
        }
    }

    // Check for tsconfig.json with project references
    let tsconfig_path = repo_root.join("tsconfig.json");
    if tsconfig_path.exists() {
        let patterns = parse_tsconfig_references(&tsconfig_path)?;
        if !patterns.is_empty() {
            return Ok(patterns);
        }
    }

    Ok(Vec::new())
}

/// Parse tsconfig.json for project references
///
/// TypeScript project references define package boundaries via the `references` field.
/// Each reference points to a directory containing another tsconfig.json.
fn parse_tsconfig_references(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        Error::config(format!(
            "Failed to read tsconfig.json at {}: {e}",
            path.display()
        ))
    })?;

    // tsconfig.json may have comments, so we need to strip them
    let content = strip_json_comments(&content);

    let tsconfig: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        Error::config(format!(
            "Failed to parse tsconfig.json at {}: {e}",
            path.display()
        ))
    })?;

    // tsconfig.json format: { "references": [{ "path": "./packages/core" }, ...] }
    if let Some(references) = tsconfig.get("references").and_then(|r| r.as_array()) {
        let patterns: Vec<String> = references
            .iter()
            .filter_map(|r| r.get("path").and_then(|p| p.as_str()))
            .map(|p| {
                // Normalize path (remove leading ./ and trailing /)
                let p = p.trim_start_matches("./").trim_end_matches('/');
                p.to_string()
            })
            .collect();

        if !patterns.is_empty() {
            return Ok(patterns);
        }
    }

    Ok(Vec::new())
}

/// Strip JSON comments (single-line // and multi-line /* */)
///
/// tsconfig.json and other JSON5-like files often contain comments
fn strip_json_comments(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    let mut in_string = false;
    let mut escape_next = false;

    while let Some(c) = chars.next() {
        if escape_next {
            result.push(c);
            escape_next = false;
            continue;
        }

        if c == '\\' && in_string {
            result.push(c);
            escape_next = true;
            continue;
        }

        if c == '"' {
            in_string = !in_string;
            result.push(c);
            continue;
        }

        if in_string {
            result.push(c);
            continue;
        }

        // Check for comments
        if c == '/' {
            if let Some(&next) = chars.peek() {
                if next == '/' {
                    // Single-line comment - skip to end of line
                    chars.next(); // consume the second /
                    while let Some(&nc) = chars.peek() {
                        if nc == '\n' {
                            break;
                        }
                        chars.next();
                    }
                    continue;
                } else if next == '*' {
                    // Multi-line comment - skip to */
                    chars.next(); // consume the *
                    while let Some(nc) = chars.next() {
                        if nc == '*' {
                            if let Some(&'/') = chars.peek() {
                                chars.next(); // consume the /
                                break;
                            }
                        }
                    }
                    continue;
                }
            }
        }

        result.push(c);
    }

    result
}

/// Parse lerna.json file for workspace packages
fn parse_lerna_json(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        Error::config(format!(
            "Failed to read lerna.json at {}: {e}",
            path.display()
        ))
    })?;

    let lerna: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        Error::config(format!(
            "Failed to parse lerna.json at {}: {e}",
            path.display()
        ))
    })?;

    // lerna.json format: { "packages": ["packages/*", "libs/*"] }
    if let Some(packages) = lerna.get("packages").and_then(|p| p.as_array()) {
        return Ok(packages
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect());
    }

    Ok(Vec::new())
}

/// Parse pnpm-workspace.yaml file
fn parse_pnpm_workspace_yaml(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        Error::config(format!(
            "Failed to read pnpm-workspace.yaml at {}: {e}",
            path.display()
        ))
    })?;

    // Simple YAML parsing for pnpm-workspace.yaml format:
    // packages:
    //   - 'packages/*'
    //   - 'apps/*'
    let mut patterns = Vec::new();
    let mut in_packages = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "packages:" {
            in_packages = true;
            continue;
        }

        if in_packages {
            // Check if we've hit another top-level key
            if !trimmed.is_empty() && !trimmed.starts_with('-') && !trimmed.starts_with('#') {
                break;
            }

            // Parse array item: - 'packages/*' or - "packages/*" or - packages/*
            if let Some(item) = trimmed.strip_prefix('-') {
                let pattern = item.trim().trim_matches('\'').trim_matches('"').to_string();
                if !pattern.is_empty() {
                    patterns.push(pattern);
                }
            }
        }
    }

    Ok(patterns)
}

/// Parse a member package's package.json
fn parse_member_package_json(member_path: &Path) -> Result<Option<PackageInfo>> {
    let package_json_path = member_path.join("package.json");
    if !package_json_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&package_json_path).map_err(|e| {
        Error::config(format!(
            "Failed to read member package.json at {}: {e}",
            package_json_path.display()
        ))
    })?;

    let package: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        Error::config(format!(
            "Failed to parse member package.json at {}: {e}",
            package_json_path.display()
        ))
    })?;

    let name = match package.get("name").and_then(|n| n.as_str()) {
        Some(n) => n.to_string(),
        None => return Ok(None),
    };

    let source_root = determine_node_source_root(member_path);

    Ok(Some(PackageInfo { name, source_root }))
}

/// Determine the source root for a Node.js package
///
/// Checks in order: src/, lib/, then falls back to package root
fn determine_node_source_root(package_root: &Path) -> PathBuf {
    let source_dirs = ["src", "lib"];
    for dir in source_dirs {
        let source_root = package_root.join(dir);
        if source_root.exists() && source_root.is_dir() {
            return source_root;
        }
    }
    package_root.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_package_map_longest_prefix_match() {
        let mut map = PackageMap::new();

        // Add packages with different depths
        map.add(
            PathBuf::from("/repo/src"),
            PackageInfo {
                name: "root".to_string(),
                source_root: PathBuf::from("/repo/src"),
            },
        );
        map.add(
            PathBuf::from("/repo/crates/core/src"),
            PackageInfo {
                name: "core".to_string(),
                source_root: PathBuf::from("/repo/crates/core/src"),
            },
        );

        // File in nested package should match the nested package
        let file = PathBuf::from("/repo/crates/core/src/lib.rs");
        let pkg = map.find_package_for_file(&file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("core"));

        // File in root should match root package
        let file = PathBuf::from("/repo/src/main.rs");
        let pkg = map.find_package_for_file(&file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("root"));

        // File outside all packages should return None
        let file = PathBuf::from("/other/file.rs");
        assert!(map.find_package_for_file(&file).is_none());
    }

    #[test]
    fn test_parse_single_rust_crate() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create Cargo.toml
        fs::write(
            root.join("Cargo.toml"),
            r#"
[package]
name = "my-crate"
version = "0.1.0"
"#,
        )
        .unwrap();

        // Create src directory
        fs::create_dir(root.join("src")).unwrap();

        let manifest = detect_manifest(root).unwrap();
        assert!(manifest.is_some());
        let manifest = manifest.unwrap();

        assert_eq!(manifest.project_type, ProjectType::RustCrate);
        assert_eq!(manifest.packages.len(), 1);

        let file = root.join("src/lib.rs");
        let pkg = manifest.packages.find_package_for_file(&file);
        assert!(pkg.is_some());
        // Hyphens should be normalized to underscores
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("my_crate"));
    }

    #[test]
    fn test_parse_rust_workspace() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create workspace Cargo.toml
        fs::write(
            root.join("Cargo.toml"),
            r#"
[workspace]
members = ["crates/*"]
"#,
        )
        .unwrap();

        // Create member crates
        fs::create_dir_all(root.join("crates/core/src")).unwrap();
        fs::write(
            root.join("crates/core/Cargo.toml"),
            r#"
[package]
name = "my-core"
version = "0.1.0"
"#,
        )
        .unwrap();

        fs::create_dir_all(root.join("crates/cli/src")).unwrap();
        fs::write(
            root.join("crates/cli/Cargo.toml"),
            r#"
[package]
name = "my-cli"
version = "0.1.0"
"#,
        )
        .unwrap();

        let manifest = detect_manifest(root).unwrap();
        assert!(manifest.is_some());
        let manifest = manifest.unwrap();

        assert_eq!(manifest.project_type, ProjectType::RustWorkspace);
        assert_eq!(manifest.packages.len(), 2);

        // Check core package
        let core_file = root.join("crates/core/src/lib.rs");
        let pkg = manifest.packages.find_package_for_file(&core_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("my_core"));

        // Check cli package
        let cli_file = root.join("crates/cli/src/main.rs");
        let pkg = manifest.packages.find_package_for_file(&cli_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("my_cli"));
    }

    #[test]
    fn test_parse_python_pyproject() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create pyproject.toml with PEP 621 format
        fs::write(
            root.join("pyproject.toml"),
            r#"
[project]
name = "my_package"
version = "0.1.0"
"#,
        )
        .unwrap();

        // Create src directory
        fs::create_dir(root.join("src")).unwrap();

        let manifest = detect_manifest(root).unwrap();
        assert!(manifest.is_some());
        let manifest = manifest.unwrap();

        assert_eq!(manifest.project_type, ProjectType::PythonPackage);
        assert_eq!(manifest.packages.len(), 1);

        let file = root.join("src/module.py");
        let pkg = manifest.packages.find_package_for_file(&file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("my_package"));
    }

    #[test]
    fn test_parse_node_package() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create package.json
        fs::write(
            root.join("package.json"),
            r#"
{
  "name": "my-node-package",
  "version": "1.0.0"
}
"#,
        )
        .unwrap();

        // Create src directory
        fs::create_dir(root.join("src")).unwrap();

        let manifest = detect_manifest(root).unwrap();
        assert!(manifest.is_some());
        let manifest = manifest.unwrap();

        assert_eq!(manifest.project_type, ProjectType::NodePackage);
        assert_eq!(manifest.packages.len(), 1);

        let file = root.join("src/index.ts");
        let pkg = manifest.packages.find_package_for_file(&file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("my-node-package"));
    }

    #[test]
    fn test_no_manifest() {
        let temp = TempDir::new().unwrap();
        let manifest = detect_manifest(temp.path()).unwrap();
        assert!(manifest.is_none());
    }

    #[test]
    fn test_parse_npm_workspace() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create root package.json with workspaces
        fs::write(
            root.join("package.json"),
            r#"
{
  "name": "my-monorepo",
  "private": true,
  "workspaces": ["packages/*"]
}
"#,
        )
        .unwrap();

        // Create member packages
        fs::create_dir_all(root.join("packages/core/src")).unwrap();
        fs::write(
            root.join("packages/core/package.json"),
            r#"
{
  "name": "@my-monorepo/core",
  "version": "1.0.0"
}
"#,
        )
        .unwrap();

        fs::create_dir_all(root.join("packages/utils/src")).unwrap();
        fs::write(
            root.join("packages/utils/package.json"),
            r#"
{
  "name": "@my-monorepo/utils",
  "version": "1.0.0"
}
"#,
        )
        .unwrap();

        let manifest = detect_manifest(root).unwrap();
        assert!(manifest.is_some());
        let manifest = manifest.unwrap();

        assert_eq!(manifest.project_type, ProjectType::NodeWorkspace);
        // 2 member packages + 1 root package
        assert_eq!(manifest.packages.len(), 3);

        // Check core package
        let core_file = root.join("packages/core/src/index.ts");
        let pkg = manifest.packages.find_package_for_file(&core_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("@my-monorepo/core"));

        // Check utils package
        let utils_file = root.join("packages/utils/src/helpers.ts");
        let pkg = manifest.packages.find_package_for_file(&utils_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("@my-monorepo/utils"));
    }

    #[test]
    fn test_parse_pnpm_workspace() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create root package.json (no workspaces field)
        fs::write(
            root.join("package.json"),
            r#"
{
  "name": "pnpm-monorepo",
  "private": true
}
"#,
        )
        .unwrap();

        // Create pnpm-workspace.yaml
        fs::write(
            root.join("pnpm-workspace.yaml"),
            r#"
packages:
  - 'apps/*'
  - 'packages/*'
"#,
        )
        .unwrap();

        // Create member packages
        fs::create_dir_all(root.join("apps/web/src")).unwrap();
        fs::write(
            root.join("apps/web/package.json"),
            r#"
{
  "name": "@monorepo/web",
  "version": "1.0.0"
}
"#,
        )
        .unwrap();

        fs::create_dir_all(root.join("packages/shared")).unwrap();
        fs::write(
            root.join("packages/shared/package.json"),
            r#"
{
  "name": "@monorepo/shared",
  "version": "1.0.0"
}
"#,
        )
        .unwrap();

        let manifest = detect_manifest(root).unwrap();
        assert!(manifest.is_some());
        let manifest = manifest.unwrap();

        assert_eq!(manifest.project_type, ProjectType::NodeWorkspace);
        // 2 member packages + 1 root package
        assert_eq!(manifest.packages.len(), 3);

        // Check web app package (has src/)
        let web_file = root.join("apps/web/src/index.tsx");
        let pkg = manifest.packages.find_package_for_file(&web_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("@monorepo/web"));

        // Check shared package (no src/, uses package root)
        let shared_file = root.join("packages/shared/index.ts");
        let pkg = manifest.packages.find_package_for_file(&shared_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("@monorepo/shared"));
    }

    #[test]
    fn test_parse_yarn_workspace_object_format() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create root package.json with yarn workspaces object format
        fs::write(
            root.join("package.json"),
            r#"
{
  "name": "yarn-monorepo",
  "private": true,
  "workspaces": {
    "packages": ["packages/*"],
    "nohoist": ["**/react-native"]
  }
}
"#,
        )
        .unwrap();

        // Create member package
        fs::create_dir_all(root.join("packages/lib/src")).unwrap();
        fs::write(
            root.join("packages/lib/package.json"),
            r#"
{
  "name": "@yarn/lib",
  "version": "1.0.0"
}
"#,
        )
        .unwrap();

        let manifest = detect_manifest(root).unwrap();
        assert!(manifest.is_some());
        let manifest = manifest.unwrap();

        assert_eq!(manifest.project_type, ProjectType::NodeWorkspace);

        // Check lib package
        let lib_file = root.join("packages/lib/src/index.ts");
        let pkg = manifest.packages.find_package_for_file(&lib_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("@yarn/lib"));
    }

    #[test]
    fn test_npm_workspace_longest_prefix_match() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create root package.json with multiple workspace patterns
        fs::write(
            root.join("package.json"),
            r#"
{
  "name": "nested-monorepo",
  "workspaces": ["packages/*", "packages/*/nested"]
}
"#,
        )
        .unwrap();

        // Create outer package
        fs::create_dir_all(root.join("packages/outer/src")).unwrap();
        fs::write(
            root.join("packages/outer/package.json"),
            r#"{"name": "outer"}"#,
        )
        .unwrap();

        // Create nested package within outer
        fs::create_dir_all(root.join("packages/outer/nested/src")).unwrap();
        fs::write(
            root.join("packages/outer/nested/package.json"),
            r#"{"name": "nested"}"#,
        )
        .unwrap();

        let manifest = detect_manifest(root).unwrap();
        assert!(manifest.is_some());
        let manifest = manifest.unwrap();

        // File in nested package should match nested (longest prefix)
        let nested_file = root.join("packages/outer/nested/src/index.ts");
        let pkg = manifest.packages.find_package_for_file(&nested_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("nested"));

        // File in outer package should match outer
        let outer_file = root.join("packages/outer/src/index.ts");
        let pkg = manifest.packages.find_package_for_file(&outer_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("outer"));
    }

    #[test]
    fn test_parse_lerna_json() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create root package.json (no workspaces)
        fs::write(
            root.join("package.json"),
            r#"
{
  "name": "lerna-monorepo",
  "private": true
}
"#,
        )
        .unwrap();

        // Create lerna.json
        fs::write(
            root.join("lerna.json"),
            r#"
{
  "version": "independent",
  "packages": ["packages/*"]
}
"#,
        )
        .unwrap();

        // Create member package
        fs::create_dir_all(root.join("packages/core/src")).unwrap();
        fs::write(
            root.join("packages/core/package.json"),
            r#"{"name": "@lerna/core", "version": "1.0.0"}"#,
        )
        .unwrap();

        let manifest = detect_manifest(root).unwrap();
        assert!(manifest.is_some());
        let manifest = manifest.unwrap();

        assert_eq!(manifest.project_type, ProjectType::NodeWorkspace);

        // Check core package
        let core_file = root.join("packages/core/src/index.ts");
        let pkg = manifest.packages.find_package_for_file(&core_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("@lerna/core"));
    }

    #[test]
    fn test_detect_packages_outside_workspace() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create root package.json with pnpm workspace containing only root
        fs::write(
            root.join("package.json"),
            r#"
{
  "name": "jotai",
  "private": true
}
"#,
        )
        .unwrap();

        // Create pnpm-workspace.yaml with only root (like jotai)
        fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - .\n").unwrap();

        // Create website package (NOT in workspace, but has package.json)
        fs::create_dir_all(root.join("website/src")).unwrap();
        fs::write(
            root.join("website/package.json"),
            r#"{"name": "jotai-website", "version": "0.0.0"}"#,
        )
        .unwrap();

        // Create examples package
        fs::create_dir_all(root.join("examples/todos/src")).unwrap();
        fs::write(
            root.join("examples/todos/package.json"),
            r#"{"name": "example-todos", "version": "0.0.0"}"#,
        )
        .unwrap();

        let manifest = detect_manifest(root).unwrap();
        assert!(manifest.is_some());
        let manifest = manifest.unwrap();

        assert_eq!(manifest.project_type, ProjectType::NodeWorkspace);

        // Check root package
        let root_file = root.join("src/index.ts");
        let pkg = manifest.packages.find_package_for_file(&root_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("jotai"));

        // Check website package (detected via scan_for_all_packages)
        let website_file = root.join("website/src/pages/index.tsx");
        let pkg = manifest.packages.find_package_for_file(&website_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("jotai-website"));

        // Check examples package (detected via scan_for_all_packages)
        let example_file = root.join("examples/todos/src/App.tsx");
        let pkg = manifest.packages.find_package_for_file(&example_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("example-todos"));
    }

    #[test]
    fn test_parse_tsconfig_references() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create root package.json
        fs::write(
            root.join("package.json"),
            r#"
{
  "name": "ts-monorepo",
  "private": true
}
"#,
        )
        .unwrap();

        // Create tsconfig.json with project references
        fs::write(
            root.join("tsconfig.json"),
            r#"
{
  // This is a comment that should be stripped
  "compilerOptions": {
    "composite": true
  },
  /* Multi-line
     comment */
  "references": [
    { "path": "./packages/core" },
    { "path": "./packages/utils" }
  ]
}
"#,
        )
        .unwrap();

        // Create core package
        fs::create_dir_all(root.join("packages/core/src")).unwrap();
        fs::write(
            root.join("packages/core/package.json"),
            r#"{"name": "@ts/core", "version": "1.0.0"}"#,
        )
        .unwrap();
        fs::write(
            root.join("packages/core/tsconfig.json"),
            r#"{"compilerOptions": {"composite": true}}"#,
        )
        .unwrap();

        // Create utils package
        fs::create_dir_all(root.join("packages/utils/src")).unwrap();
        fs::write(
            root.join("packages/utils/package.json"),
            r#"{"name": "@ts/utils", "version": "1.0.0"}"#,
        )
        .unwrap();

        let manifest = detect_manifest(root).unwrap();
        assert!(manifest.is_some());
        let manifest = manifest.unwrap();

        assert_eq!(manifest.project_type, ProjectType::NodeWorkspace);

        // Check core package
        let core_file = root.join("packages/core/src/index.ts");
        let pkg = manifest.packages.find_package_for_file(&core_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("@ts/core"));

        // Check utils package
        let utils_file = root.join("packages/utils/src/helpers.ts");
        let pkg = manifest.packages.find_package_for_file(&utils_file);
        assert!(pkg.is_some());
        assert_eq!(pkg.map(|p| p.name.as_str()), Some("@ts/utils"));
    }

    #[test]
    fn test_strip_json_comments() {
        let input = r#"
{
  // Single line comment
  "key": "value", // trailing comment
  /* Multi-line
     comment */
  "url": "https://example.com/path",
  "nested": {
    "inner": "value"
  }
}
"#;
        let stripped = strip_json_comments(input);
        let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(parsed["key"], "value");
        assert_eq!(parsed["url"], "https://example.com/path");
        assert_eq!(parsed["nested"]["inner"], "value");
    }
}
