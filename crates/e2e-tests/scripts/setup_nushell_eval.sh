#!/bin/bash
# Setup nushell repository at the exact commit used for ground truth extraction
#
# Usage: ./setup_nushell_eval.sh [target_dir]
#
# Default target: /tmp/nushell

set -e

REPO_URL="https://github.com/nushell/nushell.git"
COMMIT="11d71fe3c78670a8922143dd7e8b8f20b71a97d0"
TARGET_DIR="${1:-/tmp/nushell}"

echo "Setting up nushell repository for semantic search evaluation"
echo "  Repository: $REPO_URL"
echo "  Commit:     $COMMIT"
echo "  Target:     $TARGET_DIR"
echo

if [ -d "$TARGET_DIR" ]; then
    echo "Directory exists, checking current commit..."
    cd "$TARGET_DIR"

    CURRENT_COMMIT=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
    if [ "$CURRENT_COMMIT" = "$COMMIT" ]; then
        echo "Already at correct commit: $COMMIT"
        exit 0
    fi

    echo "Current commit: $CURRENT_COMMIT"
    echo "Fetching and checking out required commit..."
    git fetch origin
    git checkout "$COMMIT"
else
    echo "Cloning repository (this may take a few minutes)..."
    git clone --no-checkout "$REPO_URL" "$TARGET_DIR"
    cd "$TARGET_DIR"

    echo "Checking out commit $COMMIT..."
    git checkout "$COMMIT"
fi

echo
echo "Setup complete!"
echo "Repository is at: $TARGET_DIR"
echo "Commit: $(git rev-parse HEAD)"
echo
echo "Next steps:"
echo "  1. Index the repository: codesearch index $TARGET_DIR"
echo "  2. Start the server:     codesearch serve"
echo "  3. Run the evaluation:   cargo test --manifest-path crates/e2e-tests/Cargo.toml --test test_semantic_search_eval -- --ignored --nocapture"
