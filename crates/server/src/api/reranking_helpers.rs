//! Helper functions for reranking operations

use codesearch_core::CodeEntity;

const DELIM: &str = " ";

/// Extract content from entity for embedding
///
/// This extracts the most relevant text content from an entity for semantic operations.
pub fn extract_embedding_content(entity: &CodeEntity) -> String {
    // Calculate accurate capacity
    let estimated_size = entity.name.len()
        + entity.qualified_name.len()
        + entity.documentation_summary.as_ref().map_or(0, |s| s.len())
        + entity.content.as_ref().map_or(0, |s| s.len())
        + 100; // Extra padding for delimiters and formatting

    let mut content = String::with_capacity(estimated_size);

    // Add entity type and name
    content.push_str(&format!("{} {}", entity.entity_type, entity.name));
    chain_delim(&mut content, &entity.qualified_name);

    // Add documentation summary if available
    if let Some(doc) = &entity.documentation_summary {
        chain_delim(&mut content, doc);
    }

    // Add signature information for functions/methods
    if let Some(sig) = &entity.signature {
        for (name, type_opt) in &sig.parameters {
            content.push_str(DELIM);
            content.push_str(name);
            if let Some(param_type) = type_opt {
                content.push_str(": ");
                content.push_str(param_type);
            }
        }

        if let Some(ret_type) = &sig.return_type {
            chain_delim(&mut content, &format!("-> {ret_type}"));
        }
    }

    // Add the full entity content (most important for semantic search)
    if let Some(entity_content) = &entity.content {
        chain_delim(&mut content, entity_content);
    }

    content
}

fn chain_delim(out_str: &mut String, text: &str) {
    out_str.push_str(DELIM);
    out_str.push_str(text);
}

/// Prepare documents for reranking
///
/// Takes a reference to (id, content) pairs and returns borrowed references
/// suitable for passing to the reranker API.
///
/// # Memory Allocation Note
///
/// This function creates a new vector with cloned IDs and borrowed content references.
/// This allocation is necessary because:
/// - The reranker API requires `&[(String, &str)]`
/// - IDs must be owned to ensure they outlive the function call
/// - Content is borrowed from `entity_contents` to avoid an extra string clone
///
/// To fully eliminate this allocation, the reranker API would need to accept
/// owned strings directly or use a different data structure (e.g., `&[(id, content)]`).
/// This is a known trade-off between API ergonomics and memory efficiency.
pub fn prepare_documents_for_reranking(
    entity_contents: &[(String, String)],
) -> Vec<(String, &str)> {
    // Pre-allocate with exact capacity to avoid reallocation
    let mut documents = Vec::with_capacity(entity_contents.len());

    for (id, content) in entity_contents {
        documents.push((id.clone(), content.as_str()));
    }

    documents
}
