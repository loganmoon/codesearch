# String Parsing Elimination Plan

## Overview

This document outlines a plan to replace manual string parsing with tree-sitter queries and typed metadata throughout the codebase. The audit identified **~40 instances** of problematic string manipulation across 5 files.

## Audit Summary

| File | Instances | Priority |
|------|-----------|----------|
| `import_resolution.rs` | 26 | High |
| `neo4j_relationship_resolver.rs` | 6 | High |
| `generic_resolver.rs` | 4 | Medium |
| `common.rs` | 4 | Medium |
| `processor.rs` | 1 | None (OK) |

## Root Causes

The string parsing falls into 5 categories:

1. **Path Segment Manipulation** (~15 instances)
   - `split("::")`, `rsplit("::")`, `join("::")`
   - Caused by storing paths as `"::"` strings instead of `Vec<String>`

2. **Path Prefix Detection** (~12 instances)
   - `starts_with("crate::")`, `strip_prefix("self::")`, etc.
   - Caused by lack of typed `PathKind` enum

3. **UFCS Pattern Parsing** (~6 instances)
   - `find(" as ")`, `find(">::")`, string slicing
   - Caused by not extracting UFCS components at AST time

4. **Simple Name Extraction** (~4 instances)
   - `rsplit("::").next()`, `rsplit('.').next()`
   - Caused by lack of pre-computed `simple_name` field

5. **Generic Stripping** (~3 instances)
   - `split('<').next()`
   - Caused by not using tree-sitter to exclude type arguments

## Solution: New Typed Structures

### 1. Structured Path Representation

```rust
/// A structured representation of a Rust path
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredPath {
    /// Path kind (determines prefix handling)
    pub kind: PathKind,
    /// Path segments without separators
    pub segments: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// Absolute path (e.g., `std::collections::HashMap`)
    Absolute,
    /// Crate-relative path (e.g., `crate::module::Type`)
    Crate,
    /// Self-relative path (e.g., `self::submodule::Type`)
    SelfRelative,
    /// Super-relative path (e.g., `super::sibling::Type`)
    Super { levels: u32 },
    /// External dependency path (e.g., `external::serde::Serialize`)
    External,
}

impl StructuredPath {
    /// Get the simple (unqualified) name
    pub fn simple_name(&self) -> Option<&str> {
        self.segments.last().map(|s| s.as_str())
    }

    /// Get the first segment (package/crate name for absolute paths)
    pub fn first_segment(&self) -> Option<&str> {
        self.segments.first().map(|s| s.as_str())
    }

    /// Convert to qualified string (for backward compatibility)
    pub fn to_qualified_string(&self) -> String {
        let prefix = match self.kind {
            PathKind::Absolute => "",
            PathKind::Crate => "crate::",
            PathKind::SelfRelative => "self::",
            PathKind::Super { levels } => {
                return format!("{}{}",
                    "super::".repeat(levels as usize),
                    self.segments.join("::"))
            },
            PathKind::External => "external::",
        };
        format!("{}{}", prefix, self.segments.join("::"))
    }
}
```

### 2. Enhanced SourceReference

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReference {
    /// The full target reference
    pub target: String,

    /// Pre-computed simple name (last segment)
    pub simple_name: String,

    /// Source location in file
    pub location: SourceLocation,

    /// Reference type (Call, TypeUsage, Import, etc.)
    pub ref_type: ReferenceType,

    /// Whether this references an external dependency
    pub is_external: bool,
}
```

### 3. UFCS Call Structure

```rust
/// Represents a UFCS call like `<Type as Trait>::method`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UfcsCall {
    /// The implementing type (e.g., `Data`)
    pub impl_type: StructuredPath,
    /// The trait being called through (e.g., `Processor`)
    pub trait_path: StructuredPath,
    /// The method name (e.g., `process`)
    pub method_name: String,
}

impl UfcsCall {
    /// Convert to the canonical qualified name format
    pub fn to_qualified_string(&self) -> String {
        format!("<{} as {}>::{}",
            self.impl_type.to_qualified_string(),
            self.trait_path.to_qualified_string(),
            self.method_name)
    }
}
```

### 4. Enhanced EntityRelationshipData

```rust
pub struct EntityRelationshipData {
    // Existing fields...
    pub calls: Vec<SourceReference>,
    pub uses_types: Vec<SourceReference>,
    pub imports: Vec<String>,

    // NEW: Supertraits split by type
    pub supertraits: Vec<StructuredPath>,  // Trait bounds only
    // Lifetimes excluded at extraction time

    // NEW: UFCS calls tracked separately
    pub ufcs_calls: Vec<UfcsCall>,

    // Existing...
    pub call_aliases: Vec<String>,
}
```

## Implementation Phases

### Phase 1: Add `simple_name` to SourceReference

**Files to modify:**
- `crates/core/src/entities.rs` - Add field
- `crates/languages/src/rust/handler_impls/*.rs` - Populate at extraction
- `crates/outbox-processor/src/generic_resolver.rs` - Use field instead of rsplit

**Eliminates:** 4 instances of `rsplit("::")` in resolver

### Phase 2: Add `is_external` to SourceReference

**Files to modify:**
- `crates/core/src/entities.rs` - Add field
- `crates/languages/src/rust/import_resolution.rs` - Set during resolution
- `crates/outbox-processor/src/neo4j_relationship_resolver.rs` - Use field

**Eliminates:** 3 instances of `starts_with("external::")`

### Phase 3: Filter lifetimes at extraction time

**Files to modify:**
- `crates/languages/src/rust/handler_impls/type_handlers.rs` - Tree-sitter query to exclude lifetimes

**Tree-sitter query change:**
```scm
; Current: captures all trait_bound nodes including lifetimes
(trait_bound) @supertrait

; New: only capture trait bounds, not lifetime bounds
(trait_bound
  (type_identifier) @supertrait)
```

**Eliminates:** 1 instance of `starts_with('\'')` in resolver

### Phase 4: Extract UFCS components at AST time

**Files to modify:**
- `crates/languages/src/rust/handler_impls/common.rs` - New tree-sitter query for UFCS
- `crates/core/src/entities.rs` - Add `UfcsCall` struct
- `crates/languages/src/rust/import_resolution.rs` - Remove manual UFCS parsing

**Tree-sitter query for UFCS:**
```scm
(call_expression
  function: (scoped_identifier
    path: (bracketed_type
      type: (qualified_type
        type: (_) @impl_type
        alias: (_) @trait))
    name: (identifier) @method))
```

**Eliminates:** 6 instances of UFCS string parsing

### Phase 5: Structured path representation (Major)

**Files to modify:**
- `crates/core/src/entities.rs` - Add `StructuredPath`, `PathKind`
- `crates/languages/src/common/import_map.rs` - Store structured paths
- `crates/languages/src/rust/import_resolution.rs` - Rewrite path handling
- All handler files - Use structured paths

**Eliminates:** ~15 instances of `split("::")`, `join("::")`, `strip_prefix`

## Recommended Order

1. **Phase 1 & 2** - Quick wins, minimal risk
2. **Phase 3** - Simple tree-sitter query change
3. **Phase 4** - Moderate complexity, high impact
4. **Phase 5** - Major refactor, defer to separate PR

## Acceptance Criteria

- All E2E tests pass
- No `split("::")` or `rsplit("::")` in outbox-processor
- No `find(" as ")` or UFCS string parsing
- No `starts_with('\'')` for lifetime filtering
- All generics stripped via tree-sitter, not string ops
