#!/usr/bin/env bash
# Development helper for outbox processor with cargo-watch
#
# This script provides rapid iteration for outbox processor development by:
# 1. Using cargo-watch to rebuild on source changes (10-30 seconds)
# 2. Using volume mounts to avoid Docker rebuilds
# 3. Automatically restarting the container when binary changes
#
# Prerequisites: cargo install cargo-watch

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "Starting outbox processor development mode..."
echo "This will watch for changes and rebuild automatically."
echo ""

# Check if cargo-watch is installed
if ! command -v cargo-watch &> /dev/null; then
    echo "ERROR: cargo-watch not found"
    echo "Install with: cargo install cargo-watch"
    exit 1
fi

# Check if docker-compose.dev.yml exists
if [ ! -f "${REPO_ROOT}/infrastructure/docker-compose.dev.yml" ]; then
    echo "ERROR: infrastructure/docker-compose.dev.yml not found"
    exit 1
fi

# Build initial binary
echo "Building initial release binary..."
cd "${REPO_ROOT}"
cargo build --release --bin outbox-processor

echo ""
echo "Starting cargo-watch..."
echo "Changes to crates/outbox-processor, crates/core, or crates/storage will trigger rebuild"
echo ""

# Watch and rebuild (this runs in foreground)
cargo watch \
    -x "build --release --bin outbox-processor" \
    -w crates/outbox-processor \
    -w crates/core \
    -w crates/storage \
    --ignore "*/target/*" \
    --ignore "*.md"
