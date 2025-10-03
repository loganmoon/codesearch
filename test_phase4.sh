#!/bin/bash
set -e

echo "=== Phase 4 Test: Outbox Pattern with Dockerized Processor ==="

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

fn multiply(x: i32, y: i32) -> i32 {
    x * y
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
collection_name = "test_phase4"
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

# Start Docker services (Postgres and Qdrant only, NOT outbox-processor yet)
echo ""
echo "Starting Docker services (Postgres and Qdrant)..."
docker compose up -d qdrant postgres 2>&1 | grep -E "(Creating|Starting|Started|Running)" || true

# Wait for services to be healthy (including time for Postgres migrations)
echo "Waiting for services to be healthy and migrations to complete..."
sleep 10

# Clean up any old test data
echo ""
echo "Cleaning up old test data..."
docker exec codesearch-postgres psql -U codesearch -d codesearch -c "TRUNCATE TABLE entity_outbox, entity_metadata, entity_versions CASCADE;" 2>&1 > /dev/null || true

# Initialize collection
echo ""
echo "Initializing collection..."
cargo run --quiet --bin codesearch -- init --config "$TEST_DIR/codesearch.toml" 2>&1 | grep -E "(initialized|error|Error)" || true

# Index the repository
echo ""
echo "Indexing repository (writing to outbox)..."
cd "$TEST_DIR"
cargo run --quiet --manifest-path /home/logan/code/codesearch/Cargo.toml --bin codesearch -- index 2>&1 | grep -E "(INFO|complete|error|Error)" || true

# Verify outbox entries were created
echo ""
echo "Verifying outbox entries created..."
UNPROCESSED_INITIAL=$(docker exec codesearch-postgres psql -U codesearch -d codesearch -t -c \
  "SELECT COUNT(*) FROM entity_outbox WHERE processed_at IS NULL;" | tr -d ' ')

if [ "$UNPROCESSED_INITIAL" -eq 0 ]; then
    echo "❌ FAILED: No outbox entries were created during indexing!"
    exit 1
fi

echo "✅ Created $UNPROCESSED_INITIAL unprocessed outbox entries"

# Show sample entries
echo ""
echo "Sample outbox entries:"
docker exec codesearch-postgres psql -U codesearch -d codesearch -c \
  "SELECT outbox_id, entity_id, operation, target_store FROM entity_outbox LIMIT 3;" 2>&1 | head -10

# NOW start the outbox processor
echo ""
echo "Starting outbox-processor..."
docker compose up -d outbox-processor 2>&1 | grep -E "(Creating|Starting|Started)" || true
sleep 3

# Verify processor is running
echo ""
echo "Checking outbox-processor status..."
docker ps | grep codesearch-outbox-processor || (echo "❌ Outbox processor failed to start!" && exit 1)
echo "✅ Outbox processor running"

# Wait for outbox processor to drain the queue
echo ""
echo "Waiting 10 seconds for outbox processor to process entries..."
sleep 10

# Check processor logs
echo ""
echo "Outbox processor logs (last 20 lines):"
docker logs --tail 20 codesearch-outbox-processor 2>&1 | grep -E "(Outbox processor|Processing|Processed|error)" || echo "No relevant log entries"

# Query Postgres to verify entries were processed
echo ""
echo "Checking processed outbox entries..."
PROCESSED_COUNT=$(docker exec codesearch-postgres psql -U codesearch -d codesearch -t -c \
  "SELECT COUNT(*) FROM entity_outbox WHERE processed_at IS NOT NULL;" | tr -d ' ')
UNPROCESSED_REMAINING=$(docker exec codesearch-postgres psql -U codesearch -d codesearch -t -c \
  "SELECT COUNT(*) FROM entity_outbox WHERE processed_at IS NULL;" | tr -d ' ')

echo "Processed entries: $PROCESSED_COUNT"
echo "Unprocessed entries: $UNPROCESSED_REMAINING"

if [ "$PROCESSED_COUNT" -gt 0 ] && [ "$PROCESSED_COUNT" -eq "$UNPROCESSED_INITIAL" ]; then
    echo "✅ SUCCESS: All $PROCESSED_COUNT entries were processed by outbox processor!"
else
    echo "❌ FAILED: Expected $UNPROCESSED_INITIAL processed, got $PROCESSED_COUNT"
    echo ""
    echo "Checking for errors in outbox entries:"
    docker exec codesearch-postgres psql -U codesearch -d codesearch -c \
      "SELECT outbox_id, entity_id, operation, retry_count, last_error FROM entity_outbox LIMIT 5;"
    exit 1
fi

# Verify entities are in Qdrant (optional check)
echo ""
echo "Verifying entity metadata in Postgres..."
METADATA_COUNT=$(docker exec codesearch-postgres psql -U codesearch -d codesearch -t -c \
  "SELECT COUNT(*) FROM entity_metadata;" | tr -d ' ')
echo "Entity metadata entries: $METADATA_COUNT"

if [ "$METADATA_COUNT" -gt 0 ]; then
    echo "✅ Entity metadata stored correctly"
else
    echo "⚠️  No entity metadata found"
fi

# Show sample outbox entries
echo ""
echo "Sample processed outbox entries:"
docker exec codesearch-postgres psql -U codesearch -d codesearch -c \
  "SELECT outbox_id, entity_id, operation, target_store, processed_at FROM entity_outbox WHERE processed_at IS NOT NULL LIMIT 3;"

# Cleanup
echo ""
echo "Cleaning up test directory: $TEST_DIR"
rm -rf "$TEST_DIR"

echo ""
echo "=== Phase 4 Test Complete ==="
echo ""
echo "Summary:"
echo "  - Outbox entries created: $UNPROCESSED_INITIAL"
echo "  - Entries processed: $PROCESSED_COUNT"
echo "  - Entries remaining: $UNPROCESSED_REMAINING"
echo "  - Entity metadata: $METADATA_COUNT"
echo ""
echo "✅ Phase 4 outbox pattern validated successfully!"
