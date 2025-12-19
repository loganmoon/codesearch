# Language Onboarding and Relationship Extraction Guide

This document provides a comprehensive guide for adding new language support to codesearch,
with Rust as the canonical example. It covers the complete pipeline from AST parsing to
Neo4j graph creation.

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Directory Structure](#directory-structure)
3. [Step 1: Language Module Setup](#step-1-language-module-setup)
4. [Step 2: Tree-Sitter Queries](#step-2-tree-sitter-queries)
5. [Step 3: Handler Implementations](#step-3-handler-implementations)
6. [Step 4: Import Map Support](#step-4-import-map-support)
7. [Step 5: TSG Rules for Cross-File Resolution](#step-5-tsg-rules-for-cross-file-resolution)
8. [Step 6: Relationship Resolution](#step-6-relationship-resolution)
9. [Testing](#testing)
10. [Metadata Attributes Reference](#metadata-attributes-reference)

---

## Architecture Overview

The relationship extraction pipeline has two stages:

```
┌────────────────────────────────────────────────────────────────────────────┐
│                           INDEXING PHASE                                    │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│   Source File ──► Tree-Sitter Parser ──► AST                               │
│                                           │                                │
│                                           ▼                                │
│                        ┌──────────────────────────────────┐                │
│                        │  Language Extractor              │                │
│                        │  - Queries match AST patterns    │                │
│                        │  - Handlers build CodeEntity     │                │
│                        │  - Import map resolves refs      │                │
│                        └──────────────────────────────────┘                │
│                                           │                                │
│                                           ▼                                │
│                        ┌──────────────────────────────────┐                │
│                        │  PostgreSQL (entity_metadata)    │                │
│                        │  - entity_data JSON              │                │
│                        │  - metadata.attributes           │                │
│                        └──────────────────────────────────┘                │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
                                           │
                                           ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                          RESOLUTION PHASE                                   │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│                        ┌──────────────────────────────────┐                │
│                        │  Relationship Resolvers          │                │
│                        │  - Query entity_metadata         │                │
│                        │  - Match references to defs      │                │
│                        │  - Create Neo4j edges            │                │
│                        └──────────────────────────────────┘                │
│                                           │                                │
│                                           ▼                                │
│                        ┌──────────────────────────────────┐                │
│                        │  Neo4j Graph Database            │                │
│                        │  - Entity nodes                  │                │
│                        │  - Relationship edges            │                │
│                        └──────────────────────────────────┘                │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

---

## Directory Structure

Each language follows this structure (using Rust as example):

```
crates/languages/src/
├── rust/                           # Language module
│   ├── mod.rs                      # Module root with define_language_extractor! macro
│   ├── queries.rs                  # Tree-sitter query patterns
│   ├── module_path.rs              # Module path resolution logic
│   ├── entities.rs                 # (optional) Language-specific entity types
│   └── handler_impls/              # Entity extraction handlers
│       ├── mod.rs                  # Handler exports
│       ├── common.rs               # Shared extraction utilities
│       ├── function_handlers.rs    # Function/method extraction
│       ├── type_handlers.rs        # Struct/enum/trait extraction
│       ├── impl_handlers.rs        # Impl block extraction
│       ├── module_handlers.rs      # Module extraction
│       ├── constant_handlers.rs    # Const/static extraction
│       ├── type_alias_handlers.rs  # Type alias extraction
│       ├── macro_handlers.rs       # Macro extraction
│       └── tests/                  # Handler unit tests
│
├── common/                         # Shared utilities (language-agnostic)
│   ├── mod.rs
│   ├── import_map.rs               # Import resolution (per-language parsers)
│   └── entity_building.rs          # Common entity construction helpers
│
├── javascript/
│   ├── ...
│   └── utils.rs                    # JS/TS shared utilities (imported by TypeScript)
│
├── python/
│   ├── ...
│   └── utils.rs                    # Python-specific utilities
│
└── tsg/                            # Tree-sitter-graph rules
    ├── rust.tsg                    # Rust definition/import/reference extraction
    ├── javascript.tsg
    ├── typescript.tsg
    └── python.tsg
```

---

## Step 1: Language Module Setup

### 1.1 Create the module root (`mod.rs`)

The module root uses the `define_language_extractor!` macro:

```rust
// crates/languages/src/rust/mod.rs

pub(crate) mod handler_impls;
pub mod module_path;
pub(crate) mod queries;

use crate::qualified_name::{ScopeConfiguration, ScopePattern};
use codesearch_languages_macros::define_language_extractor;

/// Scope patterns define how qualified names are built from AST nesting
const RUST_SCOPE_PATTERNS: &[ScopePattern] = &[
    ScopePattern {
        node_kind: "mod_item",      // AST node type
        field_name: "name",          // Field containing the scope name
    },
    ScopePattern {
        node_kind: "impl_item",
        field_name: "type",
    },
];

// Register scope configuration globally
inventory::submit! {
    ScopeConfiguration {
        language: "rust",
        separator: "::",            // Rust uses :: as namespace separator
        patterns: RUST_SCOPE_PATTERNS,
    }
}

// Define the extractor
define_language_extractor! {
    language: Rust,                              // codesearch_core::Language variant
    tree_sitter: tree_sitter_rust::LANGUAGE,     // Tree-sitter grammar
    extensions: ["rs"],                          // File extensions

    entities: {
        // Each entity type maps a query to a handler
        function => {
            query: queries::FUNCTION_QUERY,
            handler: handler_impls::handle_function_impl
        },
        r#struct => {
            query: queries::STRUCT_QUERY,
            handler: handler_impls::handle_struct_impl
        },
        r#enum => {
            query: queries::ENUM_QUERY,
            handler: handler_impls::handle_enum_impl
        },
        r#trait => {
            query: queries::TRAIT_QUERY,
            handler: handler_impls::handle_trait_impl
        },
        r#impl => {
            query: queries::IMPL_QUERY,
            handler: handler_impls::handle_impl_impl
        },
        impl_trait => {
            query: queries::IMPL_TRAIT_QUERY,
            handler: handler_impls::handle_impl_trait_impl
        },
        module => {
            query: queries::MODULE_QUERY,
            handler: handler_impls::handle_module_impl
        },
        constant => {
            query: queries::CONSTANT_QUERY,
            handler: handler_impls::handle_constant_impl
        },
        type_alias => {
            query: queries::TYPE_ALIAS_QUERY,
            handler: handler_impls::handle_type_alias_impl
        },
        r#macro => {
            query: queries::MACRO_QUERY,
            handler: handler_impls::handle_macro_impl
        }
    }
}
```

### 1.2 Add to language registry

Update `crates/languages/src/lib.rs` to include your language module.

---

## Step 2: Tree-Sitter Queries

Queries use tree-sitter's S-expression syntax to match AST patterns.

### 2.1 Query file structure (`queries.rs`)

```rust
// crates/languages/src/rust/queries.rs

/// Query for function definitions
pub const FUNCTION_QUERY: &str = r#"
(function_item
  (visibility_modifier)? @vis        ; Optional visibility (pub, pub(crate), etc.)
  (function_modifiers)? @modifiers   ; async, const, unsafe, extern
  name: (identifier) @name           ; Function name (required)
  type_parameters: (type_parameters)? @generics  ; Generic parameters
  parameters: (parameters) @params   ; Parameter list
  return_type: (_)? @return          ; Return type annotation
  body: (block) @body                ; Function body
) @function                          ; Capture entire node
"#;

/// Query for struct definitions
pub const STRUCT_QUERY: &str = r#"
(struct_item
  (visibility_modifier)? @vis
  "struct"
  name: (type_identifier) @name
  type_parameters: (type_parameters)? @generics
  (where_clause)? @where
  body: [
    (field_declaration_list) @fields     ; Named fields: struct Foo { x: i32 }
    (ordered_field_declaration_list) @fields  ; Tuple fields: struct Foo(i32)
  ]?
) @struct
"#;

/// Query for trait implementation blocks
pub const IMPL_TRAIT_QUERY: &str = r#"
(impl_item
  type_parameters: (type_parameters)? @generics
  trait: (_) @trait                  ; Trait being implemented
  "for"
  type: (_) @type                    ; Type implementing the trait
  body: (declaration_list) @impl_body
) @impl_trait
"#;
```

### 2.2 Query design principles

1. **Capture names must match handler expectations**: Handlers use `require_capture_node()` and `find_capture_node()` to access captures
2. **Use `?` for optional captures**: Handlers check for presence with `find_capture_node()`
3. **Capture the entire node**: Always capture the root node (e.g., `@function`, `@struct`) for span information
4. **Use field names when available**: `name: (identifier)` is more precise than just `(identifier)`

---

## Step 3: Handler Implementations

Handlers extract `CodeEntity` objects from query matches.

### 3.1 Handler signature

```rust
pub fn handle_function_impl(
    query_match: &QueryMatch,           // Tree-sitter query match
    query: &Query,                      // The query object (for capture indices)
    source: &str,                       // Full source code
    file_path: &Path,                   // File being processed
    repository_id: &str,                // Repository UUID
    package_name: Option<&str>,         // Package/crate name
    source_root: Option<&Path>,         // Source root for relative paths
) -> Result<Vec<CodeEntity>>
```

### 3.2 Example handler (simplified from Rust)

```rust
// crates/languages/src/rust/handler_impls/function_handlers.rs

use crate::common::{
    entity_building::{build_entity, extract_common_components, EntityDetails, ExtractionContext},
    find_capture_node, import_map::{parse_file_imports, resolve_reference},
    node_to_text, require_capture_node,
};
use codesearch_core::{
    entities::{EntityMetadata, EntityType, FunctionSignature, Language, Visibility},
    error::Result,
    CodeEntity,
};

pub fn handle_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
) -> Result<Vec<CodeEntity>> {
    // Get required captures
    let function_node = require_capture_node(query_match, query, "function")?;
    let name_node = require_capture_node(query_match, query, "name")?;

    // Create extraction context
    let ctx = ExtractionContext {
        query_match,
        query,
        source,
        file_path,
        repository_id,
        package_name,
        source_root,
    };

    // Extract common components (name, qualified_name, parent_scope, etc.)
    let components = extract_common_components(&ctx, "name", function_node, "rust")?;

    // Parse imports for reference resolution
    let import_map = parse_file_imports(function_node.parent().unwrap(), source, Language::Rust);

    // Extract parameters
    let params_node = find_capture_node(query_match, query, "params");
    let parameters = extract_parameters(params_node, source, &import_map)?;

    // Extract return type
    let return_type = find_capture_node(query_match, query, "return")
        .and_then(|n| node_to_text(n, source).ok());

    // Extract type references for USES relationships
    let uses_types = extract_type_references(&parameters, &return_type, &import_map);

    // Build metadata
    let mut metadata = EntityMetadata {
        is_async: has_modifier(query_match, query, "async"),
        ..EntityMetadata::default()
    };

    // Store uses_types as JSON array for TypeUsageResolver
    if !uses_types.is_empty() {
        if let Ok(json) = serde_json::to_string(&uses_types) {
            metadata.attributes.insert("uses_types".to_string(), json);
        }
    }

    // Build entity
    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: Language::Rust,
            visibility: extract_visibility(query_match, query),
            documentation: extract_doc_comments(function_node, source),
            content: node_to_text(function_node, source).ok(),
            metadata,
            signature: Some(FunctionSignature {
                parameters,
                return_type,
                generics: extract_generics(query_match, query, source),
                is_async: has_modifier(query_match, query, "async"),
            }),
        },
    )?;

    Ok(vec![entity])
}
```

### 3.3 Handler module exports (`handler_impls/mod.rs`)

```rust
// crates/languages/src/rust/handler_impls/mod.rs

pub(crate) mod common;
pub(crate) mod function_handlers;
pub(crate) mod type_handlers;
pub(crate) mod impl_handlers;
// ... other handler modules

#[cfg(test)]
mod tests;

// Re-export handlers for use in mod.rs
pub use function_handlers::handle_function_impl;
pub use type_handlers::{handle_struct_impl, handle_enum_impl, handle_trait_impl};
pub use impl_handlers::{handle_impl_impl, handle_impl_trait_impl};
// ... other exports
```

---

## Step 4: Import Map Support

The import map enables resolving references to qualified names.

### 4.1 Add language support in `import_map.rs`

```rust
// crates/languages/src/common/import_map.rs

/// Parse imports from a file's AST root
///
/// The `current_module_path` parameter enables resolution of relative imports:
/// - For JS/TS: `./utils` resolves to `myapp.utils` if current file is `myapp.main.ts`
/// - For Python: `.models` resolves to `myapp.models` if current file is `myapp/__init__.py`
///
/// If `current_module_path` is None, relative imports are stored as-is.
pub fn parse_file_imports(
    root: Node,
    source: &str,
    language: Language,
    current_module_path: Option<&str>,  // Required for relative import resolution
) -> ImportMap {
    match language {
        Language::Rust => parse_rust_imports(root, source),
        Language::JavaScript => parse_js_imports(root, source, current_module_path),
        Language::TypeScript => parse_ts_imports(root, source, current_module_path),
        Language::Python => parse_python_imports(root, source, current_module_path),
        Language::NewLang => parse_newlang_imports(root, source, current_module_path),
        _ => ImportMap::new("."),
    }
}

/// Parse imports for your language
fn parse_newlang_imports(
    root: Node,
    source: &str,
    current_module_path: Option<&str>,
) -> ImportMap {
    let mut map = ImportMap::new("::");  // Use appropriate separator

    // Walk AST to find import statements
    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        if node.kind() == "import_statement" {
            // Extract imported name and source path
            // Resolve relative imports using current_module_path if needed
            // Add to map: map.add("LocalName", "module.path.LocalName");
        }
    }

    map
}
```

### 4.2 Using resolve_reference

```rust
use crate::common::import_map::{parse_file_imports, resolve_reference};
use crate::javascript::module_path::derive_module_path;

