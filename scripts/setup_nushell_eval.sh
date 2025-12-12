#!/bin/bash
# Setup script for Nushell evaluation suite
# Clones Nushell at a pinned commit and indexes it with codesearch

set -e

NUSHELL_COMMIT="11d71fe3c78670a8922143dd7e8b8f20b71a97d0"
NUSHELL_DIR="/tmp/nushell-eval"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "=== Nushell Evaluation Setup ==="
echo "Commit: $NUSHELL_COMMIT"
echo "Directory: $NUSHELL_DIR"

# Clone if needed
if [ ! -d "$NUSHELL_DIR" ]; then
    echo "Cloning Nushell repository..."
    git clone https://github.com/nushell/nushell.git "$NUSHELL_DIR"
    cd "$NUSHELL_DIR"
    git fetch origin "$NUSHELL_COMMIT"
    git checkout "$NUSHELL_COMMIT"
else
    echo "Nushell directory exists, checking commit..."
    cd "$NUSHELL_DIR"
    CURRENT_COMMIT=$(git rev-parse HEAD)
    if [ "$CURRENT_COMMIT" != "$NUSHELL_COMMIT" ]; then
        echo "Updating to pinned commit..."
        git fetch origin "$NUSHELL_COMMIT"
        git checkout "$NUSHELL_COMMIT"
    else
        echo "Already at correct commit"
    fi
fi

# Always rebuild codesearch to ensure we're using the latest code
echo "Building codesearch (release)..."
cd "$PROJECT_ROOT"
cargo build --workspace --all-targets --release
CODESEARCH_BIN="$PROJECT_ROOT/target/release/codesearch"

echo "Using codesearch: $CODESEARCH_BIN"

# Index Nushell
cd "$NUSHELL_DIR"
echo "Indexing Nushell repository..."
"$CODESEARCH_BIN" index --verbose

sleep 5

"$CODESEARCH_BIN" serve

echo ""
echo "=== Setup Complete ==="
echo "Nushell is now indexed at: $NUSHELL_DIR"
echo ""
echo "Run the evaluation with:"
echo "  cargo test --package codesearch-e2e-tests --test test_nushell_evaluation -- --ignored --nocapture"
echo ""
echo "For quick iteration, use SAMPLE_LIMIT:"
echo "  SAMPLE_LIMIT=5 cargo test --package codesearch-e2e-tests --test test_nushell_evaluation -- --ignored --nocapture"
