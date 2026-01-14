# Extraction Status Report

This document provides a comprehensive view of the spec-driven extraction system, combining analysis from previous investigations with changes made in the current session.

## Test Results Summary

| Category | Passed | Failed | Total |
|----------|--------|--------|-------|
| Rust | 45 | 16 | 61 |
| TypeScript | 31 | 7 | 38 |
| JavaScript | 1 | 2 | 3 |
| **Total** | **76** | **24** | **100** |

---

## Changes Made This Session

### QualifiedName Structured Type

**Problem Solved:** Validation logic used `child_fqn.starts_with(parent_fqn)` for CONTAINS relationships, which failed for trait impls because `<crate::Type as crate::Trait>` doesn't start with `crate`.

**Solution Implemented:**

Created `crates/core/src/qualified_name.rs` with a structured enum:

```rust
pub enum QualifiedName {
    SimplePath { segments: Vec<String>, separator: PathSeparator },
    InherentImpl { scope: Vec<String>, type_path: Vec<String> },
    TraitImpl { scope: Vec<String>, type_path: Vec<String>, trait_path: Vec<String> },
    TraitImplItem { type_path: Vec<String>, trait_path: Vec<String>, item_name: String },
    ExternBlock { scope: Vec<String>, linkage: String },
}
```

**Key Method:** `is_child_of(&self, parent: &QualifiedName) -> bool`
- Handles semantic containment for all qualified name variants
- TraitImpl is child of module if type_path OR trait_path starts with module segments
- ExternBlock items are children of the extern block's scope

**Result:** `test_trait_impl` now PASSES (was previously failing due to validation logic)

**Files Changed:**
- `crates/core/src/qualified_name.rs` (created)
- `crates/core/src/lib.rs` (added export)
- `crates/core/src/entities.rs` (changed `qualified_name: String` to `qualified_name: QualifiedName`)
- 15+ files updated to use `.to_string()` conversion at API boundaries

---

## Remaining Failures by Category

### Category 1: Foreign Type Prefix Issue (2 tests)

**Tests:** `test_extension_traits`, `test_blanket_impl`

**Problem:** When implementing traits for standard library types like `String` or `str`, extraction incorrectly prefixes the type with the local module scope.

**Example:**
```rust
impl StringExt for String { ... }
```

| Expected | Actual |
|----------|--------|
| `<String as test_crate::StringExt>` | `<test_crate::String as test_crate::StringExt>` |

**Root Cause:** The qualified name construction in extraction always prepends the module scope to the type, even for foreign/external types.

**Fix Required:** Detect when a type is external (not defined in current crate) and omit the module prefix.

---

### Category 2: Extern Block Issues (1 test)

**Test:** `test_extern_blocks`

**Problems:**
1. **Visibility:** Private extern functions extracted with `Public` visibility instead of `Private`
2. **Containment:** Extern block items are siblings with the extern block under the module, not children of the extern block

**Expected Relationships:**
```
CONTAINS test_crate -> test_crate::extern "C"
CONTAINS test_crate::extern "C" -> test_crate::external_function
```

**Actual Relationships:**
```
CONTAINS test_crate -> test_crate::extern "C"
CONTAINS test_crate -> test_crate::external_function  // Wrong parent!
```

**Root Cause:**
1. Visibility extraction for extern items doesn't account for implicit privacy
2. `parent_scope` for extern block items is set to the module, not the extern block

---

### Category 3: Tuple Struct Fields (1 test)

**Test:** `test_tuple_and_unit_structs`

**Problem:** Tuple struct fields (positional) are not extracted.

**Example:**
```rust
pub struct Point(pub f64, pub f64);
```

**Expected:** `Property test_crate::Point::0`, `Property test_crate::Point::1`
**Actual:** Only the struct itself is extracted

**Root Cause:** The STRUCT_FIELD query only matches named fields (`field_declaration`), not positional fields in tuple structs.

**Fix Required:** Add query for tuple struct field extraction.

---

### Category 4: Type Alias USES Relationships (2 tests)

**Tests:** `test_type_aliases`, `test_type_alias_chains`

**Problem:** Type aliases don't extract USES relationships to their target types.

**Example:**
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

**Expected:** `USES test_crate::Result -> test_crate::Error`
**Actual:** No USES relationship

**Root Cause:** Type alias handler doesn't have relationship extraction configured.

---

### Category 5: Method/Function Disambiguation (3 tests)

**Tests:** `test_builder_pattern`, `test_trait_vs_inherent_method`, `test_scattered_impl_blocks`

**Problem:** `#not-has-child? @params self_parameter` predicate is not evaluated by tree-sitter.

**Background:** (from TREE_SITTER_PREDICATE_ANALYSIS.md)
- `#not-has-child?` is NOT a built-in tree-sitter predicate
- Current workaround only handles `trait` field, NOT `self_parameter`
- `self_parameter` is a child node type, not a field, so `!self_parameter` syntax doesn't work

**Result:** Methods with `self` may be incorrectly classified, or methods/functions may not be properly distinguished.

**Fix Required:** Implement structural AST check for `self_parameter` presence in handler code.

---

### Category 6: Complex Trait Patterns (4 tests)

**Tests:** `test_associated_types`, `test_associated_types_resolution`, `test_generic_bounds_resolution`, `test_generic_trait`

