//! Code tokenizer for BM25 sparse embeddings

use unicode_segmentation::UnicodeSegmentation;

/// Custom tokenizer for code that implements BM25 tokenization strategy
///
/// Tokenization strategy:
/// 1. Split on whitespace
/// 2. Split on underscores (snake_case: get_user_name → ["get", "user", "name"])
/// 3. Split on camelCase boundaries (getUserName → ["get", "User", "Name"])
/// 4. Normalize to lowercase
/// 5. Filter empty tokens
#[derive(Debug, Clone, Default)]
pub struct CodeTokenizer;

impl CodeTokenizer {
    pub fn new() -> Self {
        Self
    }

    /// Split a camelCase or PascalCase string into components
    ///
    /// Examples:
    /// - "getUserName" → ["get", "user", "name"]
    /// - "HTTPResponse" → ["http", "response"]
    /// - "IOError" → ["io", "error"]
    fn split_camel_case(s: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut current = String::new();
        let chars: Vec<char> = s.chars().collect();

        for i in 0..chars.len() {
            let ch = chars[i];

            let should_split = if i > 0 {
                let prev = chars[i - 1];

                // Split on lowercase → uppercase transition (camelCase)
                (prev.is_lowercase() && ch.is_uppercase())
                // Split on multiple uppercase followed by lowercase (HTTPResponse)
                || (i + 1 < chars.len()
                    && prev.is_uppercase()
                    && ch.is_uppercase()
                    && chars[i + 1].is_lowercase())
            } else {
                false
            };

            if should_split && !current.is_empty() {
                result.push(current.clone());
                current.clear();
            }

            current.push(ch);
        }

        if !current.is_empty() {
            result.push(current);
        }

        result
    }
}

impl bm25::Tokenizer for CodeTokenizer {
    fn tokenize(&self, text: &str) -> Vec<String> {
        let mut tokens = Vec::new();

        // Split on whitespace first
        for word in text.unicode_words() {
            // Split on underscores (snake_case)
            for part in word.split('_') {
                if part.is_empty() {
                    continue;
                }

                // Split on camelCase boundaries
                for subpart in Self::split_camel_case(part) {
                    if !subpart.is_empty() {
                        // Normalize to lowercase
                        tokens.push(subpart.to_lowercase());
                    }
                }
            }
        }

        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm25::Tokenizer;

    #[test]
    fn test_snake_case_splitting() {
        let tokenizer = CodeTokenizer::new();
        let result = tokenizer.tokenize("get_user_name");
        assert_eq!(result, vec!["get", "user", "name"]);
    }

    #[test]
    fn test_camel_case_splitting() {
        let tokenizer = CodeTokenizer::new();
        let result = tokenizer.tokenize("getUserName");
        assert_eq!(result, vec!["get", "user", "name"]);
    }

    #[test]
    fn test_pascal_case_splitting() {
        let tokenizer = CodeTokenizer::new();
        let result = tokenizer.tokenize("GetUserName");
        assert_eq!(result, vec!["get", "user", "name"]);
    }

    #[test]
    fn test_uppercase_acronyms() {
        let tokenizer = CodeTokenizer::new();
        let result = tokenizer.tokenize("HTTPResponse");
        assert_eq!(result, vec!["http", "response"]);
    }

    #[test]
    fn test_mixed_patterns() {
        let tokenizer = CodeTokenizer::new();
        let result = tokenizer.tokenize("parse_HTTPRequest");
        assert_eq!(result, vec!["parse", "http", "request"]);
    }

    #[test]
    fn test_code_example() {
        let tokenizer = CodeTokenizer::new();
        let result = tokenizer.tokenize("fn calculate_sum(a: i32, b: i32) -> i32");
        assert_eq!(
            result,
            vec!["fn", "calculate", "sum", "a", "i32", "b", "i32", "i32"]
        );
    }

    #[test]
    fn test_empty_input() {
        let tokenizer = CodeTokenizer::new();
        let result = tokenizer.tokenize("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_lowercase_normalization() {
        let tokenizer = CodeTokenizer::new();
        let result = tokenizer.tokenize("FOO_BAR");
        assert_eq!(result, vec!["foo", "bar"]);
    }
}
