# Design Exploration: Entity Handler Architecture Refactor

**Issue:** #191
**Branch:** `refactor--191-trait-proc-macro`

## Problem Statement

The current `define_handler!` macro has 16 variants, and the Rust handlers are ~200 lines each of verbose, repetitive code. We need a unified architecture that:
1. Reduces boilerplate for all languages
2. Handles simple cases (JS) and complex cases (Rust) elegantly
3. Is driven by the language specifications rather than ad-hoc implementation

## Key Insight: The Specs Are Already a DSL

The YAML specs (`crates/languages/specs/*.yaml`) define extraction rules declaratively:
- **Entity Rules** (E-xxx): AST pattern → entity type
- **Visibility Rules** (V-xxx): Precedence-based visibility determination
- **Qualified Name Rules** (Q-xxx): Patterns like `{module}::{name}`
- **Relationship Rules** (R-xxx): What relationships to extract
- **Metadata Rules** (M-xxx): What metadata fields to capture

The question: **Should we make the specs executable?**

---

## Architecture Options

### Option A: Spec-Driven Code Generation (CHOSEN)

Parse the YAML specs at build time and generate handler code.

```yaml
# rust.yaml (already exists)
entity_rules:
  - id: E-FN-FREE
    construct: "fn name() { ... }"
    produces: Function
```

**Pros:**
- Single source of truth (spec = implementation)
- Automatic consistency between spec and code
- Easy to add new languages

**Cons:**
- Build complexity
- Debugging generated code is harder
- Rust-specific complexity (generics, imports) may not fit YAML well

### Option B: Trait-Based DSL with Proc-Macro

Define handlers using Rust traits + derive macros, with the architecture matching the spec structure.

```rust
#[derive(EntityHandler)]
#[entity(Rust, Function)]
#[capture("function")]
#[visibility(from_spec)]  // Uses spec rules V-PUB, V-PUB-CRATE, etc.
#[qualified_name("{module}::{name}")]
pub struct RustFunctionHandler;

impl RustFunctionHandler {
    fn metadata(ctx: &HandlerContext) -> EntityMetadata { ... }
    fn relationships(ctx: &HandlerContext) -> EntityRelationshipData { ... }
}
```

**Pros:**
- Full Rust power for complex cases
- IDE support for trait methods
- Gradual migration possible

**Cons:**
- Still need to manually sync with spec
- Two places to update (spec + code)

### Option C: Hybrid - Declarative DSL + Escape Hatches

A Rust macro DSL that mirrors spec structure, with escape hatches for complexity.

```rust
entity_handler! {
    language: Rust,
    entity_type: Function,
    capture: "function",

    // Visibility uses spec rules (parsed at compile time)
    visibility: spec_rules![V-IMPL-BLOCK, V-PUB, V-PUB-CRATE, V-PRIVATE],

    // Qualified name from spec pattern
    qualified_name: "{module}::{name}",

    // Simple metadata - declarative
    metadata: {
        is_async: bool_capture("async"),
        is_const: bool_capture("const"),
    },

    // Complex metadata - escape to function
    metadata_fn: extract_rust_function_metadata,

    // Relationships - declarative for simple cases
    relationships: {
        calls: extract_calls_from("body"),
        uses_types: extract_type_refs_from("return_type", "params"),
    },
}
```

**Pros:**
- Declarative for simple cases (most JS/TS handlers)
- Escape hatch for complex cases (Rust generics, import resolution)
- Structure matches spec

**Cons:**
- New macro syntax to learn
- May still be complex for Rust's edge cases

---

## What Does Each Language Actually Need?

### JavaScript (simplest)
- Entity type from capture name
- Visibility: `export` keyword → Public, else Private
- Qualified name: `{file_path}.{name}`
- Metadata: `is_async`, `is_generator`, `is_arrow`
- Relationships: calls, imports, reexports

**Conclusion:** Can be fully declarative

### TypeScript (medium)
- All of JavaScript, plus:
- Interface, TypeAlias, Enum, EnumVariant entity types
- Generic params and constraints
- Type usage relationships (R-USES-TYPE)

