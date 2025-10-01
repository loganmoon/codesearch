//! Entity ID generation utilities for unique, deterministic IDs

use crate::entities::EntityType;
use std::path::Path;
use twox_hash::XxHash3_128;

/// Context for tracking scope during AST traversal
#[derive(Debug, Clone)]
pub struct ScopeContext {
    /// Stack of scope names from root to current position
    pub scope_stack: Vec<String>,
    /// Counter for unnamed entities within current scope
    pub anonymous_counter: usize,
}

impl ScopeContext {
    /// Create a new root scope context
    pub fn new() -> Self {
        Self {
            scope_stack: Vec::new(),
            anonymous_counter: 0,
        }
    }

    /// Push a new named scope onto the stack
    pub fn push_scope(&mut self, name: String) {
        self.scope_stack.push(name);
        self.anonymous_counter = 0; // Reset counter for new scope
    }

    /// Pop the current scope from the stack
    pub fn pop_scope(&mut self) {
        self.scope_stack.pop();
        self.anonymous_counter = 0; // Reset counter when leaving scope
    }

    /// Get the next anonymous entity index and increment counter
    pub fn next_anonymous_index(&mut self) -> usize {
        let index = self.anonymous_counter;
        self.anonymous_counter += 1;
        index
    }

    /// Build a fully qualified name from the current scope
    pub fn build_qualified_name(&self, name: &str) -> String {
        if self.scope_stack.is_empty() {
            name.to_string()
        } else {
            format!("{}::{}", self.scope_stack.join("::"), name)
        }
    }

    /// Get the current scope path as a string
    pub fn current_scope_path(&self) -> String {
        if self.scope_stack.is_empty() {
            "root".to_string()
        } else {
            self.scope_stack.join("::")
        }
    }
}

impl Default for ScopeContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a unique entity ID based on qualified name
///
/// For named entities, uses the qualified name from the scope context.
/// The qualified name should already be built using `build_qualified_name`.
pub fn generate_entity_id_from_qualified_name(
    qualified_name: &str,
    file_path: &Path,
) -> String {
    let unique_str = format!("{}:{}", file_path.display(), qualified_name);
    format!(
        "entity-{:032x}",
        XxHash3_128::oneshot(unique_str.as_bytes())
    )
}

/// Generate a unique entity ID based on name and context (with mutable context for anonymous entities)
pub fn generate_entity_id(
    name: Option<&str>,
    entity_type: EntityType,
    file_path: &Path,
    start_line: usize,
    start_column: usize,
    scope_context: &mut ScopeContext,
) -> String {
    match name {
        Some(n) if !n.is_empty() && n != "anonymous" => {
            // Named entity: use fully qualified name
            let qualified_name = scope_context.build_qualified_name(n);
            generate_entity_id_from_qualified_name(&qualified_name, file_path)
        }
        _ => {
            // Unnamed entity: use location + context + index
            let index = scope_context.next_anonymous_index();
            let unique_str = format!(
                "{}:L{}:C{}:{}:{}:anon-{}",
                file_path.display(),
                start_line,
                start_column,
                entity_type,
                scope_context.current_scope_path(),
                index
            );
            format!(
                "entity-anon-{:032x}",
                XxHash3_128::oneshot(unique_str.as_bytes())
            )
        }
    }
}

/// Parameters for generating entity IDs with custom separators
pub struct EntityIdParams<'a> {
    pub name: Option<&'a str>,
    pub entity_type: EntityType,
    pub file_path: &'a Path,
    pub start_line: usize,
    pub start_column: usize,
    pub scope_path: &'a str,
    pub separator: &'a str,
    pub anonymous_index: usize,
}

