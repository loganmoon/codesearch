# AST Query Simplification: Language-Agnostic Patterns

## Problem Statement

Currently, each language in the codesearch system requires its own complete set of tree-sitter queries, even for universal programming concepts like functions, classes, and methods. This leads to significant code duplication and maintenance overhead.

For example, a function is represented as:
- `function_item` in Rust
- `function_declaration` in JavaScript/TypeScript/Go
- `function_definition` in Python
- `method_declaration` in Java

Despite these different names, the extraction logic is often identical: get the name, parameters, return type, and body.

## Research Findings

Tree-sitter grammars follow reasonably consistent patterns:
- Most use either `_declaration` or `_definition` suffixes
- Body nodes typically use `_body` suffix
- All use `identifier` for names
- Parameters and return types follow similar structures

## Proposed Solutions

### Option 1: Multi-Pattern Queries

Create queries that match multiple node types:

```rust
const UNIVERSAL_FUNCTION_QUERY: &str = r#"
[
  (function_item)        ; Rust
  (function_declaration) ; JavaScript, Go, TypeScript  
  (function_definition)  ; Python
  (method_declaration)   ; Java
] @function
"#;
```

**Pros:**
- Single query works across languages
- Minimal code changes needed

**Cons:**
- Query becomes large with many languages
- May impact performance
- Still need language detection

### Option 2: Language Configuration with Mapping

Define mappings from concepts to node names:

```rust
trait LanguageConfig {
    fn function_nodes(&self) -> &[&str];
    fn class_nodes(&self) -> &[&str];
    fn method_nodes(&self) -> &[&str];
}

struct RustConfig;
impl LanguageConfig for RustConfig {
    fn function_nodes(&self) -> &[&str] { &["function_item"] }
    fn class_nodes(&self) -> &[&str] { &["struct_item", "enum_item"] }
    fn method_nodes(&self) -> &[&str] { &["function_item"] }
}
```

**Pros:**
- Clean separation of concerns
- Easy to add new languages
- Type-safe configuration

**Cons:**
- Requires refactoring existing code
- Additional abstraction layer

### Option 3: Query Templates

Generate queries from templates:

```rust
fn build_function_query(node_name: &str) -> String {
    format!(r#"
({node_name}
  name: (identifier) @name
  parameters: (_) @params
  body: (_) @body
) @function
"#, node_name = node_name)
}

// Usage:
let rust_query = build_function_query("function_item");
let python_query = build_function_query("function_definition");
```

**Pros:**
- Flexible and extensible
- Can handle language-specific variations
- Reduces duplication while maintaining performance

**Cons:**
- Runtime query generation
- Need to compile queries at initialization

## Implementation Strategy

### Phase 1: Proof of Concept
1. Create a `UniversalQueries` module
2. Implement multi-pattern queries for functions only
3. Test performance impact across languages

### Phase 2: Full Implementation
1. Choose best approach based on POC results
2. Create language configuration system
3. Refactor existing extractors to use shared queries
4. Add comprehensive tests

### Phase 3: Optimization
1. Cache compiled queries
2. Benchmark different approaches
3. Add query validation

## Benefits

1. **Reduced Code Duplication**: Write extraction logic once
2. **Easier Maintenance**: Bug fixes and features in one place
3. **Faster Language Addition**: New languages just need node mappings
4. **Consistent Behavior**: All languages extract same information
5. **Better Testing**: Test extraction logic independently from queries

## Challenges

1. **Language-Specific Features**:
   - Rust's impl blocks
   - JavaScript's multiple function types
   - Python's decorators
   - Go's receivers

2. **Performance Considerations**:
   - Multi-pattern queries may be slower
   - Need to benchmark thoroughly

3. **Backwards Compatibility**:
   - Existing extractors must continue working
   - Migration path needed

## Example: Universal Function Extractor

```rust
pub struct UniversalFunctionExtractor {
    patterns: HashMap<Language, Vec<&'static str>>,
}

impl UniversalFunctionExtractor {
    pub fn new() -> Self {
        let mut patterns = HashMap::new();
        patterns.insert(Language::Rust, vec!["function_item"]);
        patterns.insert(Language::Python, vec!["function_definition"]);
        patterns.insert(Language::JavaScript, vec!["function_declaration", "arrow_function"]);
        Self { patterns }
    }
    
    pub fn build_query(&self, language: Language) -> String {
        let nodes = self.patterns.get(&language).unwrap();
        let alternatives = nodes.join("\n  ");
        format!("[\n  {}\n] @function", alternatives)
    }
}
```

## Conclusion

While the current language-specific approach works, a more unified system would significantly reduce maintenance burden and make the codebase more approachable. The generic extractor framework already provides a solid foundation - we just need to make the queries themselves more reusable.

This optimization should be considered after the current refactoring is complete and stable.