**Conclusion:** Mostly declarative, may need escape hatch for generics

### Rust (complex)
- Qualified names require import resolution (use statements)
- Method entity type depends on self parameter AND return type
- Impl blocks need special qualified name format (`<Type as Trait>::method`)
- Generic bounds affect relationship extraction
- Visibility has more variants (pub, pub(crate), pub(super), pub(in path))

**Conclusion:** Needs escape hatches but structure can still be declarative

---

## Proposed Architecture

### Layer 1: Core Traits (spec-aligned)

```rust
/// Matches entity_rules from spec
pub trait EntityRule {
    const CAPTURE: &'static str;
    const ENTITY_TYPE: EntityType;
    fn condition(ctx: &HandlerContext) -> bool { true }  // Optional
}

/// Matches visibility_rules from spec
pub trait VisibilityRule {
    fn visibility(ctx: &HandlerContext) -> Option<Visibility>;
}

/// Matches qualified_name_rules from spec
pub trait QualifiedNameRule {
    fn qualified_name(ctx: &HandlerContext) -> String;
}

/// Matches metadata_rules from spec
pub trait MetadataRule {
    fn metadata(ctx: &HandlerContext) -> EntityMetadata;
}

/// Matches relationship_rules from spec
pub trait RelationshipRule {
    fn relationships(ctx: &HandlerContext) -> EntityRelationshipData;
}

/// Combined handler
pub trait EntityHandler:
    EntityRule + VisibilityRule + QualifiedNameRule + MetadataRule + RelationshipRule
{
    fn handle(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>>;
}
```

### Layer 2: Language-Specific Defaults

```rust
/// JavaScript visibility: export → Public, else Private
pub trait JsVisibility: VisibilityRule {
    fn visibility(ctx: &HandlerContext) -> Option<Visibility> {
        if has_export_keyword(ctx) { Some(Visibility::Public) }
        else { Some(Visibility::Private) }
    }
}

/// Rust qualified names with import resolution
pub trait RustQualifiedName: QualifiedNameRule {
    fn qualified_name(ctx: &HandlerContext) -> String {
        // Default: module::name
        // Override for impl blocks, trait methods, etc.
    }
}
```

### Layer 3: Convenience Macros

```rust
// Simple JS handler - fully declarative
js_handler!(FunctionDeclaration, "function", Function,
    metadata: function_metadata,
    relationships: extract_calls);

// Simple Rust handler - uses language defaults
rust_handler!(Function, "function", Function,
    condition: |ctx| !is_in_impl_block(ctx),
    metadata: extract_rust_function_metadata,
    relationships: extract_rust_function_relationships);

// Complex Rust handler - manual impl
#[derive(EntityHandler)]
#[handler(Rust, ImplBlock, "impl_item")]
pub struct RustImplBlockHandler;

impl RustImplBlockHandler {
    // Full control over all aspects
}
```

---

## User Decisions

1. **Spec execution:** Generate code from specs (YAML is source of truth)
2. **Python removal:** Yes, remove entirely
3. **Resolution split:** Keep full resolution accuracy, but split between extraction and outbox-processor

---

## Key Insight: Extraction/Resolution Split

The outbox-processor already handles relationship resolution via `GenericResolver` and `LookupStrategy` chains. This means:

**Extraction time (languages crate):**
- Extract entities with basic qualified names
- Populate `EntityRelationshipData` fields with raw references
- Minimal resolution - just capture what's in the source

**Resolution time (outbox-processor):**
- Resolve raw references to actual entity IDs
- Use lookup strategies: qname, path_id, call_alias, unique_simple_name
- Handle cross-crate references

This simplifies the extraction layer significantly.

---

## The N:M Graph Problem

### Current Implementation

The decision graph is **implicit in tree-sitter queries**:

```
AST Pattern Space (overlapping regions)
├── const arrow_function  ──► ARROW_FUNCTION_QUERY ──► Function
├── const function_expr   ──► FUNCTION_EXPRESSION_QUERY ──► Function
├── const other           ──► CONST_QUERY (excludes above) ──► Constant
└── let/var anything      ──► LET_QUERY/VAR_QUERY ──► Variable
```

