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

### Spec File Structure

```yaml
version: "1.0"
language: rust

# Entity extraction rules
entity_rules:
  - id: E-FN-FREE
    description: "A free function produces a Function entity"
    construct: "fn name() { ... }"
    produces: Function
    tested_by: [free_functions, visibility]

# Visibility rules (precedence-ordered)
visibility_rules:
  - id: V-PUB
    description: "pub modifier results in Public visibility"
    applies_to: "*"
    result: Public
    precedence: 10

# Qualified name rules
qualified_name_rules:
  - id: Q-ITEM
    description: "Top-level items are qualified under their module path"
    pattern: "{module}::{name}"
    applies_to: [Function, Struct, Enum, Trait]

# Relationship rules
relationship_rules:
  - id: R-CALLS-FUNCTION
    description: "Function/Method CALLS another function/method"
    kind: Calls
    from: [Function, Method]
    to: [Function, Method]

# Metadata rules
metadata_rules:
  - id: M-FN-ASYNC
    description: "Async functions have is_async=true"
    applies_to: [Function, Method]
    field: is_async

# Test fixture mapping
fixtures:
  free_functions:
    tests: [E-FN-FREE, V-PUB, Q-ITEM, R-CALLS-FUNCTION]
```

### Rule ID Conventions

| Prefix | Category |
|--------|----------|
| E-xxx | Entity extraction |
| V-xxx | Visibility |
| Q-xxx | Qualified names |
| R-xxx | Relationships |
| M-xxx | Metadata |

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

The extraction framework provides two approaches for implementing handlers:
1. **Macro-based** (recommended): Use `define_handler!` with the `LanguageExtractors` trait for concise definitions
2. **Manual**: Write handlers directly for complex cases requiring custom logic

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

### 3.3 Using the define_handler! Macro (Recommended)

The `define_handler!` macro generates handlers using the trait-based extraction framework:

```rust
use crate::common::js_ts_shared::JavaScript;
use crate::define_handler;

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
```

The macro parameters:
- `$lang:ty` - Language struct implementing `LanguageExtractors`
- `$fn_name:ident` - Handler function name
- `$capture:expr` - Tree-sitter capture name for the main node
- `$entity_type:ident` - `EntityType` variant (e.g., `Function`, `Class`)
- `metadata: $fn` - (optional) Function `fn(Node, &str) -> EntityMetadata`
- `relationships: $fn` - (optional) Function `fn(&ExtractionContext, Node) -> EntityRelationshipData`

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

### 3.5 Manual Handler Pattern

For complex cases requiring custom logic (e.g., TypeScript enums with const detection):

```rust
pub(crate) fn handle_enum_impl(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let node = match extract_main_node(ctx.query_match, ctx.query, &["enum"]) {
        Some(n) => n,
        None => return Ok(Vec::new()),
    };

    let components = extract_common_components(ctx, "name", node, "typescript")?;
    let visibility = extract_visibility(node, ctx.source);
    let documentation = extract_preceding_doc_comments(node, ctx.source);
    let content = node_to_text(node, ctx.source).ok();

    // Custom logic: detect const enums
    let is_const = node.child_by_field_name("const").is_some()
        || ctx.source[node.byte_range()].trim_start().starts_with("const");

    let metadata = EntityMetadata {
        is_const,
        ..Default::default()
    };

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Enum,
            language: Language::TypeScript,
            visibility: Some(visibility),
            documentation,
            content,
            metadata,
            signature: None,
            relationships: Default::default(),
        },
    )?;

    Ok(vec![entity])
}

---

## Step 4: Relationship Data Extraction

**Key change from legacy approach**: Relationship data is now stored in typed `EntityRelationshipData` fields, not as JSON strings in `metadata.attributes`.

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

The `is_external` flag indicates whether a reference targets code outside the repository:

```rust
use crate::common::language_path::LanguagePath;
use crate::common::path_config::RUST_PATH_CONFIG;

fn create_source_reference(
    resolved: &ResolvedReference,
    ref_type: ReferenceType,
) -> SourceReference {
    SourceReference::new(
        resolved.target.clone(),
        resolved.simple_name.clone(),
        resolved.is_external,  // Set at resolution time
        SourceLocation::default(),
        ref_type,
    )
}

// For Rust, use LanguagePath to parse and determine externality
let lang_path = LanguagePath::parse(&import_path, &RUST_PATH_CONFIG);
// is_relative() checks for crate::/self::/super:: prefixes (internal references)
// is_external() checks for known stdlib prefixes (std::/core::/alloc::)
let is_external = !lang_path.is_relative() && lang_path.is_external();
```

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

### 5.1 Import Map

```rust
use crate::common::import_map::{parse_file_imports, resolve_reference};

// Parse imports from file
let import_map = parse_file_imports(root_node, source, Language::Rust);

// Resolve a reference
let resolved = resolve_reference(
    "HashMap",              // Name to resolve
    &import_map,            // Import map
    Some("crate::utils"),   // Parent scope
    "::",                   // Separator
);
// Returns ResolvedReference with target, simple_name, is_external
```

### 5.2 ResolvedReference

```rust
pub struct ResolvedReference {
    pub target: String,       // Fully qualified name
    pub simple_name: String,  // Last path segment
    pub is_external: bool,    // Outside repository
}
```

### 5.3 Adding Language Support

```rust
// In common/import_map.rs

