#!/usr/bin/env python3
"""Label evaluation candidates for hybrid search evaluation."""

import json
from typing import Dict, List

def label_query_0(candidates: List[Dict]) -> Dict[str, int]:
    """How do I add a required command line argument?"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        snippet = c['code_snippet']
        snippet_lower = snippet.lower()
        path = c['file_path'].lower()

        # THE required() method - this is the answer
        if name == 'required' and etype == 'Method':
            labels[eid] = 1
        # Arg struct/impl - needed to understand the API
        elif name == 'Arg' and etype in ['Struct', 'Impl']:
            labels[eid] = 1
        # Examples (not tests) showing .required(true) usage
        elif etype == 'Function' and '.required(true)' in snippet and 'example' in path:
            labels[eid] = 1
        # Methods for basic Arg construction (new, long, short, action)
        elif etype == 'Method' and name in ['new', 'long', 'short', 'action'] and 'impl Arg' in snippet:
            labels[eid] = 1
        # arg_required_else_help method
        elif name == 'arg_required_else_help' and etype == 'Method':
            labels[eid] = 1
        # Test functions
        elif etype == 'Function' and ('test' in path or 'tests/' in path):
            labels[eid] = 0
        # Unrelated methods (conflicts, aliases, etc.)
        elif name in ['conflicts_with', 'conflicts_with_all', 'aliases', 'get_possible_values', 'from_matches']:
            labels[eid] = 0
        # Default to not helpful
        else:
            labels[eid] = 0

    return labels

def label_query_1(candidates: List[Dict]) -> Dict[str, int]:
    """Show me how to define subcommands"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        snippet = c['code_snippet']
        snippet_lower = snippet.lower()

        # THE subcommand method - this is the answer
        if name == 'subcommand' and etype == 'Method':
            labels[eid] = 1
        # subcommands method (plural)
        elif name == 'subcommands' and etype == 'Method':
            labels[eid] = 1
        # Command struct/impl for understanding API
        elif name == 'Command' and etype in ['Struct', 'Impl']:
            labels[eid] = 1
        # Examples showing .subcommand() usage
        elif etype == 'Function' and '.subcommand(' in snippet:
            labels[eid] = 1
        # Subcommand module
        elif name == 'subcommand' and etype == 'Module':
            labels[eid] = 1
        # Methods related to subcommands
        elif any(x in name.lower() for x in ['subcommand_required', 'subcommand_help']):
            labels[eid] = 1
        # Test functions
        elif etype == 'Function' and ('test' in path or 'tests/' in path):
            labels[eid] = 0
        # Unrelated (like global, author, constants)
        elif name in ['global', 'author'] or etype == 'Constant':
            labels[eid] = 0
        # Default to not helpful
        else:
            labels[eid] = 0

    return labels

def label_query_2(candidates: List[Dict]) -> Dict[str, int]:
    """How do I add default values to arguments?"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        snippet = c['code_snippet']
        snippet_lower = snippet.lower()
        path = c['file_path'].lower()

        # Methods for setting default values (the answer)
        if 'default_value' in name.lower() and etype == 'Method' and 'impl arg' in snippet_lower:
            labels[eid] = 1
        # Arg struct/impl
        elif name == 'Arg' and etype in ['Struct', 'Impl']:
            labels[eid] = 1
        # Examples showing .default_value() usage
        elif etype == 'Function' and '.default_value(' in snippet and 'example' in path:
            labels[eid] = 1
        # Parser methods related to defaults (internal but relevant)
        elif 'default' in name.lower() and 'parser.rs' in path:
            labels[eid] = 1
        # Test functions
        elif etype == 'Function' and ('test' in path or 'tests/' in path):
            labels[eid] = 0
        # Unrelated (like bool parser, enums, etc.)
        elif etype in ['Enum', 'Constant'] or name in ['bool', 'opt']:
            labels[eid] = 0
        # Default to not helpful
        else:
            labels[eid] = 0

    return labels

def label_query_3(candidates: List[Dict]) -> Dict[str, int]:
    """What types can I use for argument values?"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        snippet = c['code_snippet']
        snippet_lower = snippet.lower()
        path = c['file_path'].lower()

        # ValueParser - THE answer for types
        if 'valueparser' in name.lower() and etype in ['Struct', 'Enum', 'Impl', 'Method', 'Macro']:
            labels[eid] = 1
        # PossibleValue - related to value types
        elif 'possiblevalue' in name.lower() and etype in ['Struct', 'Impl']:
            labels[eid] = 1
        # Arg struct (shows how to use value_parser)
        elif name == 'Arg' and etype == 'Struct':
            labels[eid] = 1
        # Examples showing value_parser usage
        elif etype == 'Function' and 'value_parser' in snippet_lower and 'example' in path:
            labels[eid] = 1
        # ValueEnum trait/derive
        elif 'valueenum' in name.lower():
            labels[eid] = 1
        # Test functions
        elif etype == 'Function' and ('test' in path or 'tests/' in path):
            labels[eid] = 0
        # Unrelated (MKeyMap, etc.)
        elif name in ['MKeyMap', 'get_visible_quoted_name']:
            labels[eid] = 0
        # Default to not helpful
        else:
            labels[eid] = 0

    return labels

