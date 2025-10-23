#!/usr/bin/env python3
"""
Label evaluation data for queries 20-29.
This script analyzes each query-candidate pair and assigns relevance labels.
"""

import json
import re

def is_test_entity(file_path, entity_name):
    """Check if entity is a test function/module."""
    return ('test' in file_path.lower() or
            entity_name.startswith('test_') or
            '/tests/' in file_path or
            '/benches/' in file_path)

def is_example_entity(file_path):
    """Check if entity is from examples."""
    return '/examples/' in file_path

def label_query_20(query, candidates):
    """How do I validate argument values?"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        path = c['file_path']
        snippet = c['code_snippet'].lower()

        # Highly relevant: validation-related entities
        if (name in ['ValueParser', 'TypedValueParser', 'ArgPredicate', 'Arg', 'action'] or
            name == 'matches' and 'possible_value' in path or
            'value_parser' in name.lower() or
            'validate' in name.lower() or
            'validator' in name.lower() or
            name == 'require_equals' or
            ('range' in name.lower() and 'value_parser' in path) or
            (etype == 'Trait' and 'valueparser' in name.lower())):
            labels[eid] = 1
        # Test/example entities - always 0
        elif is_test_entity(path, name) or is_example_entity(path):
            labels[eid] = 0
        # Generic arg methods without validation context
        else:
            labels[eid] = 0

    return labels

def label_query_21(query, candidates):
    """Show me the TypedValueParser trait"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        path = c['file_path']
        snippet = c['code_snippet'].lower()

        # The exact trait or directly related types
        if (name == 'TypedValueParser' or
            name in ['AnyValueParser', 'ValueParserFactory', 'MapValueParser',
                     'ValueParser'] and 'value_parser.rs' in path or
            (etype == 'Trait' and 'typedvalueparser' in snippet)):
            labels[eid] = 1
        # Test/example entities
        elif is_test_entity(path, name) or is_example_entity(path):
            labels[eid] = 0
        else:
            labels[eid] = 0

    return labels

def label_query_22(query, candidates):
    """Find where short and long argument names are handled"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        path = c['file_path']
        snippet = c['code_snippet'].lower()

        # Entities that handle short/long names
        if (('short' in name.lower() or 'long' in name.lower()) and
            ('arg.rs' in path or 'parser' in path or 'mkeymap' in path) or
            name in ['Flag', 'get', 'parse_long', 'parse_short', 'parse_long_arg', 'parse_short_arg'] or
            ('short_flag' in snippet and 'long_flag' in snippet) or
            'mkeymap' in path):
            labels[eid] = 1
        # Test entities unless they demonstrate the pattern
        elif is_test_entity(path, name) and 'short' not in snippet:
            labels[eid] = 0
        else:
            labels[eid] = 0

    return labels

def label_query_23(query, candidates):
    """How do I create optional arguments?"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        path = c['file_path']
        snippet = c['code_snippet'].lower()

        # Relevant: methods/docs about required/optional args
        if (('required' in name.lower() or 'optional' in snippet) and
            'arg.rs' in path or
            name in ['Arg', 'required', 'required_unless_present', 'required_if_eq'] or
            ('required(' in snippet and 'arg.rs' in path) or
            (is_example_entity(path) and 'optional' in snippet and etype == 'Function')):
            labels[eid] = 1
        # Test entities
        elif is_test_entity(path, name):
            labels[eid] = 0
        # Unrelated
        else:
            labels[eid] = 0

    return labels

def label_query_24(query, candidates):
    """Show me the main parsing entry point"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        path = c['file_path']
        snippet = c['code_snippet'].lower()

        # Main parsing entry points
        if (name in ['parse', 'try_parse', 'get_matches', 'try_get_matches',
                     'get_matches_from', 'try_get_matches_from', '_do_parse', 'Parser'] and
            'command.rs' in path or
            (name == 'parser' and etype == 'Module' and 'parser/mod.rs' in path) or
            (etype == 'Struct' and name == 'Parser' and 'parser' in path)):
            labels[eid] = 1
        # Example main functions that show get_matches usage
        elif (name == 'main' and is_example_entity(path) and
              'get_matches' in snippet):
            labels[eid] = 1
        # Test/other example entities
        elif is_test_entity(path, name) or is_example_entity(path):
            labels[eid] = 0
        else:
            labels[eid] = 0

    return labels

def label_query_25(query, candidates):
    """What error types can parsing return?"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        path = c['file_path']
        snippet = c['code_snippet'].lower()

        # Error-related entities
        if (('error' in path.lower() and etype in ['Struct', 'Enum', 'Trait', 'Module']) or
            name in ['Error', 'ErrorKind', 'Result', 'ClapResult'] or
            ('error' in name.lower() and etype in ['Enum', 'Struct', 'Trait']) or
            ('error' in snippet and 'enum' in snippet and etype == 'Enum')):
            labels[eid] = 1
        # Test/example entities
        elif is_test_entity(path, name) or is_example_entity(path):
            labels[eid] = 0
        else:
            labels[eid] = 0

    return labels