// In a handler, derive the current module path first:
let module_path = derive_module_path(file_path, source_root);

// Parse imports with module path for relative import resolution
let import_map = parse_file_imports(
    file_root,
    source,
    Language::JavaScript,
    module_path.as_deref(),  // Pass the module path
);

// Resolve a reference like "Config" to its qualified name
let resolved = resolve_reference(
    "Config",            // Name to resolve
    &import_map,         // Import map
    Some("myapp.main"),  // Parent scope (for local resolution)
    "."                  // Namespace separator (JS/Python use ".", Rust uses "::")
);
// Returns:
// - "myapp.config.Config" if imported from ./config
// - "myapp.main.Config" if defined locally
// - "external.Config" if unresolved (external dependency)
```

### 4.3 Relative Import Resolution

For languages with relative imports (JS/TS, Python), the `current_module_path` is used
to convert relative paths to absolute qualified names at extraction time:

```rust
// JS/TS: resolve_relative_import converts "./core" based on importer location
// If current file is "vanilla/atom.ts" (module_path = "vanilla.atom"):
//   "./core" → "vanilla.core"
//   "../utils" → "utils"

// Python: resolve_python_relative_import handles "." and ".." syntax
// If current file is "myapp/models/__init__.py" (module_path = "myapp.models"):
//   ".base" → "myapp.models.base"
//   "..utils" → "myapp.utils"
```

This ensures that references stored in entity attributes (e.g., `calls`, `uses_types`,
`extends_resolved`) match the `qualified_name` format used by entity definitions,
enabling proper relationship resolution in Neo4j.

---

## Step 5: TSG Rules for Cross-File Resolution

TSG (Tree-Sitter-Graph) rules extract nodes for cross-file symbol resolution.

### 5.1 Create TSG file (`tsg/newlang.tsg`)

```scheme
; crates/languages/src/tsg/rust.tsg