Precedence is encoded via `#not-match?` predicates:
```sexp
;; CONST_QUERY excludes function patterns explicitly
(#not-match? @value "^(function|async function|\\(|\\w+\\s*=>)")
```

### The Graph Structure

```
          ┌─────────────┐
          │  AST Node   │
          └──────┬──────┘
                 │ matches
                 ▼
    ┌────────────────────────────┐
    │     Pattern Groups         │ (overlapping regions)
    │  ┌────────────────────┐    │
    │  │  arrow_function    │◄───┼─── Priority 1
    │  │  function_expr     │◄───┼─── Priority 2
    │  │  const_value       │◄───┼─── Priority 3
    │  └────────────────────┘    │
    └────────────────────────────┘
                 │ first match wins
                 ▼
    ┌────────────────────────────┐
    │     Entity Type            │
    │     + Extractor            │
    └────────────────────────────┘
```

This is analogous to:
- **Lexer generators**: Maximal munch + priority rules
- **Pattern match compilation**: Rust's match exhaustiveness checking
- **Rule engines**: Conflict resolution strategies (Rete/Drools)

### What Needs to be Explicit in Specs

1. **Pattern Groups**: Which rules compete for the same AST space
2. **Priority/Precedence**: Which rule wins when multiple match
3. **Extractors**: Shared extraction functions (N:M with rules)
4. **Conditions**: Runtime guards beyond pattern matching

---

## Proposed Architecture: Spec-Driven Code Generation

### Build Pipeline

```
specs/*.yaml  ──►  build.rs  ──►  generated/handlers.rs
                    (parse)        (Rust code)

                    Phases:
                    1. Parse entity_rules
                    2. Group by overlapping patterns
                    3. Sort by priority
                    4. Generate decision tree/queries
                    5. Wire to extractors
```

### Spec Extensions Needed

The current specs define WHAT to extract but not HOW. We need to extend them with:

```yaml
# NEW: Pattern groups define overlapping AST regions with priority
pattern_groups:
  variable_initializer:
    # AST context: lexical_declaration > variable_declarator > value
    ast_context: "lexical_declaration.variable_declarator.value"
    rules_by_priority:
      1: E-FN-ARROW      # Arrow functions win
      2: E-FN-EXPR       # Function expressions next
      3: E-CONST         # Plain constants last

entity_rules:
  - id: E-FN-ARROW
    produces: Function
    pattern_group: variable_initializer
    condition: "value.type == 'arrow_function'"
    extractor: function_expression_extractor

  - id: E-FN-EXPR
    produces: Function
    pattern_group: variable_initializer
    condition: "value.type in ['function', 'function_expression']"
    extractor: function_expression_extractor  # Same extractor, different entity

  - id: E-CONST
    produces: Constant
    pattern_group: variable_initializer
    condition: "declaration_kind == 'const'"
    extractor: constant_extractor

  - id: E-FN-DECL
    produces: Function
    # Not in a pattern group - unique pattern (function declarations)
    tree_sitter:
      query: FUNCTION_DECLARATION_QUERY
    extractor: function_declaration_extractor

# Extractors can be shared across rules (N:M relationship)
extractors:
  function_expression_extractor:
    metadata: function_metadata
    relationships: extract_function_relationships
    name_derivation: from_variable_name

  function_declaration_extractor:
    metadata: function_metadata
    relationships: extract_function_relationships
    name_derivation: from_identifier_capture

  constant_extractor:
    metadata: const_metadata
    relationships: null
    name_derivation: from_identifier_capture
```

### Generated Handler Code

