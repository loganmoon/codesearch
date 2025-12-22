---
date: 2025-12-22T20:38:00-05:00
git_commit: 353c91ed7fe0778fde68eb02902674c53b9276ee
branch: feat/133-graph-validation
repository: codesearch
topic: "LSP Validation Qualified Name Mismatch"
tags: [research, codebase, lsp-validation, qualified-names, javascript, typescript]
status: complete
last_updated: 2025-12-22
---

# Research: LSP Validation Qualified Name Mismatch

**Date**: 2025-12-22T20:38:00-05:00
**Git Commit**: 353c91ed7fe0778fde68eb02902674c53b9276ee
**Branch**: feat/133-graph-validation
**Repository**: codesearch

## Research Question

We're seeing mismatches between LSP qualified names (e.g., `jotai.babel.plugin-react-refresh.reactRefreshPlugin`) and Neo4j qualified names (e.g., `tmp..tmpDcN3mA.website.src.pages.index`). Is this an issue in the validator code, or are there bugs in how we extract qualified names for TypeScript/JavaScript?

## Summary

**The mismatch is caused by bugs in the extraction code, not the validation code.** There are three distinct issues:

1. **Monorepo/Workspace Detection Gap**: npm/pnpm workspaces are not detected; only root `package.json` is parsed
2. **Fallback to Absolute File Paths**: When no package matches, qualified names fall back to full absolute file paths
3. **No npm Module Resolution**: LSP uses npm module resolution semantics; we use file-path-based resolution

## Detailed Findings

### 1. Manifest Detection Only Handles Root package.json

**Location**: `crates/core/src/project_manifest.rs:314-372`

The `try_parse_package_json()` function:
- Only reads the root `package.json`
- Checks source roots in order: `["src", "lib", "."]`
- Does NOT detect npm/pnpm/yarn workspaces
- Does NOT traverse subdirectories for nested package.json files

For monorepos like jotai:
```
jotai/
├── package.json          # name: "jotai", private: true
├── pnpm-workspace.yaml   # declares workspace packages
├── src/                  # Main source
└── website/
    ├── package.json      # name: "jotai-website"
    └── src/              # Website source
```

The detection finds:
- `name = "jotai"`
- `source_root = /tmp/tmpXXXXXX/src`

Files in `website/src/` do NOT match this `source_root`.

### 2. Package Lookup Failure for Subdirectory Files

**Location**: `crates/indexer/src/file_change_processor.rs:265-269`

```rust
let (package_name, source_root) = package_map
    .as_ref()
    .and_then(|m| m.find_package_for_file(&file_path))
    .map(|pkg| (Some(pkg.name.as_str()), Some(pkg.source_root.as_path())))
    .unwrap_or((None, None));
```

For a file like `/tmp/tmpXXXXXX/website/src/pages/index.tsx`:
- `find_package_for_file()` checks if `file_path.starts_with(source_root)`
- `/tmp/.../website/src/pages/index.tsx` does NOT start with `/tmp/.../src`
- Result: `package_name = None`, `source_root = None`

### 3. Fallback to Full Absolute File Path

**Location**: `crates/languages/src/common/module_utils.rs:24-52`

```rust
pub fn derive_qualified_name(
    file_path: &Path,
    source_root: Option<&Path>,
    separator: &str,
) -> String {
    let relative = source_root
        .and_then(|root| file_path.strip_prefix(root).ok())
        .unwrap_or(file_path);  // <-- Falls back to FULL file_path when no source_root!

    // ... iterates through path components
}
```

When `source_root` is `None`:
- `relative` = entire absolute path `/tmp/tmpXXXXXX/website/src/pages/index.tsx`
- Path components become: `tmp`, `tmpXXXXXX`, `website`, `src`, `pages`, `index`
- Final qualified name: `tmp.tmpXXXXXX.website.src.pages.index`

### 4. Comparison: Rust Workspace Detection vs Node.js

Rust workspaces ARE properly detected (`crates/core/src/project_manifest.rs:130-173`):
- Parses `[workspace]` section in root `Cargo.toml`
- Expands glob patterns from `members = ["crates/*"]`
- Parses each member's `Cargo.toml` individually
- Each crate gets its own `source_root`

Node.js workspaces are NOT detected:
- No parsing of `pnpm-workspace.yaml`, `package.json.workspaces`, or `lerna.json`
- No traversal of subdirectories to find nested `package.json` files

### 5. LSP vs Our Resolution Semantics

**LSP (typescript-language-server):**
- Uses npm module resolution algorithm
- Understands `node_modules` structure
- Follows `package.json` `exports` and `main` fields
- Resolves `jotai` -> `node_modules/jotai/...`
- Qualified names reflect package hierarchy: `jotai.babel.plugin-react-refresh.reactRefreshPlugin`

**Our System:**
- Uses file-path-based resolution
- No `node_modules` resolution
- No understanding of npm package exports
- Qualified names reflect file paths: `tmp.tmpXXXXXX.website.src.pages.index`

## Code References

### Where qualified names are built
- `crates/languages/src/common/entity_building.rs:77-145` - `extract_common_components()`
- `crates/languages/src/common/entity_building.rs:174-204` - `compose_qualified_name()`
- `crates/languages/src/common/module_utils.rs:24-52` - `derive_qualified_name()` (Module entities)
- `crates/languages/src/javascript/module_path.rs:22-47` - `derive_module_path()` (function entities)

### Where package context is determined
- `crates/core/src/project_manifest.rs:314-372` - `try_parse_package_json()`
- `crates/indexer/src/file_change_processor.rs:265-269` - Package lookup for files

### Validation engine (not the cause)
- `crates/lsp-validation/src/validation.rs:77-235` - `validate_relationships()`

## Architecture Insights

The qualified name system was designed with Rust as the primary use case:
1. Rust has explicit module declarations (`mod x;`)
2. Cargo workspaces have a standard structure
3. Module paths map directly to file paths

JavaScript/TypeScript has fundamentally different semantics:
1. No explicit module declarations - files ARE modules
2. npm packages can export arbitrary entry points
3. `node_modules` resolution is complex (package.json exports, main, etc.)
4. Workspaces have multiple incompatible formats (npm, pnpm, yarn, lerna)

## Recommended Fixes

### Short-term (Improve File Path Handling)

1. **Detect npm workspaces** in `try_parse_package_json()`:
   - Parse `package.json.workspaces` array
   - Parse `pnpm-workspace.yaml`
   - Recursively find `package.json` files in workspace directories

2. **Handle missing source_root gracefully**:
   - Instead of using full absolute path, use relative path from repo root
   - This would give `website.src.pages.index` instead of `tmp.tmpXXXXXX.website.src.pages.index`

### Medium-term (Improve Module Resolution)

3. **Add npm module resolution**:
   - Parse `node_modules` structure
   - Follow `package.json` exports
   - Build qualified names based on npm package hierarchy

### Long-term (Architectural)

4. **Consider dual qualified name system**:
   - File-path-based names for internal use (entity IDs, deduplication)
   - Module-resolution-based names for semantic matching (LSP validation)

## Open Questions

1. Should we prioritize npm workspace detection or relative-path fallback first?
2. How do we handle dynamic imports and re-exports in the npm module system?
3. Should validation use entity IDs (stable) or qualified names (semantic) for matching?
4. Do we need to distinguish between "internal" qualified names (file-based) and "external" qualified names (semantic)?

## Related Research

- GitHub Issue #133: Add exhaustive LSP validation for language relationship extraction
- PR #132: Added relative import resolution and Module entity extraction for JS/TS/Python
