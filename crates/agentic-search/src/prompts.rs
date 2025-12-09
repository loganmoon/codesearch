//! Prompt templates for agentic search
//!
//! PRIVATE MODULE - Not exported from crate
//!
//! Prompts are split into system (cacheable) and user (dynamic) parts
//! to enable Claude API prompt caching for cost reduction.

// Worker reranking prompt (not split - too small to benefit from caching)
pub const WORKER_RERANK: &str = include_str!("../assets/prompts/worker_rerank.txt");

// Split prompts for caching - System prompts (static, cacheable)
pub const ORCHESTRATOR_PLAN_SYSTEM: &str =
    include_str!("../assets/prompts/orchestrator_plan_system.txt");

// Split prompts for caching - User prompts (dynamic)
pub const ORCHESTRATOR_PLAN_USER: &str =
    include_str!("../assets/prompts/orchestrator_plan_user.txt");

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
        // Verify worker rerank prompt loads
        assert!(WORKER_RERANK.len() > 0);

        // Verify split prompts for caching
        assert!(ORCHESTRATOR_PLAN_SYSTEM.len() > 0);
        assert!(ORCHESTRATOR_PLAN_USER.len() > 0);
    }
}