pub fn parse_file_imports(
    root: Node,
    source: &str,
    language: Language,
) -> ImportMap {
    match language {
        Language::Rust => parse_rust_imports(root, source),
        Language::JavaScript => parse_js_imports(root, source),
        Language::NewLang => parse_newlang_imports(root, source),
        _ => ImportMap::new("."),
    }
}
```

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

Handler unit tests verify that individual extraction handlers correctly parse source code and produce the expected entities and relationship data. These run without external infrastructure.

```rust
#[test]
fn test_function_extracts_calls() {
    let source = r#"
        fn caller() {
            helper();
        }

        fn helper() {}
    "#;

    let entities = extract_entities(source, Language::Rust);
    let caller = entities.iter().find(|e| e.name == "caller").unwrap();

    assert_eq!(caller.relationships.calls.len(), 1);
    assert_eq!(caller.relationships.calls[0].simple_name, "helper");
}

#[test]
fn test_impl_extracts_implements_trait() {
    let source = r#"
        trait Display {}
        struct Foo;
        impl Display for Foo {}
    "#;

    let entities = extract_entities(source, Language::Rust);
    let impl_entity = entities.iter()
        .find(|e| e.entity_type == EntityType::Impl)
        .unwrap();

    let implements = impl_entity.relationships.implements_trait.as_ref().unwrap();
    assert!(implements.target.contains("Display"));
}
```

Tests should reference spec rule IDs in comments:

```rust
/// Tests rule E-FN-FREE: Free functions produce Function entity
#[test]
fn test_free_function_extraction() {
    // ...
}
```

---

## Entity Identifiers

Each entity has two identifier fields:

| Field | Purpose | Example |
|-------|---------|---------|
| `qualified_name` | Semantic, package-relative | `mypackage::utils::format` |
| `path_entity_identifier` | File-path-based | `packages.mypackage.src.utils.format` |

### qualified_name (Semantic)
- Used for graph edge resolution
- Matches LSP go-to-definition results
- Format: `package_name + module_path + scope + entity_name`

### path_entity_identifier (File-based)
- Used for resolving relative imports
- Always repo-relative
- Format: `repo_relative_path + scope + entity_name`

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

### Adding a New Resolver (if needed)

Most relationships use the standard `RelationshipDef` definitions in `crates/core/src/resolution.rs`. If your language needs custom resolution logic:

1. **Add a new `RelationshipDef`** in `crates/core/src/resolution.rs`:

```rust
/// CUSTOM relationship for NewLang-specific behavior
pub const CUSTOM_REL: RelationshipDef = RelationshipDef::new(
    "custom",
    &[EntityType::Function],           // Source types
    &[EntityType::Class],              // Target types
    RelationshipType::Uses,            // Relationship type
    &[
        LookupStrategy::QualifiedName,
        LookupStrategy::SimpleName,
    ],
);
```

2. **Create a ReferenceExtractor** in `crates/outbox-processor/src/generic_resolver.rs`:

```rust
/// Extractor for custom NewLang relationships
pub struct CustomExtractor;

impl ReferenceExtractor for CustomExtractor {
    fn extract_refs(&self, entity: &CodeEntity) -> Vec<ExtractedRef> {
        // Extract from entity.relationships fields
        entity.relationships.some_field
            .iter()
            .map(|src_ref| ExtractedRef {
                target: src_ref.target.clone(),
                simple_name: src_ref.simple_name.clone(),
            })
            .collect()
    }
}
```

3. **Add factory function** in `generic_resolver.rs`:

```rust
pub fn custom_resolver() -> GenericResolver {
    GenericResolver::new(
        &codesearch_core::resolution::definitions::CUSTOM_REL,
        Box::new(CustomExtractor),
    )
}
```

4. **Register in processor** in `crates/outbox-processor/src/processor.rs`:

```rust
// In resolve_relationships_for_repository()
let resolvers: Vec<Box<dyn RelationshipResolver>> = vec![
    // ... existing resolvers
    Box::new(custom_resolver()),
];
```

---

### E2E Spec Validation Tests (`crates/e2e-tests/tests/spec_validation/{lang}/`)

E2E spec validation tests run the full pipeline against test fixtures and validate that the resulting graph matches the expected entities and relationships defined in the language spec. These require Docker infrastructure (Postgres, Neo4j, Qdrant).

#### Test Structure

```
crates/e2e-tests/tests/spec_validation/
├── main.rs                           # Test orchestration
├── rust/
│   ├── mod.rs                        # Rust test functions
│   └── fixtures/                     # Rust-specific fixtures
│       ├── mod.rs
│       ├── modules.rs
│       ├── functions.rs
│       └── ...
├── typescript/
│   ├── mod.rs                        # TypeScript test functions
│   └── fixtures/                     # TypeScript-specific fixtures
│       ├── mod.rs
│       ├── modules.rs
│       ├── classes.rs
│       └── ...
└── common/                           # Shared test utilities (if needed)
```

### Writing Spec Validation Tests

Tests should validate each rule in the spec file:

```rust
// crates/e2e-tests/tests/rust_spec_validation.rs