/// Generate entity ID for language-specific scope separators
pub fn generate_entity_id_with_separator(params: EntityIdParams) -> String {
    let EntityIdParams {
        name,
        entity_type,
        file_path,
        start_line,
        start_column,
        scope_path,
        separator,
        anonymous_index,
    } = params;
    match name {
        Some(n) if !n.is_empty() && n != "anonymous" => {
            // Named entity: use fully qualified name with custom separator
            let qualified_name = if scope_path.is_empty() {
                n.to_string()
            } else {
                format!("{scope_path}{separator}{n}")
            };
            let unique_str = format!("{}:{}", file_path.display(), qualified_name);
            format!(
                "entity-{:032x}",
                XxHash3_128::oneshot(unique_str.as_bytes())
            )
        }
        _ => {
            // Unnamed entity: use location + context + index
            let unique_str = format!(
                "{}:L{}:C{}:{}:{}:anon-{}",
                file_path.display(),
                start_line,
                start_column,
                entity_type,
                scope_path,
                anonymous_index
            );
            format!(
                "entity-anon-{:032x}",
                XxHash3_128::oneshot(unique_str.as_bytes())
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_context() {
        let mut ctx = ScopeContext::new();
        assert_eq!(ctx.current_scope_path(), "root");

        ctx.push_scope("module".to_string());
        assert_eq!(ctx.current_scope_path(), "module");
        assert_eq!(ctx.build_qualified_name("function"), "module::function");

        ctx.push_scope("class".to_string());
        assert_eq!(ctx.current_scope_path(), "module::class");
        assert_eq!(ctx.build_qualified_name("method"), "module::class::method");

        ctx.pop_scope();
        assert_eq!(ctx.current_scope_path(), "module");
    }

    #[test]
    fn test_anonymous_counter() {
        let mut ctx = ScopeContext::new();
        assert_eq!(ctx.next_anonymous_index(), 0);
        assert_eq!(ctx.next_anonymous_index(), 1);
        assert_eq!(ctx.next_anonymous_index(), 2);

        ctx.push_scope("function".to_string());
        assert_eq!(ctx.next_anonymous_index(), 0); // Reset in new scope
        assert_eq!(ctx.next_anonymous_index(), 1);

        ctx.pop_scope();
        assert_eq!(ctx.next_anonymous_index(), 0); // Reset when leaving scope
    }

    #[test]
    fn test_entity_id_generation() {
        let mut ctx = ScopeContext::new();
        let path = Path::new("/src/main.rs");

        // Named entity
        ctx.push_scope("module".to_string());
        let id1 = generate_entity_id(
            Some("my_function"),
            EntityType::Function,
            path,
            10,
            5,
            &mut ctx,
        );
        assert!(id1.starts_with("entity-"));
        assert!(!id1.contains("anon"));

        // Anonymous entity
        let id2 = generate_entity_id(None, EntityType::Function, path, 20, 10, &mut ctx);
        assert!(id2.starts_with("entity-anon-"));

        // Same location but different anonymous index should give different ID
        let id3 = generate_entity_id(None, EntityType::Function, path, 20, 10, &mut ctx);
        assert_ne!(id2, id3);

        // Empty name should be treated as anonymous
        let id4 = generate_entity_id(Some(""), EntityType::Function, path, 30, 15, &mut ctx);
        assert!(id4.starts_with("entity-anon-"));
    }

    #[test]
    fn test_language_specific_separator() {
        let path = Path::new("/src/main.py");

        // Python-style qualified name
        let id = generate_entity_id_with_separator(EntityIdParams {
            name: Some("method"),
            entity_type: EntityType::Function,
            file_path: path,
            start_line: 10,
            start_column: 5,
            scope_path: "MyClass",
            separator: ".",
            anonymous_index: 0,
        });
        assert!(id.starts_with("entity-"));

        // JavaScript-style qualified name
        let id2 = generate_entity_id_with_separator(EntityIdParams {
            name: Some("method"),
            entity_type: EntityType::Function,
            file_path: Path::new("/src/main.js"),
            start_line: 10,
            start_column: 5,
            scope_path: "MyClass.prototype",
            separator: ".",
            anonymous_index: 0,
        });
        assert!(id2.starts_with("entity-"));
        assert_ne!(id, id2); // Different paths should give different IDs
    }
}
