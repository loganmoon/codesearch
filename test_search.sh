#!/bin/bash
# Test script for the search functionality

set -e

echo "Testing codesearch search functionality..."

# Test basic search
echo "1. Testing basic search:"
cargo run --bin codesearch -- search "function" --limit 5

# Test search with entity type filter
echo -e "\n2. Testing search with entity type filter (function):"
cargo run --bin codesearch -- search "index" --entity-type function --limit 3

# Test search with language filter
echo -e "\n3. Testing search with language filter (rust):"
cargo run --bin codesearch -- search "impl" --language rust --limit 3

# Test search with file path filter
echo -e "\n4. Testing search with file path filter:"
cargo run --bin codesearch -- search "storage" --file crates/storage --limit 3

# Test combined filters
echo -e "\n5. Testing combined filters:"
cargo run --bin codesearch -- search "new" --entity-type function --language rust --limit 3

echo -e "\nAll search tests completed!"