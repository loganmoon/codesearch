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
    pub fn add(&mut self, source_root: PathBuf, info: PackageInfo) {
        self.packages.push((source_root, info));
        // Sort by path component count (descending) for longest-prefix matching
        self.packages
            .sort_by(|a, b| b.0.components().count().cmp(&a.0.components().count()));
    }

    /// Find the package containing the given file path
    ///
    /// Returns the package with the longest matching source root prefix.
    pub fn find_package_for_file(&self, file_path: &Path) -> Option<&PackageInfo> {
        self.packages
            .iter()
            .find(|(root, _)| file_path.starts_with(root))
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

                for entry in glob_results.flatten() {
                    if let Some(pkg_info) = parse_member_cargo_toml(&entry)? {
                        let source_root = entry.join("src");
                        if source_root.exists() {
                            packages.add(source_root, pkg_info);
                        } else {
                            // Some crates might not have src/ (e.g., proc-macro crates)
                            packages.add(entry.clone(), pkg_info);
                        }
                    }
                }
            }
        }

        // Also check if the workspace root has a [package] section
        if let Some(pkg_info) = parse_package_section(&cargo, repo_root)? {
            let source_root = repo_root.join("src");
            if source_root.exists() {
                packages.add(source_root, pkg_info);
            }
        }

        return Ok(Some(ProjectManifest {
            project_type: ProjectType::RustWorkspace,
            packages,
        }));
    }

    // Single crate: extract package name
    if let Some(pkg_info) = parse_package_section(&cargo, repo_root)? {
        let source_root = repo_root.join("src");
        if source_root.exists() {
            packages.add(source_root, pkg_info);
        } else {
            packages.add(repo_root.to_path_buf(), pkg_info);
        }

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
    for root in source_roots {
        let source_root = repo_root.join(root);
        if source_root.exists() && source_root.is_dir() {
            packages.add(
                source_root.clone(),
                PackageInfo {
                    name: name.clone(),
                    source_root,
                },
            );
            break;
        }
    }

    if packages.is_empty() {
        // Default to repo root if no specific source directory found
        packages.add(
            repo_root.to_path_buf(),
            PackageInfo {
                name: name.clone(),
                source_root: repo_root.to_path_buf(),
            },
        );
    }

    Ok(Some(ProjectManifest {
        project_type: ProjectType::PythonPackage,
        packages,
    }))
}

/// Parse package.json for Node.js projects
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

    let name = match package.get("name").and_then(|n| n.as_str()) {
        Some(n) => n.to_string(),
        None => return Ok(None),
    };

    let mut packages = PackageMap::new();

    // Common JS/TS source roots to check
    let source_roots = ["src", "lib", "."];
    for root in source_roots {
        let source_root = repo_root.join(root);
        if source_root.exists() && source_root.is_dir() {
            packages.add(
                source_root.clone(),
                PackageInfo {
                    name: name.clone(),
                    source_root,
                },
            );
            break;
        }
    }

    if packages.is_empty() {
        packages.add(
            repo_root.to_path_buf(),
            PackageInfo {
                name: name.clone(),
                source_root: repo_root.to_path_buf(),
            },
        );
    }

    Ok(Some(ProjectManifest {
        project_type: ProjectType::NodePackage,
        packages,
    }))
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
}