; ============================================================
; DEFINITIONS - Items that introduce new names
; ============================================================

; Function definitions
(function_item
  (visibility_modifier)? @vis
  name: (identifier) @name) @def
{
  node @def.node
  attr (@def.node) type = "Definition"
  attr (@def.node) kind = "function"
  attr (@def.node) name = (source-text @name)
  attr (@def.node) start_row = (start-row @def)
  attr (@def.node) end_row = (end-row @def)
  if some @vis {
    attr (@def.node) visibility = (source-text @vis)
  }
}

; Struct definitions
(struct_item
  (visibility_modifier)? @vis
  name: (type_identifier) @name) @def
{
  node @def.node
  attr (@def.node) type = "Definition"
  attr (@def.node) kind = "struct"
  attr (@def.node) name = (source-text @name)
  attr (@def.node) start_row = (start-row @def)
  attr (@def.node) end_row = (end-row @def)
}

; ============================================================
; IMPORTS - Use declarations
; ============================================================

; Simple use: `use foo::Bar;`
(use_declaration
  (visibility_modifier)? @_vis
  argument: (scoped_identifier) @path) @import
{
  node @import.node
  attr (@import.node) type = "Import"
  attr (@import.node) name = (source-text @path)
  attr (@import.node) path = (source-text @path)
  attr (@import.node) is_glob = "false"
  attr (@import.node) start_row = (start-row @import)
  attr (@import.node) end_row = (end-row @import)
}