def label_query_26(query, candidates):
    """Find the code that matches arguments to their definitions"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        path = c['file_path']
        snippet = c['code_snippet'].lower()

        # Matching/parsing logic
        if (name in ['Parser', 'parse_opt', 'parse_arg', 'react', 'match_arg',
                     'verify_positionals', 'get', 'resolve_pending'] or
            ('match' in name.lower() and 'parser' in path) or
            ('parser' in path and etype in ['Struct', 'Module'] and
             'parser/mod.rs' in path or 'parser.rs' in path) or
            'mkeymap' in path or
            ('matches' in name.lower() and 'parser' in path)):
            labels[eid] = 1
        # Test/example entities
        elif is_test_entity(path, name) or is_example_entity(path):
            labels[eid] = 0
        else:
            labels[eid] = 0

    return labels

def label_query_27(query, candidates):
    """How do I add positional arguments?"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        path = c['file_path']
        snippet = c['code_snippet'].lower()

        # Positional argument handling
        if (('positional' in name.lower() or 'num_vals' in name.lower() or
             'value_name' in name.lower()) and 'arg.rs' in path or
            name in ['Arg', 'index', 'trailing_var_arg'] or
            (is_example_entity(path) and 'positional' in snippet and etype == 'Function') or
            ('positional' in snippet and 'arg.rs' in path)):
            labels[eid] = 1
        # Test entities
        elif is_test_entity(path, name):
            labels[eid] = 0
        else:
            labels[eid] = 0

    return labels

def label_query_28(query, candidates):
    """Show me where argument groups are implemented"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        path = c['file_path']
        snippet = c['code_snippet'].lower()

        # Argument group entities
        if (name in ['ArgGroup', 'group', 'groups', 'get_group', 'two_groups_of',
                     'groups_for_arg', 'find_group', 'conflicts_with_all'] or
            'arg_group.rs' in path or
            (etype == 'Struct' and name == 'ArgGroup') or
            ('group' in name.lower() and ('command.rs' in path or 'arg_group' in path))):
            labels[eid] = 1
        # Test/example entities
        elif is_test_entity(path, name) or is_example_entity(path):
            labels[eid] = 0
        else:
            labels[eid] = 0

    return labels

def label_query_29(query, candidates):
    """What's the Command builder pattern?"""
    labels = {}

    for c in candidates:
        eid = c['entity_id']
        name = c['entity_name']
        etype = c['entity_type']
        path = c['file_path']
        snippet = c['code_snippet'].lower()

        # Command builder entities - struct, key methods, docs
        if (name == 'Command' and etype in ['Struct', 'Impl'] and 'command.rs' in path or
            name in ['new', 'build', 'arg', 'subcommand', 'args', 'subcommands',
                     'author', 'version', 'about'] and 'command.rs' in path and etype == 'Method' or
            (is_example_entity(path) and 'command::new' in snippet and etype == 'Function' and name == 'main')):
            labels[eid] = 1
        # Test entities
        elif is_test_entity(path, name) or is_example_entity(path):
            labels[eid] = 0
        else:
            labels[eid] = 0

    return labels

def main():
    # Load evaluation candidates
    with open('data/evaluation_candidates.json', 'r') as f:
        data = json.load(f)

    # Extract queries 20-29
    queries_subset = data[20:30]

    # Label functions for each query
    label_functions = {
        20: label_query_20,
        21: label_query_21,
        22: label_query_22,
        23: label_query_23,
        24: label_query_24,
        25: label_query_25,
        26: label_query_26,
        27: label_query_27,
        28: label_query_28,
        29: label_query_29,
    }

    # Initialize results
    results = {
        "query_range": "20-29",
        "labels": []
    }

    # Process each query
    for idx, query_data in enumerate(queries_subset, start=20):
        query = query_data['query']
        candidates = query_data['candidates']

        print(f"Processing Query {idx}: {query}")

        # Get labels for this query
        label_func = label_functions[idx]
        entity_relevance = label_func(query, candidates)

        # Ensure all 50 candidates are labeled
        if len(entity_relevance) != 50:
            print(f"WARNING: Query {idx} has {len(entity_relevance)} labels, expected 50")

        results["labels"].append({
            "query": query,
            "entity_relevance": entity_relevance
        })

        # Print statistics
        helpful_count = sum(1 for v in entity_relevance.values() if v == 1)
        print(f"  Labeled {len(entity_relevance)} candidates: {helpful_count} helpful, {len(entity_relevance) - helpful_count} not helpful")

    # Save results
    with open('data/labels_20-29.json', 'w') as f:
        json.dump(results, f, indent=2)

    print(f"\nSaved labels to data/labels_20-29.json")

if __name__ == '__main__':
    main()
