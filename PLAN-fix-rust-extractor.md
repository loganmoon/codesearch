# Plan: Fix Rust Extractor to Meet Spec

## Summary

18 tests failed out of 50. The failures fall into 6 categories of issues that need to be fixed in the Rust extractor code.

---

## Issue Categories

### 1. Inherent Method/Constant FQN Format (Q-INHERENT-METHOD)

**Current Behavior:**
```
test_crate::Counter::new
test_crate::Foo::method
test_crate::MyType::INHERENT_CONST
```

**Expected Behavior (per spec):**
```
<test_crate::Counter>::new
<test_crate::Foo>::method
<test_crate::MyType>::INHERENT_CONST
```

**Affected Tests (8):**
- test_spec_validation_methods
- test_spec_validation_multiple_impl_blocks
- test_spec_validation_builder_pattern
- test_spec_validation_async_functions
- test_spec_validation_type_alias_chains
- test_spec_validation_scattered_impl_blocks
- test_spec_validation_trait_vs_inherent_method
- test_spec_validation_associated_constants

**Fix Location:**
- `crates/languages/src/rust/handler_impls/impl_handlers.rs`
- Function: `extract_method()` around lines 800-817
- Also: associated constants extraction around line 660

**Change Required:**
When building qualified names for inherent impl items (methods and constants), use UFCS format:
```rust
// BEFORE (inherent impl method):
let qualified_name = format!("{}::{}", type_fqn, method_name);

// AFTER:
let qualified_name = format!("<{}>::{}", type_fqn, method_name);
```

---

### 2. Associated Types in Trait Impls FQN Format (Q-ASSOCIATED-TYPE)

**Current Behavior:**
```
test_crate::Counter::Item
```

**Expected Behavior (per spec):**
```
<test_crate::Counter as test_crate::Iterator>::Item
```

**Affected Tests (1):**
- test_spec_validation_associated_types

**Fix Location:**
- `crates/languages/src/rust/handler_impls/impl_handlers.rs`
- Function: associated type handling around lines 715-782

**Change Required:**
Associated types in trait impls should use UFCS format like methods:
```rust
// BEFORE:
let qualified_name = format!("{}::{}", impl_ctx.for_type_resolved, name);

// AFTER:
let qualified_name = format!("<{} as {}>::{}",
    impl_ctx.for_type_resolved,
    impl_ctx.trait_name_resolved.unwrap(),
    name);
```

---

### 3. Foreign Type Detection in Impl Blocks

**Current Behavior:**
```
<test_crate::String as test_crate::StringExt>::is_blank
test_crate::<test_crate::String as test_crate::StringExt>
```

**Expected Behavior:**
```
<String as test_crate::StringExt>::is_blank
test_crate::<String as test_crate::StringExt>
```

`String` and `str` are foreign types from std - they should NOT have the local crate prefix.

**Affected Tests (1):**
- test_spec_validation_extension_traits

**Fix Location:**
- `crates/languages/src/rust/handler_impls/impl_handlers.rs`
- Where `for_type` is resolved to FQN
- `crates/languages/src/rust/import_resolution.rs` - `resolve_rust_reference()`

**Change Required:**
When resolving the "for type" in an impl block, check if it's a well-known std type and don't add crate prefix:
```rust
const STD_TYPES: &[&str] = &[
    "String", "str", "Vec", "Option", "Result", "Box", "Rc", "Arc",
    "HashMap", "HashSet", "BTreeMap", "BTreeSet", "Path", "PathBuf",
    // ... other common std types
];

fn resolve_type_name(type_name: &str, package_name: &str) -> String {
    // Don't add package prefix to std types
    if STD_TYPES.contains(&type_name) {
        return type_name.to_string();
    }
    // ... normal resolution
}
```

---

### 4. IMPORTS Relationship Source (R-IMPORTS)

**Current Behavior:**
```
IMPORTS test_crate::caller -> test_crate::utils::helper
IMPORTS my_utils::process_core -> my_core::CoreType
```

**Expected Behavior (per spec R-IMPORTS: Module IMPORTS Entity):**
```
IMPORTS test_crate -> test_crate::utils::helper
IMPORTS my_utils -> my_core::CoreType
```

Imports should come from the MODULE containing the use statement, not from individual functions/items.

**Affected Tests (3):**
- test_spec_validation_use_imports
- test_spec_validation_workspace_basic
- test_spec_validation_reexports

**Fix Location:**
- `crates/languages/src/rust/handler_impls/function_handlers.rs` line 195
- `crates/languages/src/rust/handler_impls/impl_handlers.rs` lines 224, 425, 892
- `crates/languages/src/rust/handler_impls/type_handlers.rs` lines 153, 264, 406

**Change Required:**
Don't attach imports to individual entities. Instead, create IMPORTS relationships from the module entity. This may require:
1. Extracting imports at the module level in module_handlers.rs
2. Or creating a separate pass that processes use statements and creates module-level IMPORTS

---

### 5. Missing Entity Types

#### 5a. Static Items (E-STATIC)

**Current Behavior:** Static items are extracted as `Constant`
**Expected Behavior:** Static items should be extracted as `Static`

**Affected Tests (1):**
- test_spec_validation_statics

**Fix Location:**
- `crates/languages/src/rust/handler_impls/constant_handlers.rs`
- `crates/languages/src/rust/queries.rs` - need separate query for static items

