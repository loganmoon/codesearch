//! Module path derivation for Rust files
//!
//! Derives the Rust module path from a file path relative to the source root.

use std::path::Path;

/// Derive Rust module path from file path relative to source root
///
/// Converts a file path to its corresponding Rust module path based on
/// Rust's module system conventions.
///
/// # Returns
/// - `None` for crate root files (`lib.rs`, `main.rs`) or if the file is not under source root
/// - `Some(module_path)` for other files, with `::` as separator
///
/// # Examples
/// - `src/lib.rs` -> `None` (crate root)
/// - `src/main.rs` -> `None` (crate root)
/// - `src/foo.rs` -> `Some("foo")`
/// - `src/foo/mod.rs` -> `Some("foo")`
/// - `src/foo/bar.rs` -> `Some("foo::bar")`
/// - `src/foo/bar/baz.rs` -> `Some("foo::bar::baz")`
pub fn derive_module_path(file_path: &Path, source_root: &Path) -> Option<String> {
    let relative = file_path.strip_prefix(source_root).ok()?;
    let mut components: Vec<&str> = relative
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    if components.is_empty() {
        return None;
    }

    // Handle special files
    let filename = *components.last()?;
    match filename {
        "lib.rs" | "main.rs" => {
            components.pop();
        }
        "mod.rs" => {
            components.pop();
        }
        _ => {
            // Strip .rs extension from last component
            if let Some(stem) = Path::new(filename).file_stem().and_then(|s| s.to_str()) {
                let last = components.len() - 1;
                components[last] = stem;
            }
        }
    }

    if components.is_empty() {
        None
    } else {
        Some(components.join("::"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_lib_rs_is_crate_root() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/lib.rs");
        assert_eq!(derive_module_path(&file, &root), None);
    }

    #[test]
    fn test_main_rs_is_crate_root() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/main.rs");
        assert_eq!(derive_module_path(&file, &root), None);
    }

    #[test]
    fn test_simple_module() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/network.rs");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("network".to_string())
        );
    }

    #[test]
    fn test_mod_rs() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/network/mod.rs");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("network".to_string())
        );
    }

    #[test]
    fn test_nested_module() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/network/http.rs");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("network::http".to_string())
        );
    }

    #[test]
    fn test_deeply_nested_module() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/network/http/client.rs");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("network::http::client".to_string())
        );
    }

    #[test]
    fn test_nested_mod_rs() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/network/http/mod.rs");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("network::http".to_string())
        );
    }

    #[test]
    fn test_file_outside_source_root() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/other/file.rs");
        assert_eq!(derive_module_path(&file, &root), None);
    }

    #[test]
    fn test_tests_directory() {
        let root = PathBuf::from("/project/tests");
        let file = PathBuf::from("/project/tests/integration.rs");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("integration".to_string())
        );
    }
}