def label_query_4(candidates: List[Dict]) -> Dict[str, int]:
    """Find the argument parser implementation"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        snippet = c['code_snippet']
        snippet_lower = snippet.lower()
        path = c['file_path'].lower()

        # Core Parser struct/impl (THE answer for arg parser)
        if name == 'Parser' and etype in ['Struct', 'Impl'] and 'clap_builder' in path:
            labels[eid] = 1
        # Main parsing methods (get_matches, parse)
        elif name in ['get_matches', 'try_get_matches', 'parse'] and etype == 'Method':
            labels[eid] = 1
        # Parser module
        elif etype == 'Module' and 'parser' in name.lower() and 'clap_builder' in path:
            labels[eid] = 1
        # Internal parser implementation files
        elif 'parser.rs' in path and etype in ['Struct', 'Impl', 'Function'] and 'test' not in name.lower():
            labels[eid] = 1
        # ValueParser is about value parsing, not arg parsing
        elif 'valueparser' in name.lower():
            labels[eid] = 0
        # Test functions
        elif etype == 'Function' and ('test' in path or 'tests/' in path):
            labels[eid] = 0
        # Examples (not implementation)
        elif 'example' in path:
            labels[eid] = 0
        # Unrelated (id.rs, etc.)
        elif name in ['as_internal_str', 'check_auto_traits']:
            labels[eid] = 0
        # Default to not helpful
        else:
            labels[eid] = 0

    return labels

def label_query_5(candidates: List[Dict]) -> Dict[str, int]:
    """Show me where argument validation happens"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        snippet = c['code_snippet']
        snippet_lower = snippet.lower()
        path = c['file_path'].lower()

        # Validation-related methods (matches, validate, etc.)
        if any(x in name.lower() for x in ['valid', 'matches']) and etype == 'Method':
            if 'possiblevalue' in snippet_lower or 'parser' in path:
                labels[eid] = 1
            else:
                labels[eid] = 0
        # ValueParser handles validation
        elif 'valueparser' in name.lower() and etype in ['Struct', 'Impl']:
            labels[eid] = 1
        # Error types related to validation
        elif 'error' in path and any(x in name.lower() for x in ['validation', 'invalid']):
            labels[eid] = 1
        # Parser validation logic
        elif 'parser' in path and any(x in name.lower() for x in ['verify', 'check', 'validate']):
            labels[eid] = 1
        # Test functions showing validation errors (useful examples)
        elif etype == 'Function' and 'validation_error' in name.lower():
            labels[eid] = 1
        # Generic test functions
        elif etype == 'Function' and 'test' in name.lower():
            labels[eid] = 0
        # Examples
        elif 'example' in path:
            labels[eid] = 0
        # Unrelated (author, verify_cli in tests)
        elif name in ['author', 'verify_cli']:
            labels[eid] = 0
        # Default to not helpful
        else:
            labels[eid] = 0

    return labels

def label_query_6(candidates: List[Dict]) -> Dict[str, int]:
    """How do I handle multiple values for an argument?"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        snippet = c['code_snippet']
        snippet_lower = snippet.lower()
        path = c['file_path'].lower()

        # THE answer: num_args method
        if name == 'num_args' and etype == 'Method':
            labels[eid] = 1
        # get_many method (for retrieving multiple values)
        elif name == 'get_many' and etype == 'Method':
            labels[eid] = 1
        # Arg struct/impl
        elif name == 'Arg' and etype in ['Struct', 'Impl']:
            labels[eid] = 1
        # Examples showing .num_args() or .action(ArgAction::Append)
        elif etype == 'Function' and ('.num_args(' in snippet or 'argaction::append' in snippet_lower):
            if 'example' in path:
                labels[eid] = 1
            else:
                labels[eid] = 0
        # Test functions specifically about multiple values (helpful examples)
        elif etype == 'Function' and 'multiple' in name.lower() and 'value' in name.lower():
            if 'multiple_values.rs' in path:
                labels[eid] = 1
            else:
                labels[eid] = 0
        # Generic test functions
        elif etype == 'Function' and 'test' in name.lower():
            labels[eid] = 0
        # Constants and unrelated
        elif etype == 'Constant' or name in ['flag_subcommand_short_conflict_with_arg', 'ensure_typed_applies_to_parse']:
            labels[eid] = 0
        # Default to not helpful
        else:
            labels[eid] = 0

    return labels

def label_query_7(candidates: List[Dict]) -> Dict[str, int]:
    """Find the error handling code for invalid arguments"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        snippet = c['code_snippet']
        snippet_lower = snippet.lower()
        path = c['file_path'].lower()

        # Error types (ErrorKind, ContextKind, etc.)
        if 'error' in name.lower() and etype in ['Enum', 'Struct', 'Impl']:
            if any(x in path for x in ['error', 'context', 'kind']):
                labels[eid] = 1
            else:
                labels[eid] = 0
        # Error handling methods (try_get_matches, etc.)
        elif name.startswith('try_') and etype == 'Method':
            labels[eid] = 1
        # Parser error handling implementation
        elif 'parser' in path and 'error' in snippet_lower and etype in ['Function', 'Method']:
            labels[eid] = 1
        # Examples showing error handling
        elif etype == 'Function' and 'example' in path and 'error' in snippet_lower:
            labels[eid] = 1
        # Test functions
        elif etype == 'Function' and ('test' in path or 'tests/' in path):
            labels[eid] = 0
        # Modules (not helpful)
        elif etype == 'Module':
            labels[eid] = 0
        # Unrelated
        else:
            labels[eid] = 0

    return labels

