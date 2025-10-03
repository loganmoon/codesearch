#!/bin/bash
set -e

echo "=== Phase 3 Test: Entity Versioning with Git Commit Tracking ==="

# Create a temp directory for the test repo
TEST_DIR=$(mktemp -d)
echo "Test repo: $TEST_DIR"

# Initialize a git repo
cd "$TEST_DIR"
git init
git config user.email "test@example.com"
git config user.name "Test User"

# Create a simple Rust file
cat > main.rs << 'EOF'
fn main() {
    println!("Hello, world!");
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}
EOF

# Make initial commit
git add main.rs
git commit -m "Initial commit"

# Get the commit hash
COMMIT_HASH=$(git rev-parse HEAD)
echo "Git commit hash: $COMMIT_HASH"

# Create config file
cat > codesearch.toml << 'EOF'
[indexer]

[storage]
qdrant_host = "localhost"
qdrant_port = 6334
collection_name = "test_phase3"
auto_start_deps = false

postgres_host = "localhost"
postgres_port = 5432
postgres_database = "codesearch"
postgres_user = "codesearch"
postgres_password = "codesearch"

[embeddings]
provider = "mock"

[watcher]
debounce_ms = 500
ignore_patterns = ["*.log", "target", ".git"]
branch_strategy = "index_current"

[languages]
enabled = ["rust"]
EOF

# Go back to codesearch directory
cd /home/logan/code/codesearch

# Initialize collection
echo "Initializing collection..."
cargo run --quiet -- init --config "$TEST_DIR/codesearch.toml" 2>&1 | grep -E "(initialized|error|Error)" || true

# Index the repository
echo "Indexing repository..."
cd "$TEST_DIR"
cargo run --quiet --manifest-path /home/logan/code/codesearch/Cargo.toml -- index 2>&1 | grep -E "(INFO|complete|error|Error)" || true

# Query Postgres to verify version with git commit hash
echo ""
echo "Querying Postgres for entity versions..."
docker exec codesearch-postgres psql -U codesearch -d codesearch -c \
  "SELECT entity_id, version_number, git_commit_hash FROM entity_versions WHERE git_commit_hash = '$COMMIT_HASH' LIMIT 5;" 2>&1

echo ""
echo "Verifying git commit hash is not 'unknown' or 'no-git'..."
COUNT=$(docker exec codesearch-postgres psql -U codesearch -d codesearch -t -c \
  "SELECT COUNT(*) FROM entity_versions WHERE git_commit_hash = '$COMMIT_HASH';")

if [ "$COUNT" -gt 0 ]; then
    echo "✅ SUCCESS: Found $COUNT entities with correct git commit hash!"
else
    echo "❌ FAILED: No entities found with git commit hash $COMMIT_HASH"
    echo "Checking what commit hashes were stored:"
    docker exec codesearch-postgres psql -U codesearch -d codesearch -c \
      "SELECT DISTINCT git_commit_hash FROM entity_versions;"
    exit 1
fi

# Cleanup
echo ""
echo "Cleaning up test directory: $TEST_DIR"
rm -rf "$TEST_DIR"

echo ""
echo "=== Phase 3 Test Complete ==="
