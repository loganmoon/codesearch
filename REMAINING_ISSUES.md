# Remaining Spec-Driven Extraction Issues

This document catalogs the 25 failing spec validation tests and the underlying issues that need design decisions.

## Summary

| Category | Count | Status |
|----------|-------|--------|
| Rust: Method vs Property disambiguation | 1 | Needs design decision |
| Rust: Tuple struct field extraction | 1 | Not implemented |
| Rust: Type alias USES relationships | 2 | Not implemented |
| Rust: UFCS qualified name format | 3 | Needs design decision |
| Rust: Trait impl FQN prefix validation | 1 | Needs design decision |
| Rust: Complex entity patterns | 6 | Various issues |
| TypeScript: Function expression extraction | 2 | Query issues |
| TypeScript: Parameter property qualified names | 1 | skip_scopes not implemented |
| TypeScript: Index signature naming | 1 | Needs design decision |
| TypeScript: Constants vs arrow functions | 1 | Query priority issue |
| TypeScript: Class expression extraction | 1 | Not implemented |
| JavaScript: Function extraction | 2 | Query issues |

---

## Rust Issues

### 1. Method vs Property Name Collision (test_builder_pattern)

**Problem:** When a struct has fields with the same names as impl methods, both are extracted but the Property entity appears in results instead of Method.

**Example:**
```rust
pub struct ConfigBuilder {
    name: Option<String>,  // Field "name"
    value: Option<i32>,    // Field "value"
}

impl ConfigBuilder {
    pub fn name(mut self, name: &str) -> Self { ... }  // Method "name"
    pub fn value(mut self, value: i32) -> Self { ... } // Method "value"
}
```

**Expected:** Both `Property test_crate::ConfigBuilder::name` AND `Method test_crate::ConfigBuilder::name`
**Actual:** Only `Property test_crate::ConfigBuilder::name` appears

**Root Cause Options:**
1. Entity deduplication is keeping Property over Method (wrong priority)
2. The method query isn't matching methods with `(mut self, additional_params)`
3. Both entities exist but test validation only checks for one

**Design Decision Needed:** How should entities with the same qualified name but different types be handled? Options:
- Disambiguate with entity type in qualified name (e.g., `ConfigBuilder::name#method`)
- Keep both entities with same qualified name (requires ID disambiguation)
- Prioritize one type over another

---

### 2. Tuple Struct Field Extraction (test_tuple_and_unit_structs)

**Problem:** Tuple struct fields (accessed by numeric index) are not extracted.

**Example:**
```rust
pub struct Point(pub f64, pub f64);
pub struct UserId(pub u64);
```

**Expected:** `Property test_crate::Point::0`, `Property test_crate::Point::1`, `Property test_crate::UserId::0`
**Actual:** Only the struct itself is extracted, no fields

**Root Cause:** The STRUCT_FIELD query only matches named fields (`field_declaration`), not positional fields in tuple structs.

**Fix Required:** Add a TUPLE_STRUCT_FIELD query that extracts from `(tuple_struct_pattern)` or similar AST nodes with numeric names.

---

### 3. Type Alias USES Relationships (test_type_aliases, test_type_alias_chains)

**Problem:** Type aliases don't extract USES relationships to their target types.

**Example:**
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

**Expected:** `USES test_crate::Result -> test_crate::Error`
**Actual:** No USES relationship extracted

**Root Cause:** The type alias handler doesn't have `relationships: extract_type_relationships` or similar.

---

### 4. UFCS Qualified Name Format (test_ufcs_explicit, test_trait_vs_inherent_method)

**Problem:** The expected qualified name format for trait impl methods is unclear.

**Example:**
```rust
impl Handler for MyHandler {
    fn handle(&self) { ... }
}

// Call site
<MyHandler as Handler>::handle(&handler);
```

**Questions:**
- Should the method FQN be `<MyHandler as Handler>::handle` or `MyHandler::handle`?
- When called via UFCS, should the CALLS relationship use the UFCS form or the simple form?
- How do we distinguish between inherent methods and trait methods with the same name?

**Current Implementation:** Trait impl methods use `<Type as Trait>::method` format
**Test Expectation:** Unclear/inconsistent

---

### 5. Trait Impl FQN Prefix Validation (test_contains_relationships_have_matching_entities)

**Problem:** CONTAINS relationship validation fails because trait impl FQNs use angle brackets.

**Error:**
```
CONTAINS child '<test_crate::MyHandler as test_crate::Handler>' should be prefixed by parent 'test_crate'
```

