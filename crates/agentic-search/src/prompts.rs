//! Prompt templates for agentic search
//!
//! PRIVATE MODULE - Not exported from crate

// Allow dead code until Phase 2 implementation
#![allow(dead_code)]

pub const ORCHESTRATOR_PLAN: &str = include_str!("../assets/prompts/orchestrator_plan.txt");
pub const WORKER_RERANK: &str = include_str!("../assets/prompts/worker_rerank.txt");
pub const FUSION_RERANK: &str = include_str!("../assets/prompts/fusion_rerank.txt");
pub const GRAPH_EVALUATE: &str = include_str!("../assets/prompts/graph_evaluate.txt");
pub const QUALITY_GATE_COMPOSE: &str = include_str!("../assets/prompts/quality_gate_compose.txt");

pub fn format_prompt(template: &str, vars: &[(&str, &str)]) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("{{{key}}}"), value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_prompt() {
        let template = "Hello {name}, you are {age} years old.";
        let vars = [("name", "Alice"), ("age", "30")];
        let result = format_prompt(template, &vars);
        assert_eq!(result, "Hello Alice, you are 30 years old.");
    }

    #[test]
    #[allow(clippy::len_zero)] // const_is_empty conflicts with len_zero for const strings
    fn test_prompts_load() {
        // Verify prompts compile and are accessible
        // Note: using len() > 0 instead of is_empty() because clippy's const_is_empty
        // lint complains that is_empty() on const strings always evaluates to false
        assert!(ORCHESTRATOR_PLAN.len() > 0);
        assert!(WORKER_RERANK.len() > 0);
        assert!(FUSION_RERANK.len() > 0);
        assert!(GRAPH_EVALUATE.len() > 0);
        assert!(QUALITY_GATE_COMPOSE.len() > 0);
    }
}