def label_query_8(candidates: List[Dict]) -> Dict[str, int]:
    """What's the difference between Arg and Command types?"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        snippet = c['code_snippet']
        snippet_lower = snippet.lower()
        path = c['file_path'].lower()

        # THE types being asked about - Arg struct/impl
        if name == 'Arg' and etype in ['Struct', 'Impl']:
            labels[eid] = 1
        # THE types being asked about - Command struct/impl
        elif name == 'Command' and etype in ['Struct', 'Impl']:
            labels[eid] = 1
        # Examples showing both types
        elif etype == 'Function' and 'arg::new' in snippet_lower and 'command::new' in snippet_lower:
            if 'example' in path:
                labels[eid] = 1
            else:
                labels[eid] = 0
        # Documentation/comments explaining the difference
        elif etype in ['Module', 'File'] and 'arg' in snippet_lower and 'command' in snippet_lower:
            labels[eid] = 1
        # Test functions
        elif etype == 'Function' and ('test' in path or 'tests/' in path):
            labels[eid] = 0
        # Other unrelated types (PossibleValue, etc.)
        elif name in ['is_hide_set', 'get_visible_quoted_name', 'require_equals']:
            labels[eid] = 0
        # Macros
        elif etype == 'Macro':
            labels[eid] = 0
        # Default to not helpful
        else:
            labels[eid] = 0

    return labels

def label_query_9(candidates: List[Dict]) -> Dict[str, int]:
    """Show me examples of custom value parsers"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        snippet = c['code_snippet']
        snippet_lower = snippet.lower()
        path = c['file_path'].lower()

        # custom_string_parsers module/example - THE answer
        if 'custom' in name.lower() and 'parser' in name.lower():
            if 'example' in path:
                labels[eid] = 1
            else:
                labels[eid] = 0
        # Examples showing value_parser usage with custom types
        elif 'example' in path and 'value_parser' in snippet_lower:
            labels[eid] = 1
        # Functions in examples that implement custom parsing
        elif etype == 'Function' and 'example' in path:
            if any(x in snippet_lower for x in ['parse', 'from_str', 'value_parser']):
                labels[eid] = 1
            else:
                labels[eid] = 0
        # ValueParser API (helpful to understand)
        elif 'valueparser' in name.lower() and etype in ['Struct', 'Impl']:
            labels[eid] = 1
        # Test functions
        elif etype == 'Function' and ('test' in path or 'tests/' in path):
            labels[eid] = 0
        # Modules not in examples
        elif etype == 'Module' and 'example' not in path:
            labels[eid] = 0
        # Unrelated methods (hide_short_help, author, etc.)
        elif name in ['hide_short_help', 'author', 'flag'] or etype == 'Constant':
            labels[eid] = 0
        # Default to not helpful
        else:
            labels[eid] = 0

    return labels

def main():
    # Read input file
    with open('/home/logan/code/codesearch/tests/data/evaluation_candidates.json', 'r') as f:
        all_queries = json.load(f)

    # Extract queries 0-9
    queries_to_label = all_queries[0:10]

    # Label functions
    label_functions = [
        label_query_0,
        label_query_1,
        label_query_2,
        label_query_3,
        label_query_4,
        label_query_5,
        label_query_6,
        label_query_7,
        label_query_8,
        label_query_9,
    ]

    # Process each query
    results = {
        "query_range": "0-9",
        "labels": []
    }

    for idx, query_data in enumerate(queries_to_label):
        query = query_data['query']
        candidates = query_data['candidates']

        # Get labels from corresponding function
        entity_relevance = label_functions[idx](candidates)

        # Verify we labeled all 50 candidates
        assert len(entity_relevance) == 50, f"Query {idx} has {len(entity_relevance)} labels, expected 50"

        results["labels"].append({
            "query": query,
            "entity_relevance": entity_relevance
        })

        # Print summary
        helpful_count = sum(1 for v in entity_relevance.values() if v == 1)
        print(f"Query {idx}: {helpful_count}/50 labeled as helpful")

    # Save output
    with open('/home/logan/code/codesearch/tests/data/labels_0-9.json', 'w') as f:
        json.dump(results, f, indent=2)

    print(f"\nLabels saved to /home/logan/code/codesearch/tests/data/labels_0-9.json")

if __name__ == '__main__':
    main()
