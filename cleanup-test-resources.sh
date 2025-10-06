#!/bin/bash
# Cleanup lingering test resources

set -e

echo "🧹 Cleaning up test resources..."

# Kill all outbox_processor and outbox-processor instances
echo "Stopping outbox processor processes..."
pkill -9 -f "outbox.processor" 2>/dev/null || echo "  No outbox processor processes found"

# Stop and remove test containers
echo "Stopping test containers..."
docker ps -aq --filter "name=postgres-test" --filter "name=qdrant-test" | xargs -r docker stop
echo "Removing test containers..."
docker ps -aq --filter "name=postgres-test" --filter "name=qdrant-test" | xargs -r docker rm -f

# Count what was cleaned up
CONTAINERS_CLEANED=$(docker ps -a | grep -cE "(qdrant-test|postgres-test)" || echo "0")

echo "✅ Cleanup complete!"
echo "   Containers remaining: $CONTAINERS_CLEANED"
