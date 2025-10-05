//! Entity ID generation utilities for unique, deterministic IDs

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

/// Generate a unique entity ID based on repository, file path, and qualified name
///
/// Includes file_path to ensure entities in different files are always unique,
/// even if they have the same qualified name.
pub fn generate_entity_id(repository_id: &str, file_path: &str, qualified_name: &str) -> String {
    let unique_str = format!("{repository_id}:{file_path}:{qualified_name}");
    format!(
        "entity-{:032x}",
        XxHash3_128::oneshot(unique_str.as_bytes())
    )
}

/// Generate anonymous entity ID with location-based uniqueness
pub fn generate_anonymous_entity_id(
    repository_id: &str,
    qualified_name: &str,
    anonymous_index: usize,
    start_line: usize,
    start_column: usize,
    entity_type: &str,
) -> String {
    let unique_str = format!(
        "{repository_id}:{qualified_name}:L{start_line}:C{start_column}:{entity_type}:anon-{anonymous_index}"
    );
    format!(
        "entity-anon-{:032x}",
        XxHash3_128::oneshot(unique_str.as_bytes())
    )
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
        let repo_id = "test-repo-uuid";

        // Named entity
        let id1 = generate_entity_id(repo_id, "src/module.rs", "module::my_function");
        assert!(id1.starts_with("entity-"));
        assert!(!id1.contains("anon"));

        // Same qualified name and file path produces same ID (stable)
        let id2 = generate_entity_id(repo_id, "src/module.rs", "module::my_function");
        assert_eq!(id1, id2);

        // Different qualified name produces different ID
        let id3 = generate_entity_id(repo_id, "src/module.rs", "module::other_function");
        assert_ne!(id1, id3);

        // Different repository produces different ID
        let id4 = generate_entity_id("other-repo-uuid", "src/module.rs", "module::my_function");
        assert_ne!(id1, id4);

        // Different file path produces different ID (even with same qualified name)
        let id5 = generate_entity_id(repo_id, "src/other.rs", "module::my_function");
        assert_ne!(id1, id5);
    }

    #[test]
    fn test_anonymous_entity_id_generation() {
        let repo_id = "test-repo-uuid";

        // Anonymous entity with location
        let id1 = generate_anonymous_entity_id(repo_id, "module", 0, 10, 5, "function");
        assert!(id1.starts_with("entity-anon-"));

        // Different index produces different ID
        let id2 = generate_anonymous_entity_id(repo_id, "module", 1, 10, 5, "function");
        assert_ne!(id1, id2);

        // Different location produces different ID
        let id3 = generate_anonymous_entity_id(repo_id, "module", 0, 20, 5, "function");
        assert_ne!(id1, id3);
    }
}