**Root Cause:** The validation logic checks if child FQN starts with parent FQN, but `<test_crate::MyHandler as test_crate::Handler>` doesn't start with `test_crate`.

**Design Decision Needed:** Either:
1. Change trait impl FQN format to not use angle brackets
2. Update validation logic to handle this case
3. Use a different qualified name format

---

### 6. Complex Entity Patterns (6 tests)

These tests fail due to various unimplemented features:

| Test | Issue |
|------|-------|
| test_associated_types | Associated type declarations not extracted |
| test_associated_types_resolution | Associated type resolution not implemented |
| test_blanket_impl | Blanket impl (`impl<T> Trait for T`) handling unclear |
| test_extension_traits | Extension trait method resolution |
| test_generic_bounds_resolution | Generic bound resolution (`T: Trait`) |
| test_generic_trait | Generic trait implementation handling |
| test_prelude_shadowing | Prelude type shadowing detection |
| test_scattered_impl_blocks | Multiple impl blocks for same type |
| test_extern_blocks | Extern block item extraction |
| test_complex_enums | Complex enum variant handling |

---

## TypeScript Issues

### 7. Function Expression Extraction (test_function_expressions)

**Problem:** Named function expressions assigned to variables aren't extracted correctly.

**Example:**
```typescript
const named = function namedFunction() { ... };
```

**Expected:** Function entity with appropriate name
**Actual:** May be extracted as Constant or not at all

---

### 8. Parameter Property Qualified Names (test_parameter_properties)

**Problem:** Constructor parameter properties have the constructor in their qualified name.

**Example:**
```typescript
class Point {
    constructor(public x: number, public y: number) {}
}
```

**Expected:** `point.Point.x`, `point.Point.y`
**Actual:** `point.Point.constructor.x`, `point.Point.constructor.y`

**Root Cause:** The `skip_scopes` field in handler config is not implemented. It should skip `method_definition` when building qualified names for parameter properties.

---

### 9. Index Signature Naming (test_index_signatures)

**Problem:** Index signatures use hardcoded `[index]` instead of the actual key type.

**Example:**
```typescript
interface StringMap {
    [key: string]: string;
}
```

**Expected:** Name like `[string]` or the actual parameter name
**Actual:** `[index]`

**Design Decision Needed:** What should the name be for index signatures?
- `[string]` / `[number]` (based on key type)
- `[key]` (actual parameter name)
- `[index]` (generic)

---

### 10. Constants vs Arrow Functions (test_constants_variables)

**Problem:** Some arrow functions are incorrectly classified as Constants.

**Root Cause:** The CONST query's `#not-match?` predicate doesn't exclude all arrow function patterns, particularly generic arrow functions starting with `<`.

---

### 11. Class Expression Extraction (test_class_expressions)

**Problem:** Class expressions assigned to variables may not be extracted correctly.

**Example:**
```typescript
const MyClass = class { ... };
```

---

## JavaScript Issues

### 12. Function Extraction (test_functions, test_classes)

**Problem:** Some function patterns in JavaScript aren't being extracted.

**Root Cause:** Query differences between TypeScript and JavaScript handling.

---

## Implementation Priority Recommendations

### High Priority (Core Functionality)
1. Method vs Property disambiguation (blocking builder pattern usage)
2. Type alias USES relationships (breaks type dependency tracking)
3. Function expression extraction (common JS/TS pattern)

### Medium Priority (Completeness)
4. Tuple struct fields (Rust completeness)
5. Parameter property skip_scopes (TypeScript classes)
6. Index signature naming (minor UX issue)

### Lower Priority (Edge Cases)
7. UFCS resolution (advanced Rust pattern)
8. Trait impl FQN format validation (test infrastructure)
9. Complex trait patterns (advanced Rust patterns)

---

## Open Design Questions

1. **Entity Disambiguation:** When two entities have the same qualified name but different types, how should they be distinguished?
   - In the qualified name itself?
   - Only in the entity ID?
   - Should they share a qualified name?

2. **Trait Impl FQN Format:** Should trait impl methods use:
   - `<Type as Trait>::method` (current UFCS style)
   - `Type::method` (simple style, loses trait info)
   - `Trait::method` (trait-centric style)
   - Something else?

3. **Handler Priority:** When multiple handlers match the same code, which should win?
   - First match?
   - Last match?
   - Most specific match?
   - Should both entities be created?
