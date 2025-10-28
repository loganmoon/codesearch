# Codesearch Languages Macros

Procedural macros for defining language extractors in codesearch with minimal boilerplate.

## Overview

The `define_language_extractor!` macro automates the creation of language extractors by generating:

1. **Extractor Struct** - The main extractor type for the language
2. **Constructor** - Builds the language configuration with all entity extractors
3. **Extractor Trait Implementation** - Implements the `Extractor` trait
4. **Inventory Registration** - Automatically registers the language for discovery
5. **Handler Wrappers** - Generates wrapper functions for entity handlers

## Usage

### Basic Example

```rust
use codesearch_languages_macros::define_language_extractor;

pub(crate) mod queries;
pub(crate) mod handlers;

define_language_extractor! {
    language: JavaScript,
    tree_sitter: tree_sitter_javascript::LANGUAGE,
    extensions: ["js", "jsx"],

    entities: {
        function => {
            query: queries::FUNCTION_QUERY,
            handler: handlers::handle_function_impl,
        },
        class => {
            query: queries::CLASS_QUERY,
            handler: handlers::handle_class_impl,
        }
    }
}
```

### DSL Syntax

The macro accepts the following fields:

- **`language`** (required): The name of the language (e.g., `JavaScript`, `TypeScript`)
- **`tree_sitter`** (required): Expression that evaluates to a tree-sitter `Language`
- **`extensions`** (required): Array of file extensions (e.g., `["js", "jsx"]`)
- **`entities`** (required): Map of entity types to their configurations

Each entity configuration requires:
- **`query`**: Tree-sitter query constant (typically from a `queries` module)
- **`handler`**: Handler implementation function (typically from a `handlers` module)

### Generated Code

For the example above, the macro generates approximately:

```rust
pub struct JavaScriptExtractor {
    repository_id: String,
    config: LanguageConfiguration,
}

impl JavaScriptExtractor {
    pub fn new(repository_id: String) -> Result<Self> {
        let language = tree_sitter_javascript::LANGUAGE.into();
        let config = LanguageConfigurationBuilder::new(language)
            .add_extractor("function", queries::FUNCTION_QUERY, Box::new(handlers::handle_function))
            .add_extractor("class", queries::CLASS_QUERY, Box::new(handlers::handle_class))
            .build()?;
        Ok(Self { repository_id, config })
    }
}

impl Extractor for JavaScriptExtractor {
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        let mut extractor = GenericExtractor::new(&self.config, self.repository_id.clone())?;
        extractor.extract(source, file_path)
    }
}

inventory::submit! {
    crate::LanguageDescriptor {
        name: "javascript",
        extensions: &["js", "jsx"],
        factory: |repo_id| Ok(Box::new(JavaScriptExtractor::new(repo_id.to_string())?)),
    }
}

mod handlers {
    pub fn handle_function(
        query_match: &QueryMatch,
        query: &Query,
        source: &str,
        file_path: &Path,
        repository_id: &str,
    ) -> Result<Vec<CodeEntity>> {
        handle_function_impl(query_match, query, source, file_path, repository_id)
    }

    pub fn handle_class(
        query_match: &QueryMatch,
        query: &Query,
        source: &str,
        file_path: &Path,
        repository_id: &str,
    ) -> Result<Vec<CodeEntity>> {
        handle_class_impl(query_match, query, source, file_path, repository_id)
    }
}
```

## Required Module Structure

When using this macro, your language module should have:

1. **`mod.rs`** - Uses the macro to define the extractor
2. **`queries.rs`** - Contains tree-sitter query constants
3. **`handlers/`** - Contains implementation functions for each entity type

### Example Directory Structure

```
crates/languages/src/javascript/
├── mod.rs              # Uses define_language_extractor! macro
├── queries.rs          # FUNCTION_QUERY, CLASS_QUERY, etc.
└── handlers/
    ├── mod.rs          # Re-exports handler implementations
    ├── function_handlers.rs  # handle_function_impl
    └── class_handlers.rs     # handle_class_impl
```

## Handler Function Signature

All handler implementation functions must have this signature:

```rust
pub fn handle_entity_impl(
    query_match: &tree_sitter::QueryMatch,
    query: &tree_sitter::Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
) -> codesearch_core::error::Result<Vec<codesearch_core::CodeEntity>>
```

## Benefits

Using this macro reduces boilerplate by approximately **60-80%**:

- **Without macro**: ~70 lines of repetitive code per language
- **With macro**: ~10-15 lines of declarative configuration

This allows adding new languages in **1-2 days** instead of 5-7 days.

## Limitations

- Entity names in the `entities` block must be valid Rust identifiers
- Handler functions must follow the exact signature shown above
- The macro generates code in the module where it's invoked, so proper visibility modifiers are important

## Debugging

To see the expanded code generated by the macro, use:

```bash
cargo expand --package codesearch-languages javascript
```

Replace `javascript` with the name of your language module.

## Error Messages

The macro provides helpful error messages for common mistakes:

- `Missing 'language' field` - You must specify the language name
- `Missing 'tree_sitter' field` - You must specify the tree-sitter language
- `Missing 'extensions' field` - You must specify file extensions
- `Missing 'entities' field` - You must define at least one entity type
- `Missing 'query' field` - Each entity must have a query
- `Missing 'handler' field` - Each entity must have a handler
- `Unknown field: X` - Unrecognized field in the macro invocation
