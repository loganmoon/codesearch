//! Test language module for validating the define_language_extractor macro
//!
//! This module is only compiled in test mode to verify the macro generates correct code.

#[cfg(test)]
mod queries;

#[cfg(test)]
mod handler_impls {
    use codesearch_core::{error::Result, CodeEntity};
    use std::path::Path;
    use tree_sitter::{Query, QueryMatch};

    #[allow(unused_variables)]
    #[allow(clippy::too_many_arguments)]
    pub fn handle_test_impl(
        _query_match: &QueryMatch,
        _query: &Query,
        _source: &str,
        _file_path: &Path,
        _repository_id: &str,
        package_name: Option<&str>,
        source_root: Option<&Path>,
        _repo_root: &Path,
    ) -> Result<Vec<CodeEntity>> {
        // Minimal implementation for testing
        Ok(Vec::new())
    }
}

#[cfg(test)]
use codesearch_languages_macros::define_language_extractor;

#[cfg(test)]
define_language_extractor! {
    language: TestLanguage,
    tree_sitter: tree_sitter_rust::LANGUAGE,
    extensions: ["test"],

    entities: {
        test => {
            query: queries::TEST_QUERY,
            handler: handler_impls::handle_test_impl,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macro_generates_extractor() {
        use std::path::PathBuf;
        // Verify the extractor can be created
        let result = TestLanguageExtractor::new(
            "test-repo".to_string(),
            None,
            None,
            PathBuf::from("/test-repo"),
        );
        if let Err(e) = &result {
            eprintln!("Error creating extractor: {e:?}");
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_extractor_implements_trait() {
        use crate::Extractor;
        use std::path::PathBuf;

        let extractor = TestLanguageExtractor::new(
            "test-repo".to_string(),
            None,
            None,
            PathBuf::from("/test-repo"),
        )
        .unwrap();

        // Verify extract method exists (will return empty vec for test implementation)
        let result = extractor.extract("", std::path::Path::new("test.test"));
        assert!(result.is_ok());
    }
}