**Problems:**
- Associated type declarations not extracted
- Associated type resolution not implemented
- Generic bound resolution (`T: Trait`) not implemented
- Generic trait implementation handling incomplete

**Status:** These are feature gaps requiring new extraction logic.

---

### Category 7: UFCS Qualified Name Format (1 test)

**Test:** `test_ufcs_explicit`

**Problem:** Unclear what the canonical qualified name format should be for UFCS calls.

**Design Question:** Should `<MyHandler as Handler>::handle(&handler)` generate:
- `<MyHandler as Handler>::handle` (UFCS style)
- `MyHandler::handle` (simple style)
- `Handler::handle` (trait-centric style)

---

### Category 8: Other Rust Issues (2 tests)

**Tests:** `test_complex_enums`, `test_prelude_shadowing`

- Complex enum variant handling incomplete
- Prelude type shadowing detection not implemented

---

### Category 9: TypeScript Function/Class Expressions (3 tests)

**Tests:** `test_function_expressions`, `test_class_expressions`, `test_constants_variables`

**Problem:** Named function/class expressions assigned to variables aren't extracted correctly.

**Example:**
```typescript
const named = function namedFunction() { ... };
const MyClass = class { ... };
```

**Root Cause:** (from TREE_SITTER_PREDICATE_ANALYSIS.md)
- The `#not-match?` regex for excluding arrow functions has gaps
- Queries need to match `function_expression` and `class_expression` node types explicitly

---

### Category 10: TypeScript Parameter Properties (1 test)

**Test:** `test_parameter_properties`

**Problem:** Constructor parameter properties include constructor in qualified name.

**Example:**
```typescript
class Point {
    constructor(public x: number) {}
}
```

**Expected:** `point.Point.x`
**Actual:** `point.Point.constructor.x`

**Root Cause:** The `skip_scopes` field in handler config is not implemented.

---

### Category 11: TypeScript Index Signatures (1 test)

**Test:** `test_index_signatures`

**Problem:** Index signatures use hardcoded `[index]` instead of actual key type.

**Design Decision Needed:** What should the name be?
- `[string]` / `[number]` (based on key type)
- `[key]` (actual parameter name)

---

### Category 12: TypeScript Type Usage (1 test)

**Test:** `test_type_usage`

**Problem:** USES relationships for type references not being extracted correctly.

---

### Category 13: JavaScript Issues (2 tests)

**Tests:** `test_functions`, `test_classes`

**Problem:** Some function/class patterns in JavaScript aren't being extracted.

**Root Cause:** Query differences between TypeScript and JavaScript grammars.

---

## Priority Matrix

### High Priority (Blocking Core Functionality)

| Issue | Tests Affected | Effort |
|-------|---------------|--------|
| Foreign type prefix | 2 | Medium |
| Method/Function self_parameter check | 3 | Medium |
| Type alias USES relationships | 2 | Low |

### Medium Priority (Feature Completeness)

| Issue | Tests Affected | Effort |
|-------|---------------|--------|
| Extern block containment & visibility | 1 | Medium |
| Tuple struct fields | 1 | Low |
| TypeScript function/class expressions | 3 | Medium |
| Parameter property skip_scopes | 1 | Low |

### Lower Priority (Edge Cases / Design Decisions)

| Issue | Tests Affected | Effort |
|-------|---------------|--------|
| Associated types | 2 | High |
| Generic bounds resolution | 2 | High |
| UFCS format decision | 1 | Design |
| Index signature naming | 1 | Design |
| JavaScript grammar differences | 2 | Medium |

---

## Recommended Next Steps

### 1. Fix Foreign Type Prefix (High Impact)

In the qualified name construction for trait impls, detect if the type is defined in the current crate or is external. For external types (like `String`, `str`, `Vec`), don't prepend the module scope.

### 2. Implement `self_parameter` Structural Check

Replace the non-functional `#not-has-child? @params self_parameter` with:

```rust
fn has_self_parameter(params_node: Node) -> bool {
    let mut cursor = params_node.walk();
    params_node.children(&mut cursor)
        .any(|child| child.kind() == "self_parameter")
}
```

Call this in METHOD vs FUNCTION handler disambiguation.

### 3. Fix Extern Block Parent Scope

When extracting extern block items, set `parent_scope` to the extern block's qualified name, not the containing module.

### 4. Add Type Alias Relationship Extraction

Add `relationships: extract_type_relationships` or similar to type alias handler config.

### 5. Add Tuple Struct Field Query

Create a query that matches positional fields in tuple struct patterns with numeric names (`0`, `1`, etc.).

---

## Files Reference

| File | Purpose |
|------|---------|
| `crates/core/src/qualified_name.rs` | Structured qualified name type |
| `crates/languages/src/spec_driven/engine.rs` | Query execution, `should_skip_match()` |
| `crates/languages/src/specs/rust.yaml` | Rust extraction queries |
| `crates/languages/src/specs/typescript.yaml` | TypeScript extraction queries |
| `crates/e2e-tests/tests/spec_validation/` | Spec validation fixtures and tests |

---

## Test Commands

```bash
# Run all spec validation tests
cargo test --manifest-path crates/e2e-tests/Cargo.toml -- --ignored

# Run specific test
cargo test --manifest-path crates/e2e-tests/Cargo.toml rust::test_extern_blocks -- --ignored

# Run fixture consistency tests (no Docker)
cargo test --manifest-path crates/e2e-tests/Cargo.toml -- "test_contains_relationships"
```
