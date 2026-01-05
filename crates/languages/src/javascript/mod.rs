//! JavaScript language extractor module (STUBBED)
//!
//! This module is temporarily stubbed pending the new macro architecture implementation.
//! See issue #179 for the migration plan.

use codesearch_core::{error::Result, CodeEntity};
use std::path::{Path, PathBuf};

/// JavaScript extractor (stubbed)
///
/// Returns empty entity vectors. Full implementation pending macro architecture migration.
pub struct JavaScriptExtractor;

impl JavaScriptExtractor {
    /// Create a new JavaScript extractor
    pub fn new(
        _repository_id: String,
        _package_name: Option<String>,
        _source_root: Option<PathBuf>,
        _repo_root: PathBuf,
    ) -> Result<Self> {
        Ok(Self)
    }
}

impl crate::Extractor for JavaScriptExtractor {
    fn extract(&self, _source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        tracing::debug!(
            "JavaScript extraction stubbed (pending macro migration): {}",
            file_path.display()
        );
        Ok(Vec::new())
    }
}

inventory::submit! {
    crate::LanguageDescriptor {
        name: "javascript",
        extensions: &["js", "jsx"],
        factory: |repo_id, pkg_name, src_root, repo_root| Ok(Box::new(JavaScriptExtractor::new(
            repo_id.to_string(),
            pkg_name.map(String::from),
            src_root.map(PathBuf::from),
            repo_root.to_path_buf(),
        )?)),
    }
}