```rust
// Auto-generated from pattern_group "variable_initializer"
fn handle_variable_initializer(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
    let value_node = get_value_node(ctx)?;

    // Priority 1: E-FN-ARROW
    if value_node.kind() == "arrow_function" {
        return function_expression_extractor(ctx, EntityType::Function);
    }
    // Priority 2: E-FN-EXPR
    if matches!(value_node.kind(), "function" | "function_expression") {
        return function_expression_extractor(ctx, EntityType::Function);
    }
    // Priority 3: E-CONST
    if is_const_declaration(ctx) {
        return constant_extractor(ctx, EntityType::Constant);
    }
    // Fallthrough: E-VAR
    return variable_extractor(ctx, EntityType::Variable);
}

// Auto-generated from standalone rule E-FN-DECL
pub struct FunctionDeclarationHandler;

impl EntityHandler for FunctionDeclarationHandler {
    const LANGUAGE: Language = Language::JavaScript;
    const ENTITY_TYPE: EntityType = EntityType::Function;
    const CAPTURE: &'static str = "function";

    fn handle(ctx: &ExtractionContext) -> Result<Vec<CodeEntity>> {
        let node = extract_main_node(ctx, "function")?;
        let name = extract_capture_text(ctx, "name")?;

        let visibility = Self::extract_visibility(ctx);
        let qualified_name = Self::build_qualified_name(ctx, &name);
        let metadata = Self::extract_metadata(ctx, node);
        let relationships = Self::extract_relationships(ctx, node);

        // ... build entity
    }
}
```

### Escape Hatch: Custom Implementations

For complex cases that don't fit the declarative spec:

```yaml
entity_rules:
  - id: E-IMPL-TRAIT
    produces: ImplBlock
    # Use custom implementation instead of generated
    custom_handler: "rust::impl_block::handle_trait_impl"
```

---

## Implementation Plan

### Phase 1: Remove Python, Simplify Foundation

1. Delete `crates/languages/src/python/` entirely
2. Simplify Rust handlers to use outbox-processor for resolution
3. Define core `EntityHandler` trait matching spec structure

### Phase 2: Extend Specs with Extraction Hints

1. Add `pattern_groups` section for overlapping rules
2. Add `extractor` references to entity rules
3. Add `condition` expressions for runtime guards
4. Start with JavaScript spec (simplest)

### Phase 3: Build-Time Code Generation

1. Add spec parser to `crates/languages/build.rs`
2. Generate decision trees for pattern groups
3. Generate handler structs for standalone rules
4. Wire generated handlers into `define_language_extractor!`

### Phase 4: Migrate JavaScript

1. Extend javascript.yaml with extraction hints
2. Generate JS handlers from spec
3. Delete old `define_handler!` calls
4. Verify tests pass

### Phase 5: Migrate TypeScript

1. Extend typescript.yaml
2. Handle TS-specific cases (interfaces, type aliases)
3. Use `custom_handler` escape hatch where needed

### Phase 6: Migrate Rust

1. Extend rust.yaml
2. Use `custom_handler` for complex cases (impl blocks, generics)
3. Simplify non-complex handlers to generated code

### Phase 7: Cleanup

1. Delete old `define_handler!` macro variants
2. Delete old `define_ts_family_handler!` macro
3. Update documentation

---

## Files to Change

| Category | Files |
|----------|-------|
| Remove | `crates/languages/src/python/` (entire directory) |
| Extend | `crates/languages/specs/*.yaml` (add extraction hints) |
| New | `crates/languages/build.rs` (spec parser + codegen) |
| New | `crates/languages/src/generated/` (generated handlers) |
| Modify | `crates/languages-macros/src/lib.rs` (wire generated handlers) |
| Simplify | `crates/languages/src/rust/handler_impls/*.rs` (reduce resolution) |
| Delete | Old macro variants in `language_extractors.rs` |

---

## Verification

1. **Build:** `cargo build --workspace --all-targets`
2. **Lint:** `cargo clippy --workspace && cargo fmt --check`
3. **Tests:** `cargo test --workspace`
4. **E2E:** `cargo test --manifest-path crates/e2e-tests/Cargo.toml -- --ignored`

At each phase, all tests must pass before proceeding.

---

## Open Questions

1. **Condition language**: What syntax/language for `condition` expressions in the spec?
   - Simple: Node kind checks only
   - Medium: A mini-expression language parsed at build time
   - Complex: Reference Rust functions by name

2. **Query generation vs. handwritten**: Should pattern groups generate tree-sitter queries, or use a single broad query + runtime filtering?

3. **Extractor interface**: Exact signature for extractor functions - do they receive the full context or just what they need?
