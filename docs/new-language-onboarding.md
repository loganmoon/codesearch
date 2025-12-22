# Adding a New Language to Codesearch

> **Note**: For a comprehensive guide with detailed examples, see
> [`crates/languages/docs/new-language-onboarding.md`](../crates/languages/docs/new-language-onboarding.md).
> This document provides a quick overview.

This guide explains how to add support for a new programming language to the codesearch indexing pipeline.

## Overview

Adding a new language involves:

1. Creating a language extractor in `crates/languages/`
2. Defining tree-sitter queries for entity extraction
3. Implementing handler functions for each entity type
4. Ensuring relationship resolution works correctly

## Language Extractor Structure

Each language module should follow this structure:

```
crates/languages/src/{language}/
├── mod.rs              # Language definition using define_language_extractor! macro
├── queries.rs          # Tree-sitter query strings
├── handler_impls/      # Handler implementations
│   ├── mod.rs          # Re-exports all handlers
│   ├── function_handlers.rs
│   ├── class_handlers.rs
│   ├── module_handlers.rs
│   └── ...
└── utils.rs            # Language-specific utilities (optional)
```

## Entity Attributes for Relationship Resolution

The relationship resolution system (in `crates/outbox-processor/src/neo4j_relationship_resolver.rs`) looks for specific attributes on entities to create graph relationships. Your language handlers MUST populate these attributes correctly for relationships to be resolved.

### Required Attributes by Relationship Type

#### CONTAINS (parent-child containment)
- **Source**: `parent_scope` field on any entity
- **Format**: Qualified name of the parent entity
- **Created by**: `ContainsResolver`

#### INHERITS_FROM (class inheritance)
- **Source**: Entity metadata attributes
- **Attribute names checked**:
  - `extends` - For JS/TS classes (single parent class name)
  - `bases` - For Python classes (JSON array of base class names)
- **Format**:
  - `extends`: Simple class name, e.g., `"BaseClass"`
  - `bases`: JSON array, e.g., `["Parent1", "Parent2"]`
- **Created by**: `InheritanceResolver`

#### IMPLEMENTS (interface/trait implementation)
- **Source**: Entity metadata attributes
- **Attribute names checked**:
  - `implements_trait` - For Rust
  - `implements` - For Java/TypeScript
- **Format**: Trait/interface name
- **Created by**: `ImplementsResolver`

#### CALLS (function/method calls)
- **Source**: `calls` attribute on functions/methods
- **Format**: JSON array of called function names
- **Example**: `["helper_fn", "utils.process", "external::std::println"]`
- **Created by**: `resolve_external_references()` for external calls

#### USES (type usage)
- **Source**: `uses_types` attribute on functions/methods/classes
- **Format**: JSON array of type names used
- **Example**: `["String", "HashMap", "MyStruct"]`
- **Created by**: `TypeUsageResolver` for internal, `resolve_external_references()` for external

#### IMPORTS (module imports)
- **Source**: `imports` attribute on Module entities
- **Format**: JSON array of import paths
- **Example**: `["./utils", "../core", "react"]`
- **Resolution**:
  - Relative paths (`./`, `../`) are resolved relative to the importing module's location
  - Bare specifiers (`react`, `lodash`) are matched by simple name
- **Created by**: `ImportsResolver`

### Qualified Name Separator Convention

The resolution system handles different separator conventions:

| Language | Separator | Example |
|----------|-----------|---------|
| Rust | `::` | `crate::utils::helpers` |
| JavaScript/TypeScript | `.` | `src.utils.helpers` |
| Python | `.` | `mypackage.utils.helpers` |

The `is_external_ref()` function handles both `::` and `.` separators when extracting simple names for matching.

### External References

References that cannot be resolved to entities within the repository become "External" stub nodes. The system identifies external references by:

1. Explicit prefix: `external::` or `external.`
2. Not matching any known qualified name
3. Simple name not found in the entity map

External nodes are created with:
- `id`: Generated from the qualified name
- `qualified_name`: The full reference path
- `name`: Simple name (last segment)
- `package`: Optional package/crate name
- `repository_id`: For proper multi-repo isolation

## Example: Implementing a Class Handler

```rust
pub fn handle_class_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
) -> Result<Vec<CodeEntity>> {
    let class_node = require_capture_node(query_match, query, "class")?;

    // Build basic entity...
    let mut metadata = EntityMetadata::default();

    // IMPORTANT: Set inheritance attribute for InheritanceResolver
    if let Some(extends_clause) = get_extends_clause(class_node, source) {
        // For JS/TS: single parent
        metadata.attributes.insert("extends".to_string(), extends_clause);

        // For Python: JSON array of bases
        // metadata.attributes.insert("bases".to_string(),
        //     serde_json::to_string(&base_classes)?);
    }

    // Set type usage for TypeUsageResolver
    let used_types = extract_type_references(class_node, source);
    if !used_types.is_empty() {
        metadata.attributes.insert(
            "uses_types".to_string(),
            serde_json::to_string(&used_types)?,
        );
    }

    // ... build and return entity
}
```

## Example: Implementing a Module Handler

```rust
pub fn handle_module_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
) -> Result<Vec<CodeEntity>> {
    let program_node = require_capture_node(query_match, query, "module")?;

    let mut metadata = EntityMetadata::default();

    // IMPORTANT: Extract imports for ImportsResolver
    let imports = extract_import_sources(program_node, source);
    if !imports.is_empty() {
        // Store as JSON array - this is what ImportsResolver expects
        metadata.attributes.insert(
            "imports".to_string(),
            serde_json::to_string(&imports)?,
        );
    }

    // Module qualified_name should be derived from file path
    // e.g., "src/utils/helpers.js" -> "src.utils.helpers"
    let qualified_name = derive_qualified_name(file_path, source_root, ".");

    // ... build and return entity
}
```

## Common Pitfalls

### 1. Import Path Format Mismatch

**Problem**: Storing raw import paths that don't match module qualified names.

**Wrong**: `imports: ["./utils", "../core"]` when modules have qualified names like `src.utils`

**Solution**: The `ImportsResolver` now handles relative path resolution. Store the raw import paths and ensure your module's `qualified_name` is derived from its file path using `.` separator.

### 2. Missing Inheritance Attributes

**Problem**: Not setting `extends` or `bases` attributes on class entities.

**Solution**:
- For JS/TS: Set `extends` attribute with the parent class name
- For Python: Set `bases` attribute with JSON array of base class names

### 3. Wrong Separator in Qualified Names

**Problem**: Using wrong separator for the language.

**Solution**:
- Rust: Use `::` separator
- JS/TS/Python: Use `.` separator

### 4. Missing Type References

**Problem**: Not extracting type references from function signatures, fields, etc.

**Solution**: Parse type annotations and store in `uses_types` attribute as JSON array.

## Testing Your Language Support

1. Create test fixtures in `crates/languages/src/{language}/tests/`
2. Run extraction tests: `cargo test --package codesearch-languages`
3. Run the e2e benchmark to verify relationship resolution:
   ```bash
   cargo test --manifest-path crates/e2e-tests/Cargo.toml --test test_resolution_e2e -- --ignored --nocapture
   ```

Check the benchmark output for:
- Non-zero relationship counts for your language
- Reasonable resolution rate (internal vs external)
- Expected relationship types (IMPORTS, INHERITS, CALLS, USES, etc.)
