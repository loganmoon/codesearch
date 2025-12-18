//! Evaluation utilities for measuring resolution rate
//!
//! This module provides tools to measure how effectively the TSG extraction
//! and resolution approach can resolve references to their canonical definitions.

#![deny(warnings)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use super::graph_types::{ResolutionNode, ResolutionNodeKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Results from evaluating resolution on a codebase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    /// Total number of files processed
    pub total_files: usize,
    /// Total number of nodes extracted
    pub total_nodes: usize,
    /// Number of Definition nodes
    pub definition_count: usize,
    /// Number of Export nodes
    pub export_count: usize,
    /// Number of Import nodes
    pub import_count: usize,
    /// Number of Reference nodes
    pub reference_count: usize,
    /// Number of references that resolved to an import in the same file
    pub intra_file_resolved: usize,
    /// Number of references that couldn't find a matching import
    pub unresolved: usize,
    /// Resolution rate (intra_file_resolved / reference_count)
    pub intra_file_resolution_rate: f64,
    /// Breakdown of unresolved references by pattern
    pub unresolved_by_pattern: HashMap<String, usize>,
}

impl EvaluationResult {
    /// Create an empty evaluation result
    pub fn new() -> Self {
        Self {
            total_files: 0,
            total_nodes: 0,
            definition_count: 0,
            export_count: 0,
            import_count: 0,
            reference_count: 0,
            intra_file_resolved: 0,
            unresolved: 0,
            intra_file_resolution_rate: 0.0,
            unresolved_by_pattern: HashMap::new(),
        }
    }

    /// Compute the resolution rate
    pub fn compute_rate(&mut self) {
        if self.reference_count > 0 {
            self.intra_file_resolution_rate =
                self.intra_file_resolved as f64 / self.reference_count as f64;
        }
    }
}

impl Default for EvaluationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Build intra-file resolution edges by matching Reference names to Import names
///
/// This simulates the first step of resolution: finding which import brings a name
/// into scope within the same file.
///
/// # Arguments
/// * `nodes` - Resolution nodes extracted from a single file
///
/// # Returns
/// Tuple of (resolved_count, unresolved_references)
pub fn build_intra_file_edges(nodes: &[ResolutionNode]) -> (usize, Vec<&ResolutionNode>) {
    // Collect imports by name
    let imports: HashMap<&str, &ResolutionNode> = nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Import)
        .map(|n| (n.name.as_str(), n))
        .collect();

    // Also collect definitions by name (for local definitions)
    let definitions: HashMap<&str, &ResolutionNode> = nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Definition)
        .map(|n| (n.name.as_str(), n))
        .collect();

    let mut resolved = 0;
    let mut unresolved = Vec::new();

    for node in nodes
        .iter()
        .filter(|n| n.kind == ResolutionNodeKind::Reference)
    {
        // Skip type references to primitive/prelude types
        if is_primitive_or_prelude(&node.name) {
            continue;
        }

        // Check if the reference matches an import or local definition
        if imports.contains_key(node.name.as_str()) || definitions.contains_key(node.name.as_str())
        {
            resolved += 1;
        } else {
            unresolved.push(node);
        }
    }

    (resolved, unresolved)
}

/// Check if a name is a Rust primitive type, prelude type, or should be skipped
pub fn is_primitive_or_prelude(name: &str) -> bool {
    is_rust_builtin(name)
}

/// Check if a name is a Rust primitive type, prelude type, or should be skipped
pub fn is_rust_builtin(name: &str) -> bool {
    // Underscore is used for unused bindings
    if name == "_" {
        return true;
    }

    // Skip single-letter uppercase names (generic type parameters like T, U, E, F)
    if name.len() == 1 && name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return true;
    }

    matches!(
        name,
        // Primitive types
        "i8" | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "bool"
            | "char"
            | "str"
            // Self reference
            | "Self"
            // Prelude types (std::prelude::v1)
            | "String"
            | "Vec"
            | "Option"
            | "Some"
            | "None"
            | "Result"
            | "Ok"
            | "Err"
            | "Box"
            | "Clone"
            | "Copy"
            | "Default"
            | "Drop"
            | "Eq"
            | "Ord"
            | "PartialEq"
            | "PartialOrd"
            | "AsRef"
            | "AsMut"
            | "Into"
            | "From"
            | "Iterator"
            | "Extend"
            | "IntoIterator"
            | "DoubleEndedIterator"
            | "ExactSizeIterator"
            | "Send"
            | "Sync"
            | "Sized"
            | "Unpin"
            | "ToOwned"
            | "ToString"
            | "TryFrom"
            | "TryInto"
            | "Fn"
            | "FnMut"
            | "FnOnce"
            // Common std types that are not prelude but very frequently used
            | "HashMap"
            | "HashSet"
            | "BTreeMap"
            | "BTreeSet"
            | "Arc"
            | "Rc"
            | "Mutex"
            | "RwLock"
            | "RefCell"
            | "Cell"
            | "Cow"
            | "Pin"
            | "PhantomData"
            // Path types (very commonly used)
            | "Path"
            | "PathBuf"
    )
}