; ============================================================
; REFERENCES - Identifier usages
; ============================================================

; Function call: `foo()`
(call_expression
  function: (identifier) @ref) @_call
{
  node @ref.node
  attr (@ref.node) type = "Reference"
  attr (@ref.node) name = (source-text @ref)
  attr (@ref.node) context = "call"
  attr (@ref.node) start_row = (start-row @ref)
  attr (@ref.node) end_row = (end-row @ref)
}

; Type reference
(type_identifier) @ref
{
  node @ref.node
  attr (@ref.node) type = "Reference"
  attr (@ref.node) name = (source-text @ref)
  attr (@ref.node) context = "type"
  attr (@ref.node) start_row = (start-row @ref)
  attr (@ref.node) end_row = (end-row @ref)
}
```

---

## Step 6: Relationship Resolution

Resolvers in `crates/outbox-processor/src/neo4j_relationship_resolver.rs` create
Neo4j edges from entity metadata.

### 6.1 Available Resolvers

| Resolver | Relationships | Required Attributes |
|----------|--------------|---------------------|
| `ContainsResolver` | CONTAINS | `parent_scope` (automatic) |
| `TraitImplResolver` | IMPLEMENTS, ASSOCIATES, EXTENDS_INTERFACE | `implements_trait`, `for_type`, `extends` |
| `InheritanceResolver` | INHERITS_FROM, HAS_SUBCLASS | `extends`, `bases` |
| `TypeUsageResolver` | USES, USED_BY | `fields`, `uses_types` |
| `CallGraphResolver` | CALLS, CALLED_BY | `calls` |
| `ImportsResolver` | IMPORTS, IMPORTED_BY | `imports` |
| `ExternalResolver` | Creates External nodes | All unresolved refs |

### 6.2 Adding Metadata for Resolution

Handlers must populate specific metadata attributes for resolvers to work:

```rust
// In impl_handlers.rs - for TraitImplResolver
metadata.attributes.insert("implements_trait".to_string(), trait_name.clone());
metadata.attributes.insert(
    "implements_trait_resolved".to_string(),
    resolve_reference(&trait_name, &import_map, None, "::")
);
metadata.attributes.insert("for_type".to_string(), for_type.clone());

