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
- `supertraits: Vec<SourceReference>` - Trait supertraits
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
│   │   ├── rust_path.rs            # Rust path parsing utilities
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
│   │   ├── import_map.rs           # Import resolution
│   │   └── entity_building.rs      # Entity construction
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
pub mod rust_path;
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

### 3.1 Handler Signature

```rust
pub fn handle_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>>
```

### 3.2 Basic Handler Pattern

```rust
pub fn handle_function_impl(
    query_match: &QueryMatch,
    query: &Query,
    source: &str,
    file_path: &Path,
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Vec<CodeEntity>> {
    let function_node = require_capture_node(query_match, query, "function")?;
    let name_node = require_capture_node(query_match, query, "name")?;

    let ctx = ExtractionContext {
        query_match, query, source, file_path,
        repository_id, package_name, source_root, repo_root,
    };

    let components = extract_common_components(&ctx, "name", function_node, "rust")?;

    // Parse imports for reference resolution
    let import_map = parse_file_imports(function_node.parent().unwrap(), source, Language::Rust);

    // Extract relationship data (see Step 4)
    let relationships = extract_function_relationships(
        query_match, query, source, &import_map
    )?;

    let entity = build_entity(
        components,
        EntityDetails {
            entity_type: EntityType::Function,
            language: Language::Rust,
            visibility: extract_visibility(query_match, query),
            relationships,  // Typed EntityRelationshipData
            // ...
        },
    )?;

    Ok(vec![entity])
}
```

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
use crate::rust::rust_path::RustPath;

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

// For Rust, use RustPath to parse and determine externality
let rust_path = RustPath::parse(&import_path);
let is_external = !rust_path.is_relative();  // External if not crate-relative
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
| `supertraits` | Trait | EXTENDS_INTERFACE |
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
5. [ ] Create `handler_impls/` with handlers
6. [ ] Populate `EntityRelationshipData` fields (not metadata.attributes)
7. [ ] Use `SourceReference` with `is_external` flag
8. [ ] Add import parser in `common/import_map.rs`
9. [ ] Add language to `Language` enum in `crates/core/src/entities.rs`

### Resolver Work (outbox-processor)
10. [ ] Verify existing `RelationshipDef` definitions cover your language's relationships
11. [ ] If needed: Add new `RelationshipDef` in `crates/core/src/resolution.rs`
12. [ ] If needed: Add new `ReferenceExtractor` in `crates/outbox-processor/src/generic_resolver.rs`
13. [ ] If needed: Add factory function and register in `processor.rs`

### Testing
14. [ ] Write handler unit tests in `crates/languages/src/{language}/handler_impls/tests/`
15. [ ] Create E2E spec validation tests in `crates/e2e-tests/tests/{language}_spec_validation.rs`
16. [ ] Create test fixtures in `crates/e2e-tests/tests/fixtures/{language}/`
17. [ ] Maintain spec coverage matrix in test file
18. [ ] Run full E2E test suite: `cargo test --manifest-path crates/e2e-tests/Cargo.toml -- --ignored`

---

## Current Language Support

| Language | Extraction | Resolution | Notes |
|----------|-----------|------------|-------|
| **Rust** | Full | Full | Canonical implementation with spec file |
| **JavaScript** | Full | Full | Complete with typed relationships |
| **TypeScript** | Full | Full | Complete with typed relationships |
| **Python** | Full | Full | Complete with typed relationships |