/// Check if a name is a JavaScript/TypeScript builtin or global
pub fn is_javascript_builtin(name: &str) -> bool {
    // Underscore is used for unused bindings
    if name == "_" {
        return true;
    }

    // Skip single-letter uppercase names (generic type parameters like T, U)
    if name.len() == 1 && name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return true;
    }

    matches!(
        name,
        // JavaScript globals
        "undefined"
            | "null"
            | "NaN"
            | "Infinity"
            | "globalThis"
            | "window"
            | "document"
            | "navigator"
            | "location"
            | "history"
            // Built-in constructors and types
            | "Object"
            | "Array"
            | "String"
            | "Number"
            | "Boolean"
            | "Symbol"
            | "BigInt"
            | "Function"
            | "Date"
            | "RegExp"
            | "Error"
            | "TypeError"
            | "RangeError"
            | "ReferenceError"
            | "SyntaxError"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "Promise"
            | "Proxy"
            | "Reflect"
            | "JSON"
            | "Math"
            | "Intl"
            | "ArrayBuffer"
            | "DataView"
            | "Int8Array"
            | "Uint8Array"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float32Array"
            | "Float64Array"
            // Global functions
            | "console"
            | "setTimeout"
            | "setInterval"
            | "clearTimeout"
            | "clearInterval"
            | "setImmediate"
            | "clearImmediate"
            | "fetch"
            | "alert"
            | "confirm"
            | "prompt"
            | "eval"
            | "isNaN"
            | "isFinite"
            | "parseInt"
            | "parseFloat"
            | "encodeURI"
            | "decodeURI"
            | "encodeURIComponent"
            | "decodeURIComponent"
            | "atob"
            | "btoa"
            // Node.js globals
            | "Buffer"
            | "process"
            | "global"
            | "__dirname"
            | "__filename"
            | "module"
            | "exports"
            | "require"
            // Web APIs
            | "URL"
            | "URLSearchParams"
            | "Request"
            | "Response"
            | "Headers"
            | "FormData"
            | "Blob"
            | "File"
            | "FileReader"
            | "AbortController"
            | "AbortSignal"
            | "Event"
            | "EventTarget"
            | "CustomEvent"
            | "Element"
            | "HTMLElement"
            | "Node"
            | "NodeList"
            | "Document"
            | "Window"
            // TypeScript utility types
            | "Partial"
            | "Required"
            | "Readonly"
            | "Record"
            | "Pick"
            | "Omit"
            | "Exclude"
            | "Extract"
            | "NonNullable"
            | "ReturnType"
            | "Parameters"
            | "InstanceType"
            | "ThisType"
            | "Awaited"
            | "ConstructorParameters"
            | "ThisParameterType"
            | "OmitThisParameter"
            | "Uppercase"
            | "Lowercase"
            | "Capitalize"
            | "Uncapitalize"
            // Common type names
            | "any"
            | "unknown"
            | "never"
            | "void"
            | "string"
            | "number"
            | "boolean"
            | "object"
            | "symbol"
            | "bigint"
    )
}

/// Check if a name is a Python builtin type, function, or should be skipped
pub fn is_python_builtin(name: &str) -> bool {
    // Underscore is used for unused bindings
    if name == "_" {
        return true;
    }

    // Skip single-letter uppercase names (generic type parameters like T, U)
    if name.len() == 1 && name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return true;
    }

    matches!(
        name,
        // Built-in types
        "int"
            | "float"
            | "str"
            | "bool"
            | "bytes"
            | "bytearray"
            | "list"
            | "dict"
            | "set"
            | "frozenset"
            | "tuple"
            | "type"
            | "object"
            | "complex"
            | "range"
            | "slice"
            | "memoryview"
            // Built-in constants
            | "None"
            | "True"
            | "False"
            | "Ellipsis"
            | "NotImplemented"
            // Self references
            | "self"
            | "cls"
            // Built-in functions
            | "print"
            | "len"
            | "enumerate"
            | "zip"
            | "map"
            | "filter"
            | "sorted"
            | "reversed"
            | "iter"
            | "next"
            | "open"
            | "input"
            | "isinstance"
            | "issubclass"
            | "hasattr"
            | "getattr"
            | "setattr"
            | "delattr"
            | "super"
            | "property"
            | "staticmethod"
            | "classmethod"
            | "abs"
            | "all"
            | "any"
            | "bin"
            | "hex"
            | "oct"
            | "ord"
            | "chr"
            | "repr"
            | "callable"
            | "compile"
            | "eval"
            | "exec"
            | "globals"
            | "locals"
            | "vars"
            | "dir"
            | "id"
            | "hash"
            | "min"
            | "max"
            | "sum"
            | "pow"
            | "round"
            | "divmod"
            | "format"
            | "ascii"
            | "breakpoint"
            // Typing module types (commonly used without import)
            | "List"
            | "Dict"
            | "Set"
            | "Tuple"
            | "FrozenSet"
            | "Optional"
            | "Union"
            | "Any"
            | "Callable"
            | "Type"
            | "Sequence"
            | "Mapping"
            | "MutableMapping"
            | "MutableSequence"
            | "MutableSet"
            | "Iterator"
            | "Iterable"
            | "Generator"
            | "Coroutine"
            | "AsyncIterator"
            | "AsyncIterable"
            | "AsyncGenerator"
            | "Awaitable"
            | "Generic"
            | "Protocol"
            | "Final"
            | "Literal"
            | "ClassVar"
            | "TypeVar"
            | "TypeAlias"
            | "TypeGuard"
            | "ParamSpec"
            | "Concatenate"
            | "Self"
            | "Never"
            | "NoReturn"
            // Common exception types
            | "Exception"
            | "BaseException"
            | "ValueError"
            | "TypeError"
            | "KeyError"
            | "IndexError"
            | "AttributeError"
            | "RuntimeError"
            | "StopIteration"
            | "StopAsyncIteration"
            | "GeneratorExit"
            | "ImportError"
            | "ModuleNotFoundError"
            | "OSError"
            | "IOError"
            | "FileNotFoundError"
            | "PermissionError"
            | "TimeoutError"
            | "AssertionError"
            | "NotImplementedError"
            | "RecursionError"
            | "SyntaxError"
            | "IndentationError"
            | "TabError"
            | "SystemExit"
            | "KeyboardInterrupt"
    )
}