// In type_handlers.rs - for TypeUsageResolver (fields)
metadata.attributes.insert("fields".to_string(), serde_json::to_string(&fields)?);

// In function_handlers.rs - for TypeUsageResolver (signatures)
metadata.attributes.insert("uses_types".to_string(), serde_json::to_string(&type_refs)?);

// For CallGraphResolver
metadata.attributes.insert("calls".to_string(), serde_json::to_string(&call_names)?);
```

---

## Testing

### E2E Tests (Authoritative)

The authoritative tests for relationship resolution are in:
`crates/e2e-tests/tests/test_resolution_e2e.rs`

These tests exercise the complete pipeline against real codebases:

```bash
cargo test --manifest-path crates/e2e-tests/Cargo.toml \
    --test test_resolution_e2e -- --ignored --nocapture
```

### Unit Tests for Handlers

Each handler module should have unit tests:

```rust
// crates/languages/src/rust/handler_impls/tests/function_tests.rs

#[test]
fn test_function_extracts_uses_types() {
    let source = r#"
        use std::collections::HashMap;

        fn process(map: HashMap<String, i32>) -> Option<i32> {
            map.get("key").copied()
        }
    "#;

    let entities = extract_entities(source, Language::Rust);
    let func = entities.iter().find(|e| e.name == "process").unwrap();

    let uses_types: Vec<String> = serde_json::from_str(
        func.metadata.attributes.get("uses_types").unwrap()
    ).unwrap();

    assert!(uses_types.contains(&"std::collections::HashMap".to_string()));
    assert!(uses_types.contains(&"Option".to_string()));
}
```

---

## Metadata Attributes Reference

### Automatic Attributes

| Attribute | Description | Set By |
|-----------|-------------|--------|
| `parent_scope` | Qualified name of containing entity | Qualified name builder |

### Relationship Attributes

| Attribute | Format | Used By | Example |
|-----------|--------|---------|---------|
| `implements_trait` | String | TraitImplResolver | `"Display"` |
| `implements_trait_resolved` | String | TraitImplResolver | `"std::fmt::Display"` |
| `for_type` | String | TraitImplResolver | `"MyStruct"` |
| `extends` | String | InheritanceResolver | `"BaseClass"` |
| `extends_resolved` | String | InheritanceResolver | `"module.BaseClass"` |
| `bases` | String (comma-sep) | InheritanceResolver | `"Base1, Base2"` |
| `bases_resolved` | JSON array | InheritanceResolver | `["mod.Base1", "mod.Base2"]` |
| `fields` | JSON array | TypeUsageResolver | `[{"name": "x", "field_type": "i32"}]` |
| `uses_types` | JSON array | TypeUsageResolver | `["Config", "Result"]` |
| `calls` | JSON array | CallGraphResolver | `["process", "validate"]` |
| `imports` | JSON array | ImportsResolver | `["std::io", "crate::utils"]` |

### Current Language Support Status

| Language | Extraction | Resolution | Notes |
|----------|-----------|------------|-------|
| **Rust** | Full | Full | Canonical implementation |
| **JavaScript** | Full | Full | Complete with `calls`, `uses_types`, `extends_resolved` |
| **TypeScript** | Full | Full | Complete with `calls`, `uses_types`, `extends_resolved`, `implements_trait_resolved` |
| **Python** | Full | Full | Complete with `calls`, `uses_types`, `bases_resolved` |
| **Go** | Infrastructure | None | Grammar exists, handlers not implemented |

All languages (except Go) now support:
- Relative import resolution at extraction time
- `calls` attribute for function/method calls
- `uses_types` attribute for type references
- `*_resolved` attributes for inheritance/implementation

---

## Checklist for Adding a New Language

1. [ ] Create language directory: `crates/languages/src/newlang/`
2. [ ] Add `mod.rs` with `define_language_extractor!` macro
3. [ ] Create `queries.rs` with tree-sitter queries
4. [ ] Create `handler_impls/` directory with handlers
5. [ ] Add import parser in `common/import_map.rs`
6. [ ] Create `tsg/newlang.tsg` for cross-file resolution
7. [ ] Add language to `Language` enum in `crates/core/src/entities.rs`
8. [ ] Add file extension mapping
9. [ ] Write handler unit tests
10. [ ] Test with e2e resolution tests
