#[cfg(test)]
mod debug_tests {
    use crate::tsg::TsgExecutor;
    use std::path::Path;

    #[test]
    fn test_debug_enum_extraction() {
        let source = std::fs::read_to_string(
            "crates/languages/src/rust/handler_impls/tests/enum_tests.rs"
        ).unwrap();
        
        let mut executor = TsgExecutor::new_rust().unwrap();
        let nodes = executor.extract(&source, Path::new("enum_tests.rs")).unwrap();
        
        println!("\n=== All definitions from enum_tests.rs ===");
        for node in &nodes {
            if matches!(node.kind, crate::tsg::ResolutionNodeKind::Definition) {
                println!("DEF: {} ({:?}) at line {}", 
                    node.name, 
                    node.definition_kind,
                    node.start_line
                );
            }
        }
        
        println!("\n=== Err/Ok/Result nodes ===");
        for node in &nodes {
            if node.name == "Err" || node.name == "Ok" || node.name == "Result" {
                println!("{:?}: {} at line {}", node.kind, node.name, node.start_line);
            }
        }
    }
}
