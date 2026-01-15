# Comprehensive Architecture Analysis: Spec-Driven Extraction System

## Executive Summary

This report provides an in-depth analysis of the codesearch spec-driven extraction system, grounded in extensive research into comparable industry systems, tree-sitter capabilities, and academic literature on multi-language code analysis. The goal is to inform a thoughtful architectural discussion about what's working, what's problematic, and what paths forward exist.

**Key Findings:**

1. **The declarative approach has genuine strengths** that should be preserved: the YAML spec system provides a clean single source of truth, the NameStrategy pattern is elegant and extensible, and the relationship resolution pipeline is well-architected.

2. **The fundamental tension is real but nuanced**: Tree-sitter queries are excellent for pattern matching but cannot express semantic constraints. However, this is a known limitation that every comparable system addresses differently.

3. **Industry evidence shows there is no "zero per-language code" solution**: GitHub's stack-graphs TypeScript definition is ~6,300 lines of TSG DSL. Sourcegraph's SCIP uses separate language-specific indexers. The question is not whether language-specific complexity exists, but where it should live.

4. **The current architecture has made defensible choices** but may benefit from clearer abstraction boundaries between syntactic pattern matching and semantic analysis.

---

## Table of Contents

1. [Research Methodology](#1-research-methodology)
2. [Current System: Deep Analysis](#2-current-system-deep-analysis)
3. [What's Working Well](#3-whats-working-well)
4. [Identified Challenges](#4-identified-challenges)
5. [Tree-Sitter: Capabilities and Limitations](#5-tree-sitter-capabilities-and-limitations)
6. [Comparative Analysis: Industry Systems](#6-comparative-analysis-industry-systems)
7. [Academic Perspective](#7-academic-perspective)
8. [Architectural Options Analysis](#8-architectural-options-analysis)
9. [Synthesis and Discussion Points](#9-synthesis-and-discussion-points)
10. [Sources and References](#10-sources-and-references)

---

## 1. Research Methodology

This analysis is based on:

1. **Deep codebase exploration**: Complete examination of the extraction pipeline including `build.rs`, `engine.rs`, `qualified_name.rs`, `relationships.rs`, test infrastructure, and YAML specifications
2. **Industry system research**: Detailed study of GitHub Semantic, Sourcegraph SCIP, GitHub Stack Graphs, Google Kythe, rust-analyzer, and Universal Ctags
3. **Tree-sitter documentation**: Official docs, Rust bindings API, predicate system, and tree-sitter-graph DSL
4. **Academic literature**: Surveys on multi-language code analysis and declarative vs imperative tradeoffs
5. **Test failure analysis**: Examination of the 24 failing tests and their root causes

---

## 2. Current System: Deep Analysis

### 2.1 Architecture Overview

The system implements a three-phase extraction pipeline:

```
Phase 1: Build Time (build.rs)
├── Parse YAML specs (rust.yaml, typescript.yaml, javascript.yaml)
├── Generate query constants (pub mod queries { ... })
├── Generate HandlerConfig structs (pub const FREE_FUNCTION: HandlerConfig = ...)
└── Generate ALL_HANDLERS array

Phase 2: Runtime Extraction (engine.rs)
├── Compile tree-sitter queries from config.query
├── Execute queries on parsed AST
├── Apply NameStrategy to derive entity names
├── Expand qualified_name_template with captures
├── Extract metadata via MetadataExtractor dispatch
├── Extract relationships via RelationshipExtractor dispatch
└── Build CodeEntity with all components

Phase 3: Resolution (outbox-processor)
├── Cache all entities for repository
├── Run 9 relationship resolvers (Contains, Calls, Uses, Implements, etc.)
├── Apply LookupStrategy chains (QualifiedName → CallAliases → UniqueSimpleName)
├── Create external reference stub nodes
└── Mark repository as graph_ready
```

### 2.2 Key Abstractions

**HandlerConfig** encapsulates extraction rules:
```rust
pub struct HandlerConfig {
    pub entity_rule: &'static str,              // Rule ID (e.g., "E-FN-FREE")
    pub query: &'static str,                    // Tree-sitter query
    pub capture: &'static str,                  // Primary capture name
    pub name_strategy: NameStrategy,            // How to derive name
    pub qualified_name_template: Option<&'static str>,
    pub metadata_extractor: Option<MetadataExtractor>,
    pub relationship_extractor: Option<RelationshipExtractor>,
    pub visibility_override: Option<Visibility>,
}
```

**NameStrategy** handles name derivation:
```rust
pub enum NameStrategy {
    Capture { name: &'static str },
    Fallback { captures: &'static [&'static str] },
    Template { template: &'static str },
    Static { name: &'static str },
    FilePath,
    CrateName,
    PositionalIndex,
}
```

**QualifiedName** (recently introduced) provides semantic structure:
```rust
pub enum QualifiedName {
    SimplePath { segments: Vec<String>, separator: PathSeparator },
    InherentImpl { scope: Vec<String>, type_path: Vec<String> },
    TraitImpl { scope: Vec<String>, type_path: Vec<String>, trait_path: Vec<String> },
    TraitImplItem { type_path: Vec<String>, trait_path: Vec<String>, item_name: String },
    ExternBlock { scope: Vec<String>, linkage: String },
}
```

### 2.3 YAML Spec Structure

The YAML specifications serve multiple purposes:

1. **Documentation**: Entity rules (E-xxx), visibility rules (V-xxx), qualified name rules (Q-xxx)
2. **Code Generation Input**: `extraction_hints` section drives build.rs
3. **Test Specification**: Rules reference which fixtures validate them

Example from `rust.yaml`:
```yaml
queries:
  METHOD_IN_INHERENT_IMPL:
    description: "Methods with self parameter in inherent impl"
    capture: "method"
    query: |
      (impl_item
        type: [
          (type_identifier) @impl_type_name
          (generic_type type: (type_identifier) @impl_type_name)
        ]
        body: (declaration_list
          (function_item
            name: (identifier) @name
            parameters: (parameters . (self_parameter) @self_param)
          ) @method)) @impl
      (#not-has-child? @impl trait)

handlers:
  MethodInInherentImpl:
    entity_rule: E-METHOD-SELF
    query: METHOD_IN_INHERENT_IMPL
    name_strategy: capture
    qualified_name_template: "<{impl_type_name}>::{name}"
    metadata: method_metadata
    relationships: extract_function_relationships
```

---

## 3. What's Working Well

Based on comprehensive codebase analysis, these patterns are successful and should be preserved:

### 3.1 NameStrategy Enum

The `NameStrategy` pattern elegantly handles all name derivation cases:

| Strategy | Use Case | Example |
|----------|----------|---------|
| `Capture` | Standard entities | Functions, structs, methods |
| `Fallback` | Multiple possible captures | Try `@fn_name` then `@name` |
| `Template` | Computed names | `impl {impl_type_name}` |
| `Static` | Fixed names | Call signatures `()` |
| `FilePath` | File-derived modules | `src/utils.ts` → `utils` |
| `CrateName` | Package root | Crate root module |
| `PositionalIndex` | Positional fields | Tuple struct fields |

This abstraction cleanly separates the "how to get the name" concern from the extraction engine.

### 3.2 Entity ID with Type Differentiation

Recent fix (commit 93e2d50) solved a critical collision issue:

```
entity_id = hash(repository_id, file_path, qualified_name, entity_type)
```

Including `entity_type` prevents collisions for entities like:
- `ConfigBuilder::name` (Property) vs `ConfigBuilder::name` (Method)

This enables the builder pattern and method overloading without data loss.

### 3.3 QualifiedName Structured Type

Recent addition (commit c222cd0) provides semantic containment checking:

**Before**: `child_fqn.starts_with(parent_fqn)` — failed for trait impls
**After**: `child.is_child_of(&parent)` — semantic understanding of impl structure

This solved the `test_trait_impl` failure where `<crate::Type as crate::Trait>` doesn't start with `crate`.

### 3.4 Relationship Resolution Pipeline

The resolution system is well-architected:

1. **EntityCache**: Single database query loads all entities; shared across 9 resolvers
2. **LookupStrategy chains**: Configurable per relationship type
3. **GenericResolver**: Declarative RelationshipDef + pluggable ReferenceExtractor
4. **External reference deduplication**: HashSet prevents duplicate stub nodes

The `RelationshipDef` pattern is particularly clean:
```rust
pub const CALLS: RelationshipDef = RelationshipDef::new(
    "calls",
    CALLABLE_TYPES,
    CALLABLE_TYPES,
    RelationshipType::Calls,
    &[LookupStrategy::QualifiedName, LookupStrategy::CallAliases, LookupStrategy::UniqueSimpleName],
);
```

### 3.5 Import Map

Effective at resolving bare identifiers:
- Handles Rust `use` statements (simple, grouped, aliased, glob)
- Handles JS/TS imports (named, default, namespace)
- Relative import resolution (`.`, `..`)
- Folder module collapsing (`index.ts`)

### 3.6 Test Infrastructure

Comprehensive spec validation:
- 53 Rust fixtures, 45 TypeScript fixtures
- Subset matching (expected ⊆ actual) for forward compatibility
- Consistency tests validate fixture definitions without Docker
- RAII guards ensure cleanup on test failure

### 3.7 Build-Time Code Generation

The `build.rs` approach provides:
- Single source of truth (YAML specs)
- Compile-time validation (generated Rust code must compile)
- Deterministic output (sorted queries)
- IDE support for generated code

---

## 4. Identified Challenges

### 4.1 Tree-Sitter Predicate Handling

The YAML specs use predicates like `#not-has-child?` and `#not-eq?` that require manual evaluation.

**Current implementation in engine.rs:**
```rust
fn should_skip_match(config: &HandlerConfig, main_node: Node, captures: &HashMap<String, String>) -> bool {
    // Handle #not-has-child? for trait disambiguation
    if config.query.contains("#not-has-child?") && config.query.contains("trait") {
        if let Some(impl_node) = find_ancestor_of_kind(main_node, "impl_item") {
            if impl_node.child_by_field_name("trait").is_some() {
                return true;
            }
        }
    }
    // ... more special cases
}
```

**The issue**: This creates a gap between what the spec declares and what the engine actually evaluates. The spec says `#not-has-child? @params self_parameter`, but the engine only handles specific known patterns.

**Clarification on Rust bindings**: The tree-sitter Rust bindings do provide methods for predicate access:
- `Query::property_predicates(pattern_index)` - for `is?` and `is-not?`
- `Query::property_settings(pattern_index)` - for `set!`
- `Query::general_predicates(pattern_index)` - for other predicates

However, the **evaluation** of these predicates is still the caller's responsibility. The bindings expose predicates in structured form; they don't automatically filter matches.

### 4.2 Semantic vs Syntactic Boundaries

The spec attempts to declaratively encode semantic knowledge that inherently requires code:

| Challenge | Why It's Hard |
|-----------|---------------|
| Foreign type detection | `impl Trait for String` — is `String` local or external? |
| Method vs Function | Requires checking for `self` parameter OR `Self` return type |
| Visibility inheritance | Items in `pub mod` inherit visibility from ancestors |
| Import resolution | Requires building and querying import map |
| UFCS qualified names | `<Type as Trait>::method` has no standard format |

### 4.3 Test Failure Patterns

The 24 failing tests cluster into categories:

| Category | Tests | Nature of Problem |
|----------|-------|-------------------|
| Foreign type prefix | 2 | Semantic knowledge needed at extraction time |
| Method disambiguation | 3 | `self_parameter` check not fully implemented |
| Extern block containment | 1 | Parent scope derivation |
| Type alias USES | 2 | Relationship extraction not configured |
| Associated types | 2 | Feature gap in extraction |
| Generic bounds | 2 | Feature gap in extraction |
| UFCS format | 1 | Design decision needed |
| TS expressions | 3 | Query pattern gaps |
| Parameter properties | 1 | `skip_scopes` not implemented |
| Index signatures | 1 | Design decision on naming |
| JS patterns | 2 | Grammar differences |

### 4.4 Complexity Distribution

Despite declarative goals, substantial per-language code exists:

| Component | Rust | TypeScript |
|-----------|------|------------|
| YAML spec | ~1000 lines | ~800 lines |
| Relationship extractors | ~500 lines | ~400 lines |
| Edge case handlers | ~200 lines | ~150 lines |
| Test fixtures | ~2000 lines | ~1500 lines |

The declarative specs haven't eliminated language-specific code—they've reorganized where it lives.

---

## 5. Tree-Sitter: Capabilities and Limitations

### 5.1 What Tree-Sitter Excels At

From [GitHub's analysis](https://github.com/github/semantic/blob/main/docs/why-tree-sitter.md):

1. **Grammar-based parsing**: "You don't have to write a lot of complicated code to parse a language; you just write the grammar."
2. **Version flexibility**: Can parse "the union of supported versions" of a language
3. **Comment preservation**: Comments are "in the AST"
4. **Full source fidelity**: "Every bracket, colon, and semicolon"
5. **Incremental parsing**: Millisecond updates on edits
6. **Error recovery**: Meaningful results even with syntax errors

### 5.2 What Tree-Sitter Cannot Do

From official documentation and [Cycode's analysis](https://cycode.com/blog/tips-for-using-tree-sitter-queries/):

1. **Not semantic analysis**: "Tree-sitter is not a Language Server Protocol (LSP) implementation"
2. **File-local only**: No cross-file analysis
3. **CST not AST**: Preserves syntax, not semantics
4. **Query limitations**: "Queries are great for capturing text from code. But to extract anything moderately structured we need to traverse the syntax tree."

### 5.3 Predicate Reality

From [tree-sitter documentation](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/3-predicates-and-directives.html):

> "Predicates and directives are not handled directly by the Tree-sitter C library. They are just exposed in a structured form so that higher-level code can perform the filtering."

**Built-in predicates** (implemented by Rust/JS bindings):
- `#eq?` / `#not-eq?` - string equality
- `#match?` / `#not-match?` - regex matching
- `#any-of?` - membership testing

**NOT built-in**:
- `#not-has-child?` - structural child checks
- `#has-ancestor?` - structural ancestor checks
- Custom semantic predicates

### 5.4 Recursive/Nested Structure Limitation

From [Parsiya's analysis](https://parsiya.net/blog/knee-deep-tree-sitter-queries/):

> "How do we capture recursive types with tree-sitter queries? I don't know the answer."

The workaround is always: traverse the tree programmatically.

---

## 6. Comparative Analysis: Industry Systems

### 6.1 Sourcegraph SCIP

**Architecture**: Language-specific indexers that emit a common protocol.

From [SCIP DESIGN.md](https://github.com/sourcegraph/scip/blob/main/DESIGN.md):

> "Rather than using a graph-based representation where all semantic entities are nodes and relationships are edges, SCIP avoids this approach because it encourages wholesale indexers that are less likely to support parallelism."

**scip-typescript implementation** ([announcement blog](https://sourcegraph.com/blog/announcing-scip-typescript)):
- Built with TypeScript type checker
- "Indexing performance is largely bottlenecked by type checking performance"
- Uses TypeScript Compiler API for semantic analysis

**Indexer approach** (from [writing-an-indexer docs](https://sourcegraph.com/docs/code-navigation/writing-an-indexer)):
> "In the context of an indexer, this typically involves using a compiler frontend or a language server as a library. First, run the compiler pipeline until semantic analysis is completed. Next, perform a top-down traversal of ASTs for all files."

**Key insight**: SCIP explicitly accepts language-specific indexers. They optimize the **output format**, not the extraction logic.

### 6.2 GitHub Stack Graphs

**Architecture**: Tree-sitter-graph DSL for mapping AST to scope graphs.

From [introducing stack graphs](https://github.blog/open-source/introducing-stack-graphs/):

> "You use stanzas to define the gadget of graph nodes and edges that should be created for each occurrence of a Tree-sitter query."

**TypeScript complexity**:
- Stack-graphs TypeScript definition: **~6,297 lines of TSG DSL**
- Handles: modules, namespaces, classes, functions, types, generics, async/await, JSX

**TSG example**:
```tsg
(function_definition name: (identifier) @id) @func {
  node def
  attr (def) type = "pop_symbol", symbol = (source-text @id), source_node = @func, is_definition
}
```

**Key insight**: Even with a purpose-built DSL, the per-language definitions are extensive. The complexity is inherent in language semantics.

### 6.3 GitHub Semantic (Haskell)

**Architecture**: Typed Haskell AST with "data types à la carte."

From [CodeGen announcement](https://github.blog/engineering/architecture-optimization/codegen-semantics-improved-language-support-system/):

> "Generates per-language Haskell syntax types based on tree-sitter grammar definitions."

**Approach** (from [why-haskell.md](https://github.com/github/semantic/blob/main/docs/why-haskell.md)):
- Tree-sitter for parsing
- Generated Haskell types for each language's AST
- Abstract interpretation for program analysis
- À la carte syntax types for cross-language generalization

**Key insight**: They separate **parsing** (tree-sitter) from **semantic analysis** (typed Haskell). Raw tree-sitter CST is insufficient for their needs.

### 6.4 rust-analyzer

**Architecture**: Custom parser + Salsa incremental computation + HIR.

From [architecture docs](https://rust-analyzer.github.io/book/contributing/architecture.html):

> "The top-level hir crate is an API Boundary... It wraps ECS-style internal API into a more OO-flavored API."

**Key concepts**:
- **Syntax trees as value types**: "Fully determined by contents, doesn't store semantic info"
- **HIR for semantics**: "Bound to a particular crate instance... has cfg flags and features applied"
- **Salsa for incrementality**: "Typing inside a function's body never invalidates global derived data"

**Key insight**: High-quality code intelligence requires **deep language-specific investment**. rust-analyzer is ~200k lines of Rust code for one language.

### 6.5 Google Kythe

**Architecture**: Extractors + Language-specific indexers + Common graph schema.

From [Kythe overview](https://kythe.io/docs/kythe-overview.html):

> "A hub-and-spoke model reduces the overall work to integrate L languages, C clients, and B build systems from O(L×C×B) to O(L+C+B)."

**Indexer implementations**:
- C++: cxx_indexer
- Java: java_indexer.jar
- Go: go_indexer
- Proto: proto_indexer

**Key insight**: Kythe's efficiency comes from **standardizing output**, not extraction. Each language has dedicated indexer code.

### 6.6 Universal Ctags

**Architecture**: Multiple parser types with guest/host model.

From [ctags documentation](https://docs.ctags.io/):

> "Universal Ctags supports multiple types of parsers: Optlib Parsers (regex-based) and PEG Parsers (grammar-based)."

**Guest/host model**:
- Host parser detects embedded language regions
- Guest parsers handle those regions
- Example: HTML host, CSS/JS guests

**Key insight**: ctags prioritizes **breadth over depth**. Symbol extraction without full semantic analysis is achievable with pattern matching.

### 6.7 Comparison Matrix

| System | Parsing | Semantic Analysis | Per-Language Investment | Extraction Approach |
|--------|---------|-------------------|------------------------|---------------------|
| **Codesearch** | Tree-sitter | YAML + engine | Medium | Declarative specs + Rust helpers |
| **SCIP** | Language-specific | TypeScript/Java compiler APIs | High | Language-specific indexers |
| **Stack Graphs** | Tree-sitter | TSG DSL (~6k lines/lang) | High | Declarative DSL |
| **GitHub Semantic** | Tree-sitter | Typed Haskell | High | Generated types + analysis |
| **rust-analyzer** | Custom parser | Salsa + HIR | Very High | Single-language deep integration |
| **Kythe** | Build integration | Language-specific | High | Separate indexers |
| **Universal Ctags** | Regex/PEG | Pattern matching | Low-Medium | Breadth over depth |

**Conclusion**: There is no system that achieves high-fidelity semantic analysis without substantial per-language investment. The question is **where** that complexity lives and **how** it's organized.

---

## 7. Academic Perspective

### 7.1 Multi-Language Code Analysis Challenges

From [Multilingual Source Code Analysis: A Systematic Literature Review](https://www.researchgate.net/publication/317671792):

> "Different languages have different lexical, syntactical, and semantic rules that make thorough analysis difficult. They also offer different modularization and dependency mechanisms."

Research identified 46 issues across 13 software engineering domains, with most work in:
- Static source code analysis
- Program comprehension
- Cross-language link detection
- Security analysis

### 7.2 Declarative vs Imperative Tradeoffs

From [industry analysis](https://www.techtarget.com/searchapparchitecture/tip/A-brief-breakdown-of-declarative-vs-imperative-programming):

**Declarative advantages**:
- More concise and readable
- Easier to reason about
- Better for testability
- Abstracts implementation details

**Declarative limitations**:
- Can be less performant
- Steeper learning curve for complex cases
- May not handle all edge cases
- "The abstraction away from direct control... can introduce overhead"

**Practical recommendation**:
> "In practice, most large applications use a blend of imperative and declarative code."

### 7.3 Maintenance Burden Reality

From [ACM paper on polyglot transformation](https://dl.acm.org/doi/full/10.1145/3656429):

> "Imperative frameworks are often language-specific and rely heavily on underlying compiler infrastructure, resulting in an additional burden when automation must support multiple languages."

The paper advocates for DSLs that define "flow and dependencies between lightweight match-replace rules" — a hybrid approach.

---

## 8. Architectural Options Analysis

Based on research, here are the viable architectural directions:

### 8.1 Option A: Preserve and Refine Current Architecture

**Approach**: Keep YAML specs + build.rs generation, but be more explicit about what's declarative vs what requires code.

**Changes**:
1. Document which predicates are actually evaluated (not just declared)
2. Move complex semantic logic to explicit Rust functions referenced from YAML
3. Add validation that specs don't use unsupported predicates
4. Expand edge case handler system for language-specific patterns

**Pros**:
- Minimal disruption
- Preserves working patterns (NameStrategy, QualifiedName, resolution)
- Leverages existing test infrastructure

**Cons**:
- Doesn't fundamentally resolve declarative/semantic tension
- Gap between spec and implementation may grow

### 8.2 Option B: Adopt tree-sitter-graph / Stack Graphs

**Approach**: Replace YAML extraction with tree-sitter-graph DSL for graph construction.

**What this provides**:
- Purpose-built DSL for AST → graph transformation
- Stanzas with node creation, edge construction, attribute setting
- Scoped variables for passing context between stanzas
- Execution modes (strict, lazy)

**Example TSG stanza**:
```tsg
(function_item name: (identifier) @name) @func {
    node def
    attr (def) type = "definition"
    attr (def) symbol = (source-text @name)
    attr (def) source_node = @func
    edge ROOT_NODE -> def
}
```

**Pros**:
- Principled theoretical foundation (scope graphs)
- Handles name binding correctly
- Already proven for TypeScript, Python, Java

**Cons**:
- TypeScript definition is ~6k lines — not simpler
- New DSL to learn and maintain
- May require rethinking relationship resolution

### 8.3 Option C: Language-Specific Indexers (SCIP Model)

**Approach**: Accept that each language needs dedicated extraction code. Standardize the output format, not the extraction logic.

**Structure**:
```rust
trait LanguageIndexer {
    fn index_file(&self, tree: &Tree, source: &str, ctx: &IndexContext) -> Vec<CodeEntity>;
}

struct RustIndexer { /* Rust-specific extraction */ }
struct TypeScriptIndexer { /* Uses TypeScript compiler API */ }
```

**Pros**:
- Full expressiveness per language
- Can leverage language-specific tools (rust-analyzer APIs, TypeScript compiler)
- No workarounds for query limitations
- Easier debugging (it's just Rust code)

**Cons**:
- More per-language code
- Less "declarative" appearance
- Harder to add new languages quickly

### 8.4 Option D: Tiered Extraction Architecture

**Approach**: Explicitly define tiers of extraction fidelity with different implementation strategies per tier.

**Tier 1 - Structural (Declarative)**:
- Modules, functions, classes, structs, enums
- Uses tree-sitter queries + NameStrategy
- ~ctags level fidelity

**Tier 2 - Semantic (Hybrid)**:
- Methods vs functions, visibility, basic relationships
- Queries + Rust helper functions
- Import map integration

**Tier 3 - Advanced (Language-Specific)**:
- Trait impls, generics, complex inheritance
- Full Rust extraction functions
- Language server integration optional

**Pros**:
- Clear complexity budget per tier
- Declarative where it works
- Escape hatch for complex cases
- Progressive enhancement path

**Cons**:
- Need clear tier assignment criteria
- May duplicate patterns across tiers

### 8.5 Option E: Proc-Macro DSL

**Approach**: Replace YAML with Rust proc-macro DSL that generates extraction code at compile time.

**Example**:
```rust
define_extractor! {
    language: Rust,

    #[query = "(function_item name: (identifier) @name) @func"]
    entity Function {
        name: capture("name"),
        when: !has_self_param(@func),
        qualified_name: |ctx| format!("{}::{}", ctx.scope, ctx.name),
        relationships: extract_function_calls,
    }

    #[query = "(function_item name: (identifier) @name) @func"]
    entity Method {
        name: capture("name"),
        when: has_self_param(@func) || returns_self(@func),
        qualified_name: |ctx| format!("<{}>::{}", ctx.impl_type, ctx.name),
    }
}
```

**Pros**:
- Compile-time validation
- Full Rust expressiveness in closures
- IDE support (autocomplete, type checking)
- No runtime YAML parsing

**Cons**:
- Proc macro complexity
- Longer compile times
- DSL design is non-trivial
- Still need per-language rules

---

## 9. Synthesis and Discussion Points

### 9.1 Core Questions for Architectural Discussion

1. **What level of semantic fidelity do we need?**
   - ctags-level (symbols only)?
   - SCIP-level (precise navigation)?
   - rust-analyzer-level (full IDE support)?

2. **Where should language-specific complexity live?**
   - In declarative specs (current)?
   - In explicit Rust code (SCIP model)?
   - In a purpose-built DSL (stack-graphs model)?

3. **What's the right abstraction boundary?**
   - Current: Queries + templates + edge cases
   - Alternative: Queries for syntax, code for semantics

4. **How do we handle the "80/20" problem?**
   - 80% of entities are simple (functions, classes, structs)
   - 20% require semantic analysis (methods, impls, relationships)

### 9.2 What the Research Suggests

1. **No free lunch**: Every system that achieves high-fidelity analysis has substantial per-language code. The question is organization, not elimination.

2. **Tree-sitter is for syntax**: It's excellent at parsing and pattern matching. Semantic analysis requires additional layers.

3. **Declarative + escape hatches is common**: Most systems blend declarative patterns with imperative code for complex cases.

4. **Output format standardization works**: SCIP, Kythe, and our CodeEntity/Neo4j approach all standardize output while allowing varied extraction implementations.

### 9.3 Strengths to Preserve

Based on analysis, these current patterns are valuable:

1. **NameStrategy enum**: Clean, extensible, handles all cases
2. **QualifiedName structured type**: Semantic containment checking
3. **Entity ID with type**: Prevents collisions
4. **Relationship resolution pipeline**: Well-architected, configurable
5. **Build-time code generation**: Single source of truth
6. **Test infrastructure**: Comprehensive spec validation

### 9.4 Areas for Potential Improvement

1. **Predicate handling**: Make explicit which predicates are evaluated vs declared
2. **Semantic extraction**: Consider moving complex logic to explicit Rust functions
3. **Error feedback**: Better validation that specs don't exceed capabilities
4. **Documentation**: Clear boundary between what's declarative and what's code
5. **Tier definition**: Consider explicit extraction tiers with different guarantees

---

## 10. Sources and References

### Tree-Sitter
- [Tree-sitter Official Documentation](https://tree-sitter.github.io/tree-sitter/)
- [Predicates and Directives](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/3-predicates-and-directives.html)
- [tree-sitter Rust API - Query](https://docs.rs/tree-sitter/latest/tree_sitter/struct.Query.html)
- [tree-sitter-graph DSL](https://github.com/tree-sitter/tree-sitter-graph)
- [Tips for Using Tree-sitter Queries - Cycode](https://cycode.com/blog/tips-for-using-tree-sitter-queries/)
- [Tree-sitter Query Analysis - Parsiya](https://parsiya.net/blog/knee-deep-tree-sitter-queries/)

### Code Intelligence Systems
- [Sourcegraph SCIP](https://github.com/sourcegraph/scip)
- [SCIP Design Document](https://github.com/sourcegraph/scip/blob/main/DESIGN.md)
- [SCIP Announcement](https://sourcegraph.com/blog/announcing-scip)
- [scip-typescript](https://github.com/sourcegraph/scip-typescript)
- [Writing a SCIP Indexer](https://sourcegraph.com/docs/code-navigation/writing-an-indexer)
- [GitHub Stack Graphs](https://github.com/github/stack-graphs)
- [Introducing Stack Graphs - GitHub Blog](https://github.blog/open-source/introducing-stack-graphs/)
- [tree-sitter-stack-graphs Documentation](https://docs.rs/tree-sitter-stack-graphs)
- [GitHub Semantic](https://github.com/github/semantic)
- [Why Tree-sitter - GitHub Semantic](https://github.com/github/semantic/blob/main/docs/why-tree-sitter.md)
- [Why Haskell - GitHub Semantic](https://github.com/github/semantic/blob/main/docs/why-haskell.md)
- [CodeGen Announcement - GitHub Blog](https://github.blog/engineering/architecture-optimization/codegen-semantics-improved-language-support-system/)
- [Kythe Overview](https://kythe.io/docs/kythe-overview.html)
- [Writing a Kythe Indexer](https://kythe.io/docs/schema/writing-an-indexer.html)
- [Universal Ctags](https://github.com/universal-ctags/ctags)
- [Universal Ctags Documentation](https://docs.ctags.io/)

### rust-analyzer and Incremental Computation
- [rust-analyzer Architecture](https://rust-analyzer.github.io/book/contributing/architecture.html)
- [rust-analyzer Architecture - GitHub](https://github.com/rust-lang/rust-analyzer/blob/main/docs/dev/architecture.md)
- [HIR Documentation](https://rust-lang.github.io/rust-analyzer/hir/index.html)
- [Salsa Framework](https://github.com/salsa-rs/salsa)
- [Salsa Overview](https://salsa-rs.github.io/salsa/overview.html)
- [Salsa Algorithm Explained](https://medium.com/@eliah.lakhin/salsa-algorithm-explained-c5d6df1dd291)

### Rust Metaprogramming
- [Procedural Macros Reference](https://doc.rust-lang.org/reference/procedural-macros.html)
- [Rust Macros - Earthly Blog](https://earthly.dev/blog/rust-macros/)
- [Creating DSLs in Rust](https://softwarepatternslexicon.com/rust/metaprogramming-and-macros/creating-domain-specific-languages-dsls/)
- [Guide to Rust Proc Macros - developerlife](https://developerlife.com/2022/03/30/rust-proc-macro/)

### Academic Literature
- [Multilingual Source Code Analysis: A Systematic Literature Review](https://www.researchgate.net/publication/317671792)
- [A Lightweight Polyglot Code Transformation Language - ACM](https://dl.acm.org/doi/full/10.1145/3656429)
- [Static Code Analysis of Multilanguage Software Systems - arXiv](https://arxiv.org/abs/1906.00815)

### Language Server Protocol
- [LSP Specification 3.17](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
- [Semantic Tokens Guide - VS Code](https://code.visualstudio.com/api/language-extensions/semantic-highlight-guide)

---

## Appendix A: Glossary

| Term | Definition |
|------|------------|
| **CST** | Concrete Syntax Tree - preserves all syntax details including whitespace |
| **AST** | Abstract Syntax Tree - semantic structure without syntactic noise |
| **HIR** | High-Level Intermediate Representation - resolved semantic view |
| **FQN** | Fully Qualified Name - complete path to an entity |
| **UFCS** | Universal Function Call Syntax - `<Type>::method` or `<Type as Trait>::method` |
| **TSG** | Tree-sitter Graph DSL - language for AST → graph transformation |
| **SCIP** | SCIP Code Intelligence Protocol - Sourcegraph's indexing format |
| **LSIF** | Language Server Index Format - predecessor to SCIP |
| **LSP** | Language Server Protocol - editor-server communication standard |
| **Salsa** | Incremental computation framework used by rust-analyzer |

## Appendix B: Test Failure Summary

| Test | Category | Root Cause | Potential Fix |
|------|----------|------------|---------------|
| `test_extension_traits` | Foreign type | Type origin detection | External type list |
| `test_blanket_impl` | Foreign type | Type origin detection | External type list |
| `test_extern_blocks` | Containment | Parent scope derivation | Extern block handling |
| `test_tuple_and_unit_structs` | Query gap | Missing tuple field query | Add query pattern |
| `test_type_aliases` | Relationships | No USES extraction | Configure extractor |
| `test_type_alias_chains` | Relationships | No USES extraction | Configure extractor |
| `test_builder_pattern` | Disambiguation | Method/function classification | Structural check |
| `test_trait_vs_inherent_method` | Disambiguation | Method/function classification | Structural check |
| `test_scattered_impl_blocks` | Disambiguation | Method/function classification | Structural check |
| `test_associated_types` | Feature gap | Not implemented | Add extraction |
| `test_associated_types_resolution` | Feature gap | Not implemented | Add extraction |
| `test_generic_bounds_resolution` | Feature gap | Not implemented | Add extraction |
| `test_generic_trait` | Feature gap | Not implemented | Add extraction |
| `test_ufcs_explicit` | Design decision | QN format undefined | Define format |
| `test_complex_enums` | Feature gap | Incomplete handling | Extend extraction |
| `test_prelude_shadowing` | Feature gap | Not implemented | Add detection |
| `test_function_expressions` | Query gap | TS expression patterns | Fix queries |
| `test_class_expressions` | Query gap | TS expression patterns | Fix queries |
| `test_constants_variables` | Query gap | TS patterns | Fix queries |
| `test_parameter_properties` | Feature gap | skip_scopes not implemented | Implement feature |
| `test_index_signatures` | Design decision | Naming convention | Define convention |
| `test_type_usage` | Relationships | Extraction gap | Configure extractor |
| `test_functions` (JS) | Query gap | JS grammar differences | Adjust queries |
| `test_classes` (JS) | Query gap | JS grammar differences | Adjust queries |
