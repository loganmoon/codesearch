# Language Onboarding Guide

This guide covers adding new language support to codesearch, using Rust as the canonical example.

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Specification Files](#specification-files)
3. [Directory Structure](#directory-structure)
4. [Step 1: Language Module Setup](#step-1-language-module-setup)
5. [Step 2: Tree-Sitter Queries](#step-2-tree-sitter-queries)
6. [Step 3: Handler Implementations](#step-3-handler-implementations)
7. [Step 4: Relationship Data Extraction](#step-4-relationship-data-extraction)
8. [Step 5: Import Resolution](#step-5-import-resolution)
9. [Testing](#testing)
10. [Checklist](#checklist)

---

## Architecture Overview

> **🔒 ARCHITECTURAL REQUIREMENT**
>
> All extraction handlers **MUST** be implemented using the `LanguageExtractors` trait with the `define_handler!` macro. This architecture is mandatory for:
> - Entity extraction handlers
> - Relationship data extraction
> - Visibility and documentation extraction
>
> The macro supports all extraction patterns through various parameters (`metadata:`, `relationships:`, `visibility:`, `name:`, `name_fn:`, `name_ctx_fn:`, `module_name_fn:`). See [Step 3: Handler Implementations](#step-3-handler-implementations) for details.

The extraction and resolution pipeline:

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
│                        │  - EntityRelationshipData typed  │                │
│                        │  - Import map resolves refs      │                │
│                        └──────────────────────────────────┘                │
│                                           │                                │
│                                           ▼                                │
│                        ┌──────────────────────────────────┐                │
│                        │  PostgreSQL (entity_metadata)    │                │
│                        │  - Typed EntityRelationshipData  │                │
│                        │  - SourceReference with is_ext   │                │
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
│                        │  GenericResolver                 │                │
│                        │  - RelationshipDef config        │                │
│                        │  - LookupStrategy chains         │                │
│                        │  - Typed field extractors        │                │
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

### Key Concepts

**EntityRelationshipData**: Typed struct on each entity containing relationship references:
- `calls: Vec<SourceReference>` - Function/method calls
- `uses_types: Vec<SourceReference>` - Type references
- `imports: Vec<SourceReference>` - Import statements
- `implements_trait: Option<SourceReference>` - Trait being implemented
- `for_type: Option<SourceReference>` - Type for impl block
- `extends: Vec<SourceReference>` - Parent class/interface
- `extended_types: Vec<SourceReference>` - Extended types (Rust trait bounds, TS interface extends)
- `call_aliases: Vec<String>` - UFCS aliases for Rust

**SourceReference**: Reference with resolution metadata:
- `target: String` - Qualified name of target
- `simple_name: String` - Last path segment
- `is_external: bool` - Whether target is outside repository
- `location: SourceLocation` - Source position
- `ref_type: ReferenceType` - Call, TypeUsage, Import, Extends, Uses

**GenericResolver**: Configurable resolver using `RelationshipDef`:
- Source/target entity types
- `RelationshipType` enum value
- `LookupStrategy` chain (QualifiedName, PathEntityIdentifier, CallAliases, UniqueSimpleName, SimpleName)

---

## Specification Files

Each language should have a specification file defining extraction rules. See `crates/languages/specs/rust.yaml` as the canonical example.

Rule ID conventions: `E-xxx` (entity), `V-xxx` (visibility), `Q-xxx` (qualified names), `R-xxx` (relationships), `M-xxx` (metadata).

---

## Directory Structure

```
crates/languages/
├── specs/
│   └── rust.yaml                   # Language specification
├── src/
│   ├── rust/                       # Language module
│   │   ├── mod.rs                  # define_language_extractor! macro
│   │   ├── queries.rs              # Tree-sitter queries
│   │   ├── module_path.rs          # Module path resolution
│   │   └── handler_impls/          # Entity extraction handlers
│   │       ├── mod.rs
│   │       ├── common.rs           # Shared utilities
│   │       ├── function_handlers.rs
│   │       ├── type_handlers.rs
│   │       ├── impl_handlers.rs
│   │       ├── module_handlers.rs
│   │       └── tests/              # Handler unit tests
│   │
│   ├── common/                     # Shared utilities
│   │   ├── language_extractors.rs  # LanguageExtractors trait + define_handler! macro
│   │   ├── import_map.rs           # Import resolution
│   │   ├── entity_building.rs      # Entity construction
│   │   └── js_ts_shared/           # Shared JS/TS infrastructure
│   │       ├── extractors.rs       # JavaScript, TypeScript trait implementations
│   │       └── ...
│   │
│   └── {language}/                 # Other languages follow same structure
```

---

## Step 1: Language Module Setup

### 1.1 Create module root (`mod.rs`)

```rust
// crates/languages/src/rust/mod.rs

pub(crate) mod handler_impls;
pub mod module_path;
pub(crate) mod queries;

use crate::qualified_name::{ScopeConfiguration, ScopePattern};
use codesearch_languages_macros::define_language_extractor;

const RUST_SCOPE_PATTERNS: &[ScopePattern] = &[
    ScopePattern {
        node_kind: "mod_item",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "impl_item",
        field_name: "type",
    },
];

inventory::submit! {
    ScopeConfiguration {
        language: "rust",
        separator: "::",
        patterns: RUST_SCOPE_PATTERNS,
    }
}

define_language_extractor! {
    language: Rust,
    tree_sitter: tree_sitter_rust::LANGUAGE,
    extensions: ["rs"],

    entities: {
        function => {
            query: queries::FUNCTION_QUERY,
            handler: handler_impls::handle_function_impl
        },
        r#struct => {
            query: queries::STRUCT_QUERY,
            handler: handler_impls::handle_struct_impl
        },
        // ... other entities
    }
}
```

### 1.2 Add to language registry

Update `crates/languages/src/lib.rs` to include your language module.

---

## Step 2: Tree-Sitter Queries

### 2.1 Query file (`queries.rs`)

```rust
pub const FUNCTION_QUERY: &str = r#"
(function_item
  (visibility_modifier)? @vis
  (function_modifiers)? @modifiers
  name: (identifier) @name
  type_parameters: (type_parameters)? @generics
  parameters: (parameters) @params
  return_type: (_)? @return
  body: (block) @body
) @function
"#;

pub const IMPL_TRAIT_QUERY: &str = r#"
(impl_item
  type_parameters: (type_parameters)? @generics
  trait: (_) @trait
  "for"
  type: (_) @type
  body: (declaration_list) @impl_body
) @impl_trait
"#;
```

### 2.2 Design Principles

1. Capture names must match handler expectations
2. Use `?` for optional captures
3. Always capture the root node for span information
4. Use field names when available: `name: (identifier)` not just `(identifier)`

---

## Step 3: Handler Implementations

### 3.1 The LanguageExtractors Trait

The `LanguageExtractors` trait defines language-specific extraction behavior. Implement this for each language:

```rust
// crates/languages/src/common/language_extractors.rs

pub trait LanguageExtractors {
    /// The Language enum variant for this language
    const LANGUAGE: Language;

    /// String identifier used for qualified name building
    const LANG_STR: &'static str;

    /// Extract visibility from an AST node (e.g., `pub`, `export`)
    fn extract_visibility(node: Node, source: &str) -> Visibility;

    /// Extract documentation comments (e.g., `///`, `/** */`, docstrings)
    fn extract_docs(node: Node, source: &str) -> Option<String>;
}
```

Example implementation for JavaScript:

```rust
// crates/languages/src/common/js_ts_shared/extractors.rs

pub struct JavaScript;

impl LanguageExtractors for JavaScript {
    const LANGUAGE: Language = Language::JavaScript;
    const LANG_STR: &'static str = "javascript";

    fn extract_visibility(node: Node, source: &str) -> Visibility {
        extract_visibility(node, source)  // Language-specific function
    }

    fn extract_docs(node: Node, source: &str) -> Option<String> {
        extract_preceding_doc_comments(node, source)  // JSDoc extraction
    }
}
```

### 3.2 Handler Signature

All handlers take a single `ExtractionContext` parameter that bundles query match data and file context:

```rust
pub(crate) fn handle_function_impl(
    ctx: &ExtractionContext,
) -> Result<Vec<CodeEntity>>
```

The `ExtractionContext` contains:
- `query_match: &QueryMatch` - Tree-sitter query match
- `query: &Query` - Tree-sitter query (for capture name lookup)
- `source: &str` - Source code
- `file_path: &Path` - Path to the file
- `repository_id: &str` - Repository identifier
- `package_name: Option<&str>` - Package/crate name
- `source_root: Option<&Path>` - Source root directory
- `repo_root: &Path` - Repository root

### 3.3 Using the define_handler! Macro (Required)

The `define_handler!` macro generates handlers using the trait-based extraction framework. **All handlers must use this macro** - no manual handlers are permitted.

```rust
use crate::common::js_ts_shared::JavaScript;
use crate::define_handler;
use codesearch_core::Visibility;

// Basic handler with default metadata and no relationships
define_handler!(JavaScript, handle_let_impl, "let", Variable);

// Handler with custom metadata function
define_handler!(JavaScript, handle_function_impl, "function", Function,
    metadata: function_metadata);

// Handler with custom relationships function
define_handler!(JavaScript, handle_class_impl, "class", Class,
    relationships: extract_extends_relationships);

// Handler with both custom metadata and relationships
define_handler!(JavaScript, handle_method_impl, "method", Method,
    metadata: method_metadata,
    relationships: extract_implements);

// Handler with visibility override (for interface members that are always Public)
define_handler!(TypeScript, handle_interface_property_impl, "interface_property", Property,
    visibility: Visibility::Public);

// Handler with static name and visibility (for call/construct signatures)
define_handler!(TypeScript, handle_call_signature_impl, "call_signature", Method,
    name: "()",
    visibility: Visibility::Public);

// Handler with name derivation function and visibility
define_handler!(TypeScript, handle_index_signature_impl, "index_signature", Property,
    name_fn: derive_index_signature_name,
    visibility: Visibility::Public);

// Handler with context-aware name function (for complex name resolution)
define_handler!(JavaScript, handle_function_expression_impl, "function", Function,
    name_ctx_fn: derive_function_expression_name,
    metadata: function_metadata);

// Module handler with file-path-based name derivation
define_handler!(JavaScript, handle_module_impl, "program",
    module_name_fn: derive_module_name_from_ctx);
```

The macro parameters:
- `$lang:ty` - Language struct implementing `LanguageExtractors`
- `$fn_name:ident` - Handler function name
- `$capture:expr` - Tree-sitter capture name for the main node
- `$entity_type:ident` - `EntityType` variant (e.g., `Function`, `Class`)
- `metadata: $fn` - (optional) Function `fn(Node, &str) -> EntityMetadata`
- `relationships: $fn` - (optional) Function `fn(&ExtractionContext, Node) -> EntityRelationshipData`
- `visibility: $expr` - (optional) Static visibility override (e.g., `Visibility::Public`)
- `name: $expr` - (optional) Static name string (e.g., `"()"`, `"new()"`)
- `name_fn: $fn` - (optional) Name derivation function `fn(Node, &str) -> String`
- `name_ctx_fn: $fn` - (optional) Context-aware name function `fn(&ExtractionContext, Node) -> Result<String>`
- `module_name_fn: $fn` - (for module entities) Module name derivation from file path

### 3.4 Helper Functions for Metadata and Relationships

Define helper functions for common metadata and relationship patterns:

```rust
// Metadata helper (JS/TS example)
pub(crate) fn function_metadata(node: Node, source: &str) -> EntityMetadata {
    EntityMetadata {
        is_async: is_async(node),
        is_generator: is_generator(node),
        ..Default::default()
    }
}

// Relationships helper
pub(crate) fn extract_extends_relationships(
    ctx: &ExtractionContext,
    node: Node,
) -> EntityRelationshipData {
    let extends = extract_class_extends(node, ctx.source);
    EntityRelationshipData {
        extends,
        ..Default::default()
    }
}
```

### 3.5 Helper Functions for Name Derivation

For entities requiring custom name derivation, use the appropriate macro parameter:

```rust
// For static names (call/construct signatures)
// Use `name:` parameter directly in the macro

// For names derived from AST (e.g., index signature type)
pub(crate) fn derive_index_signature_name(node: Node, source: &str) -> String {
    // Extract type from index signature: [key: string] -> "[string]"
    find_first_type_in_node(node, source)
        .map(|t| format!("[{t}]"))
        .unwrap_or_else(|| "[index]".to_string())
}

// For names requiring context (e.g., file path or multiple captures)
pub(crate) fn derive_function_expression_name(
    ctx: &ExtractionContext,
    _node: Node,
) -> Result<String> {
    // Prefer function's own name over variable name
    find_capture_node(ctx.query_match, ctx.query, "fn_name")
        .or_else(|| find_capture_node(ctx.query_match, ctx.query, "name"))
        .and_then(|n| node_to_text(n, ctx.source).ok())
        .ok_or_else(|| Error::entity_extraction("Could not derive name"))
}

// For module entities (name from file path)
pub(crate) fn derive_module_name_from_ctx(
    ctx: &ExtractionContext,
    _node: Node,
) -> Result<String> {
    Ok(module_utils::derive_module_name(ctx.file_path))
}
```

> **Note:** The `define_handler!` macro now supports all extraction patterns through its various parameters. Manual handlers are not needed.

---

## Step 4: Relationship Data Extraction

Relationship data is stored in typed `EntityRelationshipData` fields.

### 4.1 Using SourceReference

```rust
use codesearch_core::entities::{
    EntityRelationshipData, SourceReference, SourceLocation, ReferenceType
};

// Create a SourceReference for a function call
let call_ref = SourceReference::new(
    "crate::utils::process",    // target (qualified name)
    "process",                   // simple_name
    false,                       // is_external
    SourceLocation { start_line: 10, end_line: 10, start_column: 4, end_column: 11 },
    ReferenceType::Call,
);

// Build EntityRelationshipData
let relationships = EntityRelationshipData {
    calls: vec![call_ref],
    uses_types: extract_type_references(params, return_type, &import_map),
    ..Default::default()
};
```

### 4.2 Determining is_external

The `is_external` flag indicates whether a reference targets code outside the repository. Use `LanguagePath` to determine externality based on path prefixes (e.g., Rust: `std::/core::/alloc::` are external, `crate::/self::/super::` are internal).

### 4.3 Relationship Field Usage

| Field | Entity Types | Relationship |
|-------|--------------|--------------|
| `calls` | Function, Method | CALLS |
| `uses_types` | Function, Method, Struct, Enum, etc. | USES |
| `imports` | Module, Function, etc. | IMPORTS |
| `implements_trait` | Impl | IMPLEMENTS |
| `for_type` | Impl | ASSOCIATES |
| `extends` | Class | INHERITS_FROM |
| `extended_types` | Trait, Interface | EXTENDS_INTERFACE |
| `call_aliases` | Method | (UFCS resolution) |

---

## Step 5: Import Resolution

Add a language-specific import parser in `common/import_map.rs`:

```rust
pub fn parse_file_imports(root: Node, source: &str, language: Language) -> ImportMap {
    match language {
        Language::Rust => parse_rust_imports(root, source),
        Language::JavaScript => parse_js_imports(root, source),
        // Add your language here
        _ => ImportMap::new("."),
    }
}
```

Use `resolve_reference()` to resolve names against the import map, returning a `ResolvedReference` with `target`, `simple_name`, and `is_external`.

---

## Testing

Language implementations require two levels of testing. The table below clarifies when to use each:

| Aspect | Handler Unit Tests | E2E Spec Validation Tests |
|--------|-------------------|---------------------------|
| **Location** | `crates/languages/src/{lang}/handler_impls/tests/` | `crates/e2e-tests/tests/spec_validation/{lang}/` |
| **Checklist** | Item 14 | Items 15-18 |
| **Infrastructure** | None (pure Rust unit tests) | Docker (Postgres, Neo4j, Qdrant) |
| **Speed** | Fast (~ms) | Slow (~seconds per test) |
| **Scope** | Single handler correctness | Full pipeline: parse → extract → resolve → graph |
| **Run command** | `cargo test -p codesearch-languages` | `cargo test --manifest-path crates/e2e-tests/Cargo.toml -- --ignored` |

**When to use each:**
- **Handler unit tests**: Write these first when developing handlers. Test that individual handlers correctly extract entities and populate relationship data from source code.
- **E2E spec validation tests**: Write these to validate that spec rules are correctly implemented end-to-end, including graph resolution and Neo4j storage.

---

### Handler Unit Tests (`crates/languages/src/{lang}/handler_impls/tests/`)

Test that handlers correctly extract entities and relationship data. Reference spec rule IDs in comments:

```rust
/// Tests rule E-FN-FREE: Free functions produce Function entity
#[test]
fn test_function_extracts_calls() {
    let source = r#"
        fn caller() { helper(); }
        fn helper() {}
    "#;
    let entities = extract_entities(source, Language::Rust);
    let caller = entities.iter().find(|e| e.name == "caller").unwrap();
    assert_eq!(caller.relationships.calls[0].simple_name, "helper");
}
```

---

## Entity Identifiers

| Field | Purpose | Format |
|-------|---------|--------|
| `qualified_name` | Graph resolution, LSP-compatible | `package::module::entity` |
| `path_entity_identifier` | Relative import resolution | `path.to.file.entity` |

---

## Resolution Phase (outbox-processor)

The `GenericResolver` handles all relationship types using configuration:

```rust
// In crates/core/src/resolution.rs

pub struct RelationshipDef {
    pub name: &'static str,
    pub source_types: &'static [EntityType],
    pub target_types: &'static [EntityType],
    pub forward_rel: RelationshipType,
    pub lookup_strategies: &'static [LookupStrategy],
}

// Example: CALLS relationship
pub const CALLS: RelationshipDef = RelationshipDef::new(
    "calls",
    CALLABLE_TYPES,           // Function, Method
    CALLABLE_TYPES,
    RelationshipType::Calls,
    &[
        LookupStrategy::QualifiedName,
        LookupStrategy::CallAliases,
        LookupStrategy::UniqueSimpleName,
    ],
);
```

### Lookup Strategies

| Strategy | Description |
|----------|-------------|
| `QualifiedName` | Match by fully qualified name |
| `PathEntityIdentifier` | Match by file-path-based identifier |
| `CallAliases` | Match by pre-computed aliases (Rust UFCS) |
| `UniqueSimpleName` | Match if only one entity has that simple name |
| `SimpleName` | First match wins (logs warning on ambiguity) |

### Adding a New Resolver (rarely needed)

Most relationships use standard `RelationshipDef` definitions in `crates/core/src/resolution.rs`. If custom resolution is needed: add a `RelationshipDef`, create a `ReferenceExtractor`, and register in `processor.rs`. See existing resolvers in `crates/outbox-processor/src/generic_resolver.rs` for examples.

---

### E2E Spec Validation Tests (`crates/e2e-tests/tests/spec_validation/{lang}/`)

E2E tests validate the full pipeline (parse → extract → resolve → graph). Require Docker (Postgres, Neo4j, Qdrant).

```bash
cargo test --manifest-path crates/e2e-tests/Cargo.toml -- --ignored
```

Tests should reference spec rule IDs and use minimal fixtures. See existing language tests for examples.

---

## Checklist

### Specification
1. [ ] Create spec file: `crates/languages/specs/{language}.yaml`
   - Define entity rules (E-xxx)
   - Define visibility rules (V-xxx)
   - Define qualified name rules (Q-xxx)
   - Define relationship rules (R-xxx)
   - Define metadata rules (M-xxx)
   - Map fixtures to rules

### Language Module
2. [ ] Create language directory: `crates/languages/src/{language}/`
3. [ ] Add `mod.rs` with `define_language_extractor!` macro
4. [ ] Create `queries.rs` with tree-sitter queries
5. [ ] **REQUIRED**: Implement `LanguageExtractors` trait for visibility and doc extraction
6. [ ] **REQUIRED**: Create `handler_impls/` with handlers using `define_handler!` macro
   - All handlers MUST use the `define_handler!` macro with `LanguageExtractors`
   - Use `metadata:` and `relationships:` parameters for custom logic
   - Use `visibility:`, `name:`, `name_fn:`, `name_ctx_fn:`, or `module_name_fn:` for special cases
7. [ ] Populate `EntityRelationshipData` fields (not metadata.attributes)
8. [ ] Use `SourceReference` with `is_external` flag
9. [ ] Add import parser in `common/import_map.rs`
10. [ ] Add language to `Language` enum in `crates/core/src/entities.rs`

### Resolver Work (outbox-processor)
11. [ ] Verify existing `RelationshipDef` definitions cover your language's relationships
12. [ ] If needed: Add new `RelationshipDef` in `crates/core/src/resolution.rs`
13. [ ] If needed: Add new `ReferenceExtractor` in `crates/outbox-processor/src/generic_resolver.rs`
14. [ ] If needed: Add factory function and register in `processor.rs`

### Testing
15. [ ] Write handler unit tests in `crates/languages/src/{language}/handler_impls/tests/`
16. [ ] Create E2E spec validation tests in `crates/e2e-tests/tests/{language}_spec_validation.rs`
17. [ ] Create test fixtures in `crates/e2e-tests/tests/fixtures/{language}/`
18. [ ] Maintain spec coverage matrix in test file
19. [ ] Run full E2E test suite: `cargo test --manifest-path crates/e2e-tests/Cargo.toml -- --ignored`

---

## Current Language Support

| Language | Extraction | Resolution | Notes |
|----------|-----------|------------|-------|
| **Rust** | Full | Full | Canonical implementation with spec file |
| **JavaScript** | Full | Full | Complete with typed relationships |
| **TypeScript** | Full | Full | Complete with typed relationships |
| **Python** | Full | Full | Complete with typed relationships |