use codesearch_languages::extract_entities;
use codesearch_core::entities::{EntityType, Language, Visibility};

/// Fixture: free_functions
/// Tests: E-FN-FREE, V-PUB, Q-ITEM, R-CALLS-FUNCTION
mod free_functions {
    use super::*;

    const FIXTURE: &str = include_str!("fixtures/rust/free_functions.rs");

    #[test]
    fn e_fn_free_produces_function_entity() {
        // Rule E-FN-FREE: A free function produces a Function entity
        let entities = extract_entities(FIXTURE, Language::Rust).unwrap();
        let func = entities.iter().find(|e| e.name == "my_function").unwrap();

        assert_eq!(func.entity_type, EntityType::Function);
    }

    #[test]
    fn v_pub_results_in_public_visibility() {
        // Rule V-PUB: pub modifier results in Public visibility
        let entities = extract_entities(FIXTURE, Language::Rust).unwrap();
        let func = entities.iter().find(|e| e.name == "public_function").unwrap();

        assert_eq!(func.visibility, Some(Visibility::Public));
    }

    #[test]
    fn q_item_qualified_under_module() {
        // Rule Q-ITEM: Top-level items are qualified under module path
        let entities = extract_entities(FIXTURE, Language::Rust).unwrap();
        let func = entities.iter().find(|e| e.name == "my_function").unwrap();

        assert!(func.qualified_name.ends_with("::my_function"));
    }

    #[test]
    fn r_calls_function_extracts_calls() {
        // Rule R-CALLS-FUNCTION: Function CALLS another function
        let entities = extract_entities(FIXTURE, Language::Rust).unwrap();
        let caller = entities.iter().find(|e| e.name == "caller").unwrap();

        assert!(!caller.relationships.calls.is_empty());
        assert!(caller.relationships.calls.iter()
            .any(|c| c.simple_name == "helper"));
    }
}

/// Fixture: trait_impl
/// Tests: E-IMPL-TRAIT, R-IMPLEMENTS, Q-IMPL-TRAIT
mod trait_impl {
    use super::*;

    const FIXTURE: &str = include_str!("fixtures/rust/trait_impl.rs");

    #[test]
    fn e_impl_trait_produces_impl_entity() {
        // Rule E-IMPL-TRAIT: A trait impl block produces an ImplBlock entity
        let entities = extract_entities(FIXTURE, Language::Rust).unwrap();
        let impl_entity = entities.iter()
            .find(|e| e.entity_type == EntityType::Impl)
            .unwrap();

        assert!(impl_entity.relationships.implements_trait.is_some());
    }

    #[test]
    fn r_implements_links_to_trait() {
        // Rule R-IMPLEMENTS: Trait impl block IMPLEMENTS the trait
        let entities = extract_entities(FIXTURE, Language::Rust).unwrap();
        let impl_entity = entities.iter()
            .find(|e| e.entity_type == EntityType::Impl)
            .unwrap();

        let implements = impl_entity.relationships.implements_trait.as_ref().unwrap();
        assert!(implements.target.contains("Display"));
    }
}
```

### Running E2E Tests

```bash
# Run all e2e tests (requires Docker for infrastructure)
cargo test --manifest-path crates/e2e-tests/Cargo.toml -- --ignored

# Run specific language validation
cargo test --manifest-path crates/e2e-tests/Cargo.toml rust_spec_validation -- --ignored
```

### Fixture File Requirements

Each fixture should be minimal but complete for testing specific rules:

```rust
// fixtures/rust/free_functions.rs

/// A public free function
pub fn public_function() {
    helper();
}

/// A private free function
fn private_function() {}

/// Helper function for call testing
fn helper() {}

/// Caller function for R-CALLS-FUNCTION
fn caller() {
    helper();
    private_function();
}
```

### Spec Coverage Matrix

Maintain a coverage matrix in each test file:

```rust
// Spec coverage for rust_spec_validation.rs
//
// | Rule ID | Description | Test | Status |
// |---------|-------------|------|--------|
// | E-FN-FREE | Free function → Function | e_fn_free_produces_function_entity | ✓ |
// | E-METHOD-SELF | Self param → Method | e_method_self_produces_method | ✓ |
// | V-PUB | pub → Public | v_pub_results_in_public_visibility | ✓ |
// | V-PRIVATE | no modifier → Private | v_private_default | ✓ |
// | Q-ITEM | Module::name format | q_item_qualified_under_module | ✓ |
// | R-CALLS | CALLS relationship | r_calls_function_extracts_calls | ✓ |
// | R-IMPLEMENTS | IMPLEMENTS relationship | r_implements_links_to_trait | ✓ |
```

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
5. [ ] Implement `LanguageExtractors` trait for visibility and doc extraction
6. [ ] Create `handler_impls/` with handlers using `define_handler!` macro
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
