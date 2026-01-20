# Language Onboarding Guide

This guide covers adding new language support to codesearch using the spec-driven extraction system.

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Directory Structure](#directory-structure)
3. [Step 1: Create YAML Specification](#step-1-create-yaml-specification)
4. [Step 2: Register the Extractor](#step-2-register-the-extractor)
5. [Step 3: Add Language-Specific Support](#step-3-add-language-specific-support)
6. [Testing](#testing)
7. [Checklist](#checklist)
8. [Troubleshooting](#troubleshooting)

---

## Architecture Overview

The spec-driven extraction system uses declarative YAML specifications to define how entities are extracted from source code:

```
┌────────────────────────────────────────────────────────────────────────────┐
│                           BUILD TIME                                        │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│   specs/{language}.yaml ──► build.rs ──► {language}_generated.rs           │
│   (Declarative rules)        (Code gen)   (Query constants + HandlerConfig)│
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
                                           │
                                           ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                           INDEXING PHASE                                    │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│   Source File ──► Tree-Sitter Parser ──► AST                               │
│                                           │                                │
│                                           ▼                                │
│                        ┌──────────────────────────────────┐                │
│                        │  SpecDriven<Lang>Extractor       │                │
│                        │  - Iterates ALL_HANDLERS         │                │
│                        │  - Runs queries from HandlerConfig│               │
│                        │  - Builds CodeEntity via engine   │               │
│                        │  - Extracts relationships        │                │
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

### Key Components

**YAML Spec File** (`specs/{language}.yaml`): Declarative specification defining:
- Entity rules (what constructs produce which entity types)
- Visibility rules (how visibility is determined)
- Qualified name rules (how FQNs are constructed)
- Relationship rules (what relationships exist)
- Extraction hints (queries, extractors, handlers)

**Build Script** (`build.rs`): Generates Rust code from YAML specs:
- Query constants module
- `HandlerConfig` structs
- `ALL_HANDLERS` array

**HandlerConfig**: Runtime configuration for entity extraction:
```rust
pub struct HandlerConfig {
    pub entity_rule: &'static str,           // Rule ID from spec
    pub query: &'static str,                  // Tree-sitter query
    pub capture: &'static str,                // Primary capture name
    pub name_strategy: NameStrategy,          // How to derive entity name
    pub qualified_name_template: Option<&'static str>,  // FQN template
    pub metadata_extractor: Option<MetadataExtractor>,
    pub relationship_extractor: Option<RelationshipExtractor>,
    pub visibility_override: Option<Visibility>,
    pub parent_scope_template: Option<&'static str>,
}
```

**SpecDriven&lt;Lang&gt;Extractor**: Language-specific extractor that:
1. Parses source with tree-sitter
2. Builds import map for reference resolution
3. Iterates `ALL_HANDLERS` from generated code
4. Calls `extract_with_config()` for each handler
5. Returns collected `CodeEntity` instances

---

## Directory Structure

```
crates/languages/
├── specs/
│   ├── rust.yaml              # Rust specification (canonical example)
│   ├── javascript.yaml        # JavaScript specification
│   └── typescript.yaml        # TypeScript specification
│
├── src/
│   ├── spec_driven/
│   │   ├── mod.rs             # HandlerConfig, NameStrategy, enums
│   │   ├── engine.rs          # extract_with_config() core logic
│   │   ├── extractors.rs      # SpecDriven<Lang>Extractor implementations
│   │   └── relationships.rs   # Relationship extraction functions
│   │
│   ├── common/
│   │   ├── import_map.rs      # Import resolution
│   │   ├── reference_resolution.rs  # Reference resolution
│   │   ├── path_config.rs     # Language path configurations
│   │   └── js_ts_shared/      # Shared JS/TS infrastructure
│   │
│   ├── rust/
│   │   ├── mod.rs             # Rust-specific utilities
│   │   ├── module_path.rs     # Rust module path derivation
│   │   ├── import_resolution.rs  # Rust import/type alias resolution
│   │   └── edge_case_handlers.rs # Rust-specific edge cases
│   │
│   └── qualified_name.rs      # Qualified name building from AST
│
├── build.rs                   # Code generation from YAML specs
│
└── Cargo.toml
```

---

## Step 1: Create YAML Specification

Create `specs/{language}.yaml` with the following sections. See `specs/rust.yaml` for a comprehensive example.

### 1.1 Header and Entity Rules

```yaml
version: "1.0"
language: {language}

entity_rules:
  - id: E-FN-FREE
    description: "A free function produces a Function entity"
    construct: "function name() { ... }"
    produces: Function
    tested_by: [free_functions]

  - id: E-CLASS
    description: "A class declaration produces a Class entity"
    construct: "class Name { ... }"
    produces: Class
    tested_by: [classes]
```

**Rule ID conventions:**
- `E-xxx` = Entity extraction rules
- `V-xxx` = Visibility rules
- `Q-xxx` = Qualified name rules
- `R-xxx` = Relationship rules
- `M-xxx` = Metadata rules

### 1.2 Visibility Rules

```yaml
visibility_rules:
  - id: V-EXPORT
    description: "Exported items are Public"
    applies_to: "*"
    condition: "has export keyword"
    result: Public
    precedence: 10

  - id: V-PRIVATE
    description: "Non-exported items are Private"
    applies_to: "*"
    condition: "no export keyword"
    result: Private
    precedence: 100
```

### 1.3 Qualified Name Rules

```yaml
qualified_name_rules:
  - id: Q-MODULE
    description: "Modules are qualified by file path"
    pattern: "{module_path}"
    example: "src.utils.helpers"

  - id: Q-FUNCTION
    description: "Functions are qualified under their module"
    pattern: "{module}.{name}"
    example: "src.utils.processData"
```

### 1.4 Extraction Hints

The `extraction_hints` section drives code generation:

```yaml
extraction_hints:
  queries:
    FUNCTION_DECLARATION:
      description: "Function declarations"
      capture: "function"
      query: |
        (function_declaration
          name: (identifier) @name
          parameters: (formal_parameters) @params
          body: (statement_block) @body) @function

    CLASS_DECLARATION:
      description: "Class declarations"
      capture: "class"
      query: |
        (class_declaration
          name: (identifier) @name
          body: (class_body) @body) @class

  extractors:
    function_metadata:
      description: "Extract function metadata"
      metadata_fields:
        - is_async: "Check for async keyword"
        - is_generator: "Check for generator (*) syntax"

    extract_function_relationships:
      description: "Extract calls and type usages"
      relationship_fields:
        - calls: "Function/method calls in body"
        - uses_types: "Type references in signature"

  handlers:
    FreeFunction:
      entity_rule: E-FN-FREE
      query: FUNCTION_DECLARATION
      capture: "function"
      name_strategy: capture
      qualified_name_template: "{scope}.{name}"
      metadata: function_metadata
      relationships: extract_function_relationships

    Class:
      entity_rule: E-CLASS
      query: CLASS_DECLARATION
      capture: "class"
      name_strategy: capture
      qualified_name_template: "{scope}.{name}"
      relationships: extract_class_relationships
```

### 1.5 Handler Configuration Options

| Field | Description | Example |
|-------|-------------|---------|
| `entity_rule` | Rule ID from entity_rules | `E-FN-FREE` |
| `query` | Query name from queries section | `FUNCTION_DECLARATION` |
| `capture` | Primary capture in the query | `"function"` |
| `name_strategy` | How to derive entity name | See below |
| `qualified_name_template` | Template for FQN | `"{scope}::{name}"` |
| `metadata` | Metadata extractor name | `function_metadata` |
| `relationships` | Relationship extractor name | `extract_function_relationships` |
| `visibility_override` | Force specific visibility | `Public` or `null` |
| `parent_scope_template` | Override parent derivation | `"{scope}::extern {abi}"` |

**Name strategies:**
- `capture` - Use the `@name` capture directly
- `template` - Use `name_template` with placeholders
- `static` - Use `static_name` value
- `file_path` - Derive from file path (for modules)
- `crate_name` - Use package name (for crate roots)
- `positional_index` - Use position index (for tuple fields)
- `fallback` - Try multiple captures via `name_captures`

### 1.6 Template Placeholders

Templates in `qualified_name_template` and `name_template` support:

| Placeholder | Source | Description |
|-------------|--------|-------------|
| `{scope}` | Engine | Parent scope from AST traversal |
| `{name}` | Engine | Entity name from name_strategy |
| `{crate}` | Context | Package/crate name |
| `{module_path}` | Context | Module path from file |
| `{capture_name}` | Query | Any captured value |

---

## Step 2: Register the Extractor

### 2.1 Add Scope Configuration

In `src/spec_driven/extractors.rs`, register the scope configuration:

```rust
// Define scope patterns for qualified name building
const LANG_SCOPE_PATTERNS: &[ScopePattern] = &[
    ScopePattern {
        node_kind: "class_declaration",
        field_name: "name",
    },
    ScopePattern {
        node_kind: "namespace_declaration",
        field_name: "name",
    },
];

inventory::submit! {
    ScopeConfiguration {
        language: "lang",
        separator: ".",                    // Or "::" for Rust-like
        patterns: LANG_SCOPE_PATTERNS,
        module_path_fn: Some(lang_module_path::derive_module_path),
        path_config: &MODULE_BASED_PATH_CONFIG,  // Or CRATE_BASED_PATH_CONFIG
        edge_case_handlers: None,          // Or Some(LANG_EDGE_CASE_HANDLERS)
        custom_scope_extractor: None,
    }
}
```

### 2.2 Add Language Descriptor

Register the language descriptor:

```rust
inventory::submit! {
    LanguageDescriptor {
        name: "lang",
        extensions: &["ext1", "ext2"],
        factory: create_lang_extractor,
    }
}

fn create_lang_extractor(
    repository_id: &str,
    package_name: Option<&str>,
    source_root: Option<&Path>,
    repo_root: &Path,
) -> Result<Box<dyn Extractor>> {
    Ok(Box::new(SpecDrivenLangExtractor::new(
        repository_id.to_string(),
        package_name.map(String::from),
        source_root.map(PathBuf::from),
        repo_root.to_path_buf(),
    )?))
}
```

### 2.3 Create the Extractor

```rust
pub struct SpecDrivenLangExtractor {
    repository_id: String,
    package_name: Option<String>,
    source_root: Option<PathBuf>,
    repo_root: PathBuf,
}

impl SpecDrivenLangExtractor {
    pub fn new(
        repository_id: String,
        package_name: Option<String>,
        source_root: Option<PathBuf>,
        repo_root: PathBuf,
    ) -> Result<Self> {
        Ok(Self { repository_id, package_name, source_root, repo_root })
    }
}

impl Extractor for SpecDrivenLangExtractor {
    fn extract(&self, source: &str, file_path: &Path) -> Result<Vec<CodeEntity>> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_lang::LANGUAGE.into())
            .map_err(|e| anyhow::anyhow!("Failed to set language: {e}"))?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse source code"))?;

        // Build import map for reference resolution
        let import_map = parse_file_imports(
            tree.root_node(),
            source,
            Language::Lang,
            None,
        );

        // Empty type alias map (unless language has type aliases)
        let type_alias_map = TypeAliasMap::new();

        let ctx = SpecDrivenContext {
            source,
            file_path,
            repository_id: &self.repository_id,
            package_name: self.package_name.as_deref(),
            source_root: self.source_root.as_deref(),
            repo_root: &self.repo_root,
            language: Language::Lang,
            language_str: "lang",
            import_map: &import_map,
            path_config: &MODULE_BASED_PATH_CONFIG,
            edge_case_handlers: None,
            type_alias_map: &type_alias_map,
        };

        // Use generated handlers
        use super::lang::handler_configs::ALL_HANDLERS;

        let mut all_entities = Vec::new();
        for config in ALL_HANDLERS {
            let entities = extract_with_config(config, &ctx, tree.root_node())?;
            all_entities.extend(entities);
        }

        Ok(all_entities)
    }
}
```

### 2.4 Include Generated Code

In `src/spec_driven/mod.rs`, add:

```rust
pub mod lang {
    include!(concat!(env!("OUT_DIR"), "/lang_generated.rs"));
}
```

---

## Step 3: Add Language-Specific Support

### 3.1 Add Language to Core Enum

In `crates/core/src/entities.rs`:

```rust
pub enum Language {
    // ...existing languages...
    Lang,
}
```

### 3.2 Add Import Parser (if needed)

In `src/common/import_map.rs`, add a case to `parse_file_imports`:

```rust
pub fn parse_file_imports(
    root: Node,
    source: &str,
    language: Language,
    module_path: Option<&str>,
) -> ImportMap {
    match language {
        Language::Lang => parse_lang_imports(root, source, module_path),
        // ...
    }
}
```

### 3.3 Add Visibility Extraction (if needed)

The engine's `extract_visibility_from_node` in `engine.rs` handles visibility extraction. Add language-specific logic if needed:

```rust
fn extract_visibility_from_node(node: Node, source: &str, language: &str) -> Option<Visibility> {
    // ...existing code...

    if language == "lang" {
        if let Some(vis) = extract_lang_visibility(node, source) {
            return Some(vis);
        }
    }

    None
}
```

### 3.4 Add Relationship Queries (if needed)

In `src/spec_driven/relationships.rs`, add query constants and lazy compilation:

```rust
const LANG_CALL_QUERY: &str = r#"
[
  (call_expression
    function: (identifier) @callee)
]
"#;

fn get_lang_call_query() -> Option<&'static Query> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    QUERY
        .get_or_init(|| {
            let language = tree_sitter_lang::LANGUAGE.into();
            Query::new(&language, LANG_CALL_QUERY).ok()
        })
        .as_ref()
}
```

Update extraction functions to handle the new language:

```rust
pub fn extract_function_calls(node: Node, ctx: &SpecDrivenContext, parent_scope: Option<&str>) -> Vec<SourceReference> {
    let query = match ctx.language_str {
        "lang" => get_lang_call_query(),
        // ...existing languages...
    };
    // ...
}
```

---

## Testing

### Unit Tests

Handler unit tests verify individual handler correctness:

```bash
cargo test -p codesearch-languages
```

### E2E Spec Validation Tests

E2E tests validate the full pipeline. Located in `crates/e2e-tests/tests/spec_validation/{lang}/`.

```bash
# Run all E2E tests for a language
cargo test --manifest-path crates/e2e-tests/Cargo.toml --test spec_validation {lang}:: -- --ignored

# Run specific test
cargo test --manifest-path crates/e2e-tests/Cargo.toml --test spec_validation {lang}::test_free_functions -- --ignored
```

### Testing Workflow

| Phase | Test Type | Purpose |
|-------|-----------|---------|
| Query Development | Tree-sitter playground | Verify queries match expected AST patterns |
| Handler Development | Unit tests | Verify entity extraction correctness |
| Integration | E2E tests | Verify full pipeline including relationships |

---

## Checklist

### Phase 1: Specification
- [ ] Create `specs/{language}.yaml`
- [ ] Define entity rules for all supported constructs
- [ ] Define visibility rules
- [ ] Define qualified name rules
- [ ] Add extraction hints (queries, extractors, handlers)

### Phase 2: Registration
- [ ] Add `Language::Lang` to core enum
- [ ] Register `ScopeConfiguration` in `extractors.rs`
- [ ] Register `LanguageDescriptor` in `extractors.rs`
- [ ] Create `SpecDriven<Lang>Extractor` in `extractors.rs`
- [ ] Include generated module in `mod.rs`

### Phase 3: Language Support
- [ ] Add tree-sitter dependency to `Cargo.toml`
- [ ] Implement import parser (if language has imports)
- [ ] Add relationship queries in `relationships.rs`
- [ ] Add edge case handlers (if needed)

### Phase 4: Testing
- [ ] Create E2E test fixtures in `crates/e2e-tests/tests/spec_validation/{lang}/`
- [ ] Verify entity extraction for all types
- [ ] Verify relationship extraction (calls, uses, imports)
- [ ] Run full test suite

### Phase 5: Documentation
- [ ] Update "Current Language Support" table
- [ ] Document any language-specific behaviors

---

## Troubleshooting

### Query doesn't match expected nodes

1. Test query in [tree-sitter playground](https://tree-sitter.github.io/tree-sitter/playground)
2. Verify capture names match handler configuration
3. Check for missing `?` on optional captures

### Entity not being extracted

1. Verify handler is in `handlers:` section of YAML spec
2. Check that `entity_rule` maps to a valid `EntityType` in `engine.rs`
3. Enable tracing to see skipped matches: `RUST_LOG=codesearch_languages=trace`

### Wrong qualified name

1. Check scope patterns in `ScopeConfiguration`
2. Verify `qualified_name_template` placeholders
3. Test with simple cases first

### Relationships not appearing

1. Verify `relationship_extractor` is specified in handler config
2. Check that relationship queries compile for the language
3. Ensure `RelationshipExtractor` variant exists in `mod.rs`

### Build errors from generated code

1. Check YAML syntax (use a YAML validator)
2. Verify all referenced queries exist in `queries:` section
3. Check `name_strategy` matches expected format

---

## Current Language Support

| Language | Extraction | Resolution | Notes |
|----------|-----------|------------|-------|
| **Rust** | Full | Full | Canonical implementation |
| **JavaScript** | Full | Partial | Entity extraction complete |
| **TypeScript** | Full | Partial | Entity extraction complete |
| **TSX** | Full | Partial | Uses TypeScript handlers |