/// Categorize why a reference couldn't be resolved
pub fn categorize_unresolved(node: &ResolutionNode) -> &'static str {
    let name = &node.name;

    // Check patterns
    if name.starts_with('_') {
        "underscore_prefix"
    } else if name.chars().next().is_some_and(|c| c.is_uppercase()) {
        // Could be from glob import, prelude, or external crate
        "type_from_external"
    } else if name.chars().next().is_some_and(|c| c.is_lowercase()) {
        // Could be from glob import, prelude, or external crate
        "function_from_external"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsg::graph_types::ResolutionNode;
    use std::path::PathBuf;

    #[test]
    fn test_intra_file_resolution() {
        let nodes = vec![
            ResolutionNode::import(
                "Read".to_string(),
                "test::Read".to_string(),
                PathBuf::from("test.rs"),
                1,
                1,
                "std::io::Read".to_string(),
                false,
            ),
            ResolutionNode::definition(
                "MyStruct".to_string(),
                "test::MyStruct".to_string(),
                PathBuf::from("test.rs"),
                3,
                5,
                Some("pub".to_string()),
                "struct".to_string(),
            ),
            ResolutionNode::reference(
                "Read".to_string(),
                "test::Read".to_string(),
                PathBuf::from("test.rs"),
                10,
                10,
                Some("type".to_string()),
            ),
            ResolutionNode::reference(
                "MyStruct".to_string(),
                "test::MyStruct".to_string(),
                PathBuf::from("test.rs"),
                11,
                11,
                Some("type".to_string()),
            ),
            ResolutionNode::reference(
                "Unknown".to_string(),
                "test::Unknown".to_string(),
                PathBuf::from("test.rs"),
                12,
                12,
                Some("type".to_string()),
            ),
        ];

        let (resolved, unresolved) = build_intra_file_edges(&nodes);

        // Read resolves to import, MyStruct resolves to definition
        assert_eq!(resolved, 2);
        // Unknown has no matching import or definition
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].name, "Unknown");
    }

    #[test]
    fn test_primitives_and_prelude_skipped() {
        let nodes = vec![
            ResolutionNode::reference(
                "i32".to_string(),
                "test::i32".to_string(),
                PathBuf::from("test.rs"),
                1,
                1,
                Some("type".to_string()),
            ),
            ResolutionNode::reference(
                "String".to_string(),
                "test::String".to_string(),
                PathBuf::from("test.rs"),
                2,
                2,
                Some("type".to_string()),
            ),
            ResolutionNode::reference(
                "Vec".to_string(),
                "test::Vec".to_string(),
                PathBuf::from("test.rs"),
                3,
                3,
                Some("type".to_string()),
            ),
            ResolutionNode::reference(
                "Option".to_string(),
                "test::Option".to_string(),
                PathBuf::from("test.rs"),
                4,
                4,
                Some("type".to_string()),
            ),
            ResolutionNode::reference(
                "Result".to_string(),
                "test::Result".to_string(),
                PathBuf::from("test.rs"),
                5,
                5,
                Some("type".to_string()),
            ),
        ];

        let (resolved, unresolved) = build_intra_file_edges(&nodes);

        // All are primitives or prelude types, should be skipped
        assert_eq!(resolved, 0);
        assert_eq!(unresolved.len(), 0);
    }
}
