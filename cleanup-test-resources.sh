#!/bin/bash
# Cleanup lingering test resources

set -e

echo "🧹 Cleaning up test resources..."

# Kill all outbox_processor and outbox-processor instances
echo "Stopping outbox processor processes..."
pkill -9 -f "outbox.processor" 2>/dev/null || echo "  No outbox processor processes found"

# Stop and remove test containers (both postgres and qdrant)
echo "Stopping test containers..."
TEST_CONTAINERS=$(docker ps -aq --filter "name=postgres-test" --filter "name=qdrant-test" --filter "name=outbox-processor-test")
if [ -n "$TEST_CONTAINERS" ]; then
    echo "$TEST_CONTAINERS" | xargs docker stop
    echo "Removing test containers..."
    echo "$TEST_CONTAINERS" | xargs docker rm -f
else
    echo "  No test containers found"
fi

# Also clean up any containers with "test" in the name that are using test images
echo "Cleaning up additional test containers..."
docker ps -a | grep -E "(qdrant.*test|postgres.*test|outbox.*test)" | awk '{print $1}' | xargs -r docker rm -f 2>/dev/null || true

# Clean up orphaned test temp directories
echo "Cleaning up test temp directories..."
sudo rm -rf /tmp/qdrant-test-* 2>/dev/null || true
rm -rf /tmp/codesearch-test-* 2>/dev/null || true

# Count what remains
CONTAINERS_REMAINING=$(docker ps -a | grep -cE "(qdrant-test|postgres-test|outbox-processor-test)" || echo "0" | head -1)

echo "✅ Cleanup complete!"
echo "   Containers remaining: $CONTAINERS_REMAINING"

# Show what test containers are still running, if any
if [ "$CONTAINERS_REMAINING" -gt 0 ] 2>/dev/null; then
    echo "⚠️  Warning: Some test containers still remain:"
    docker ps -a | grep -E "(qdrant-test|postgres-test|outbox-processor-test)" || true
fi