**Change Required:**
1. Add `STATIC_QUERY` to queries.rs to match `static_item` nodes
2. Add `handle_static_impl` function or modify `handle_constant_impl` to distinguish
3. Use `EntityType::Static` (need to add to core if not exists)

#### 5b. Union Types (E-UNION)

**Current Behavior:** Union types are not extracted at all
**Expected Behavior:** Union types should produce `Union` entities

**Affected Tests (1):**
- test_spec_validation_unions

**Fix Location:**
- `crates/languages/src/rust/handler_impls/type_handlers.rs` - add union handling
- `crates/languages/src/rust/queries.rs` - add UNION_QUERY

**Change Required:**
1. Add tree-sitter query for `union_item` nodes
2. Add `handle_union_impl` function similar to struct/enum handlers
3. Use `EntityType::Union` (need to add to core if not exists)

#### 5c. Extern Blocks (E-EXTERN-BLOCK)

**Current Behavior:** Extern blocks and their contents are not extracted
**Expected Behavior:**
- `extern "C" { ... }` produces `ExternBlock` entity
- Functions inside produce `Function` entities
- Statics inside produce `Static` entities

**Affected Tests (1):**
- test_spec_validation_extern_blocks

**Fix Location:**
- Add new file `crates/languages/src/rust/handler_impls/extern_handlers.rs`
- `crates/languages/src/rust/queries.rs` - add EXTERN_BLOCK_QUERY

**Change Required:**
1. Add tree-sitter query for `extern_block` nodes
2. Create handler that extracts:
   - The extern block itself as `ExternBlock` entity
   - Nested function declarations as `Function` entities
   - Nested static declarations as `Static` entities
3. Use `EntityType::ExternBlock` (need to add to core if not exists)

---

### 6. Visibility Issues

#### 6a. pub(self) Should Be Private

**Current Behavior:** `pub(self)` maps to `Visibility::Internal`
**Expected Behavior:** `pub(self)` should map to `Visibility::Private`

**Affected Tests (1):**
- test_spec_validation_visibility

**Fix Location:**
- `crates/languages/src/rust/handler_impls/common.rs`
- Function: `extract_visibility_from_node()` around lines 34-86

**Change Required:**
```rust
// BEFORE:
"pub(self)" => Visibility::Internal,  // or however it's handled

// AFTER:
"pub(self)" => Visibility::Private,
```

#### 6b. Macro Visibility Without #[macro_export]

**Current Behavior:** Macros without `#[macro_export]` shown as `Public`
**Expected Behavior:** Macros without `#[macro_export]` should be `Private`

**Affected Tests (1):**
- test_spec_validation_macro_rules

**Fix Location:**
- `crates/languages/src/rust/handler_impls/macro_handlers.rs`

**Change Required:**
Check for `#[macro_export]` attribute when determining macro visibility:
```rust
fn determine_macro_visibility(node: &Node, source: &[u8]) -> Visibility {
    // Check for #[macro_export] attribute
    if has_macro_export_attribute(node) {
        Visibility::Public
    } else {
        Visibility::Private
    }
}
```

---

## Implementation Order

Recommended order based on impact and dependencies:

### Phase 1: Core Entity Type Additions
1. Add `EntityType::Static`, `EntityType::Union`, `EntityType::ExternBlock` to core
2. Add corresponding queries to queries.rs
3. Implement handlers for static, union, extern blocks

### Phase 2: Qualified Name Format Fixes (High Impact - fixes 10 tests)
4. Fix inherent method/constant FQN format (UFCS `<Type>::name`)
5. Fix associated type FQN format in trait impls

### Phase 3: Foreign Type Detection
6. Fix foreign type detection (don't add crate prefix to std types)

### Phase 4: IMPORTS Relationship Fix
7. Change IMPORTS to come from module, not individual items

### Phase 5: Visibility Fixes
8. Fix pub(self) → Private mapping
9. Fix macro visibility (check for #[macro_export])

---

## Files to Modify

| File | Changes |
|------|---------|
| `crates/core/src/entities.rs` | Add Static, Union, ExternBlock entity types (if not present) |
| `crates/languages/src/rust/queries.rs` | Add STATIC_QUERY, UNION_QUERY, EXTERN_BLOCK_QUERY |
| `crates/languages/src/rust/handler_impls/impl_handlers.rs` | Fix UFCS format for inherent methods/constants, fix associated type FQN |
| `crates/languages/src/rust/handler_impls/constant_handlers.rs` | Separate static from const handling |
| `crates/languages/src/rust/handler_impls/type_handlers.rs` | Add union handling |
| `crates/languages/src/rust/handler_impls/extern_handlers.rs` | New file for extern block handling |
| `crates/languages/src/rust/handler_impls/common.rs` | Fix pub(self) visibility |
| `crates/languages/src/rust/handler_impls/macro_handlers.rs` | Fix macro visibility |
| `crates/languages/src/rust/handler_impls/module_handlers.rs` | Add IMPORTS relationships |
| `crates/languages/src/rust/import_resolution.rs` | Foreign type detection |
| `crates/languages/src/rust/mod.rs` | Register new handlers |

---

## Test Expectations After Fixes

After all fixes, all 50 tests should pass:
- 32 tests already passing
- 18 tests currently failing should pass
