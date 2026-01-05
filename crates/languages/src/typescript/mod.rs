//! TypeScript language extractor module (STUBBED)
//!
//! This module is temporarily stubbed pending the new macro architecture implementation.
//! See issue #179 for the migration plan.
//!
//! ## Re-enabling TypeScript extraction
//!
//! When implementing the full extractor, ensure you:
//! 1. Use `define_language_extractor!` macro with a `fqn:` block
//! 2. Define `SCOPE_PATTERNS` for qualified name building
//! 3. Provide `module_path_fn` if module path derivation is needed
//!
//! The macro will automatically register `ScopeConfiguration` via inventory.

use codesearch_core::{error::Result, CodeEntity};
use std::path::{Path, PathBuf};

/// TypeScript extractor (stubbed)
///
/// Returns empty entity vectors. Full implementation pending macro architecture migration.
pub struct TypeScriptExtractor;

impl TypeScriptExtractor {
    /// Create a new TypeScript extractor
    pub fn new(
        _repository_id: String,
        _package_name: Option<String>,
        _source_root: Option<PathBuf>,
        _repo_root: PathBuf,
    ) -> Result<Self> {
        Ok(Self)
    }
}

impl crate::Extractor for TypeScriptExtractor {
    fn extract(&self, _source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        tracing::warn!(
            "TypeScript extraction is currently disabled (pending macro migration). \
             File will not be indexed: {}",
            file_path.display()
        );
        Ok(Vec::new())
    }
}

inventory::submit! {
    crate::LanguageDescriptor {
        name: "typescript",
        extensions: &["ts", "tsx"],
        factory: |repo_id, pkg_name, src_root, repo_root| Ok(Box::new(TypeScriptExtractor::new(
            repo_id.to_string(),
            pkg_name.map(String::from),
            src_root.map(PathBuf::from),
            repo_root.to_path_buf(),
        )?)),
    }
}
