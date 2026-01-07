//! Module path derivation for JavaScript and TypeScript files
//!
//! Derives the module path from a file path relative to the source root.
//! Each file is treated as its own module in JS/TS.

use std::path::Path;

/// Derive module path from file path relative to source root
///
/// Converts a file path to its corresponding module path using `.` as separator.
///
/// # Returns
/// - `Some(module_path)` with the path components joined by `.`
///
/// # Examples
/// - `index.ts` -> `Some("index")`
/// - `utils/helpers.ts` -> `Some("utils.helpers")`
/// - `src/components/Button.tsx` -> `Some("src.components.Button")`
pub fn derive_module_path(file_path: &Path, source_root: &Path) -> Option<String> {
    let relative = file_path.strip_prefix(source_root).ok()?;

    // Get directory components
    let parent = relative.parent()?;
    let mut components: Vec<&str> = parent
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Get the filename without extension
    let filename = file_path.file_stem()?.to_str()?;

    // For TypeScript .d.ts files, also strip the .d suffix
    let module_name = filename.strip_suffix(".d").unwrap_or(filename);
    components.push(module_name);

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
    fn test_simple_file() {
        let root = PathBuf::from("/project");
        let file = PathBuf::from("/project/index.ts");
        assert_eq!(derive_module_path(&file, &root), Some("index".to_string()));
    }

    #[test]
    fn test_nested_file() {
        let root = PathBuf::from("/project");
        let file = PathBuf::from("/project/utils/helpers.ts");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("utils.helpers".to_string())
        );
    }

    #[test]
    fn test_deeply_nested_file() {
        let root = PathBuf::from("/project");
        let file = PathBuf::from("/project/src/components/Button.tsx");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("src.components.Button".to_string())
        );
    }

    #[test]
    fn test_declaration_file() {
        let root = PathBuf::from("/project");
        let file = PathBuf::from("/project/types.d.ts");
        assert_eq!(derive_module_path(&file, &root), Some("types".to_string()));
    }

    #[test]
    fn test_nested_declaration_file() {
        let root = PathBuf::from("/project");
        let file = PathBuf::from("/project/types/global.d.ts");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("types.global".to_string())
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
        let root = PathBuf::from("/project");
        let file = PathBuf::from("/project/components/App.jsx");
        assert_eq!(
            derive_module_path(&file, &root),
            Some("components.App".to_string())
        );
    }
}
