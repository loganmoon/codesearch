# Handler Refactor Plan

**Issue:** #191
**Branch:** `refactor--191-trait-proc-macro`

## Goal

Replace the 16+ variant `define_handler!` macro with spec-driven code generation where YAML specs are the single source of truth.

## Completed

### Phase 1: Remove Python
- Deleted `crates/languages/src/python/` entirely
- Commit: `ba2011f`

### Phase 2: Extend JS/TS Specs with Extraction Hints
- Added `extraction_hints` section to javascript.yaml and typescript.yaml
- Commit: `62cddc9`

### Phase 2.1: Simplify Precedence Handling
- Removed `pattern_groups` as a runtime disambiguation mechanism
- Precedence is now handled directly in tree-sitter queries via `#not-match?` predicates
- Removed `pattern_group` and `priority` fields from handlers
- Added `template` name strategy for declarative name derivation (e.g., IndexSignature)
- JS/TS specs are now fully declarative (no escape hatches)

### Phase 3: Add Extraction Hints to Rust Spec
- Added comprehensive `extraction_hints` section to rust.yaml
- Key design decisions:
  - **Query-level scoping**: Separate queries for each context (inherent impl, trait impl, trait def)
  - **AST captures for type components**: No string parsing; use tree-sitter to extract type_identifier, etc.
  - **Engine-provided context**: `{crate}`, `{module_path}`, `{scope}` placeholders from extraction engine
  - **Import resolution deferred**: Raw type names captured; outbox-processor handles resolution
- Covered all entity types: modules, functions, methods, impl blocks, structs, enums, traits, etc.
- Added `qualified_name_template` to all handlers for declarative qname construction
- Added new name strategies: `crate_name`, `positional_index`

### Phase 4: Build-Time Code Generation
- Created `crates/languages/build.rs` with YAML spec parsing
- Generates per-language modules with:
  - **Query constants**: Tree-sitter queries from `extraction_hints.queries`
  - **Handler configs**: `HandlerConfig` structs from `extraction_hints.handlers`
- Created `crates/languages/src/spec_driven/mod.rs`:
  - Core types: `HandlerConfig`, `NameStrategy`, `MetadataExtractor`, `RelationshipExtractor`
  - Includes generated modules via `include!(concat!(env!("OUT_DIR"), "/*_generated.rs"))`
- All specs (JS, TS, Rust) now generate compilable code
- Build dependencies: `serde`, `serde_yaml`

## Extraction Hints Schema

```yaml
extraction_hints:
  queries:
    # Tree-sitter queries with context scoping
    # Use predicates like #not-match?, #eq?, #not-has-child? for disambiguation
    METHOD_IN_INHERENT_IMPL:
      capture: "method"
      query: |
        (impl_item
          type: (type_identifier) @impl_type_name
          body: (declaration_list
            (function_item
              parameters: (parameters (self_parameter))
              name: (identifier) @name) @method))
        (#not-has-child? @impl_item trait)

  extractors:
    # Shared metadata/relationship extraction (N:M with handlers)
    function_metadata:
      metadata_fields:
        - is_async
        - is_const
        - is_unsafe

  handlers:
    # Qualified name templates use:
    # - Engine-provided: {crate}, {module_path}, {scope}
    # - Query captures: {name}, {impl_type_name}, {trait_name}, etc.
    MethodInInherentImpl:
      entity_rule: E-METHOD-SELF
      query: METHOD_IN_INHERENT_IMPL
      capture: "method"
      name_strategy: capture
      qualified_name_template: "<{impl_type_name}>::{name}"
      metadata: function_metadata
```

## Remaining Phases

### Phase 5: Create Spec-Driven Extraction Engine

1. Implement extraction logic that uses `HandlerConfig` to:
   - Run tree-sitter queries from config
   - Derive entity names using `NameStrategy`
   - Build qualified names from `qualified_name_template`
   - Call metadata/relationship extractors
2. Wire the spec-driven engine into extractor implementations
3. Verify tests pass

### Phase 6: Migrate Languages

For each language (JS, TS, Rust):
1. Replace old handlers with spec-driven extraction
2. Delete old `define_handler!` calls
3. Verify tests pass

### Phase 7: Cleanup

1. Delete old `define_handler!` macro variants
2. Delete `define_ts_family_handler!` macro
3. Update documentation

## Handler Configuration Options

- `name_strategy`: `capture` | `fallback` | `static` | `template` | `file_path` | `crate_name` | `positional_index`
- `name_captures`: List of captures for `fallback` strategy
- `static_name`: Fixed name for `static` strategy
- `name_template`: Template string with `{capture_name}` placeholders for `template` strategy
- `qualified_name_template`: Template for full qualified name (uses engine-provided + captures)
- `visibility_override`: Override visibility (e.g., `Public` for trait impl members, `null` for None)
- `metadata`: Reference to extractor for metadata fields
- `relationships`: Reference to extractor for relationship fields
- `skip_scopes`: AST node kinds to skip during scope traversal

## Verification

At each phase:
1. `cargo build --workspace --all-targets`
2. `cargo clippy --workspace && cargo fmt --check`
3. `cargo test --workspace`
