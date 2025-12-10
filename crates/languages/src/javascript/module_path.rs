//! Module path derivation for JavaScript/TypeScript files
//!
//! Derives the JS/TS module path from a file path relative to the source root.

use std::path::Path;

/// Derive JavaScript/TypeScript module path from file path relative to source root
///
/// Converts a file path to its corresponding module path based on
/// common JS/TS module conventions.
///
/// # Returns
/// - `None` for module root `index.js`/`index.ts` files directly in the source root
/// - `Some(module_path)` for other files, with `.` as separator
///
/// # Examples
/// - `index.ts` -> `None` (module root)
/// - `index.js` -> `None` (module root)
/// - `utils.ts` -> `Some("utils")`
/// - `components/Button.tsx` -> `Some("components.Button")`
/// - `components/index.tsx` -> `Some("components")`
pub fn derive_module_path(file_path: &Path, source_root: &Path) -> Option<String> {
    let relative = file_path.strip_prefix(source_root).ok()?;

    // Get directory components
    let parent = relative.parent()?;
    let mut components: Vec<&str> = parent
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        // Filter out common non-module directories
        .filter(|c| !["node_modules", "dist", "build", "__tests__", "__mocks__"].contains(c))
        .collect();

    // Get the filename without extension
    let filename = file_path.file_stem()?.to_str()?;

    // index.js/index.ts marks a directory module, not a separate module
    if filename != "index" {
        components.push(filename);
    }

    if components.is_empty() {
        None
    } else {
        Some(components.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_root_index_ts() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/index.ts");
        assert_eq!(derive_module_path(&file, &root), None);
    }

    #[test]
    fn test_root_index_js() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/index.js");
        assert_eq!(derive_module_path(&file, &root), None);
    }

    #[test]
    fn test_simple_module_ts() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/utils.ts");
        assert_eq!(derive_module_path(&file, &root), Some("utils".to_string()));
    }

    #[test]
    fn test_simple_module_js() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/helpers.js");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("helpers".to_string())
        );
    }

    #[test]
    fn test_directory_index() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/components/index.tsx");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("components".to_string())
        );
    }

    #[test]
    fn test_nested_component() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/components/Button.tsx");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("components.Button".to_string())
        );
    }

    #[test]
    fn test_deeply_nested_module() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/features/auth/Login.tsx");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("features.auth.Login".to_string())
        );
    }

    #[test]
    fn test_file_outside_source_root() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/other/file.ts");
        assert_eq!(derive_module_path(&file, &root), None);
    }

    #[test]
    fn test_jsx_file() {
        let root = PathBuf::from("/project/src");
        let file = PathBuf::from("/project/src/App.jsx");
        assert_eq!(derive_module_path(&file, &root), Some("App".to_string()));
    }

    #[test]
    fn test_node_modules_filtered() {
        let root = PathBuf::from("/project");
        let file = PathBuf::from("/project/node_modules/lodash/index.js");
        // node_modules is filtered out, leaving empty components
        assert_eq!(derive_module_path(&file, &root), Some("lodash".to_string()));
    }
}
