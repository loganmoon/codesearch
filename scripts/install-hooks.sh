#!/bin/bash

# Install git hooks for codesearch project
# Run this script from the project root directory

set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

print_status() {
    local color=$1
    local message=$2
    echo -e "${color}${message}${NC}"
}

# Check if we're in the project root
if [ ! -f "Cargo.toml" ] || [ ! -d ".git" ]; then
    print_status $RED "❌ Error: Please run this script from the project root directory"
    exit 1
fi

print_status $BLUE "🔧 Installing git hooks for codesearch project..."

# Check the configured hooks path
hooks_path=$(git config core.hooksPath || echo ".git/hooks")
print_status $BLUE "Using hooks path: $hooks_path"

# Create hooks directory if it doesn't exist
mkdir -p "$hooks_path"

# Copy hooks
cp scripts/hooks/pre-commit "$hooks_path/pre-commit"
cp scripts/hooks/pre-merge-commit "$hooks_path/pre-merge-commit"

# Make them executable
chmod +x "$hooks_path/pre-commit"
chmod +x "$hooks_path/pre-merge-commit"

print_status $GREEN "✅ Git hooks installed successfully!"
print_status $YELLOW "The following checks will now run before each commit:"
print_status $YELLOW "  • Prevent commits/merges to main branch"
print_status $YELLOW "  • Code formatting (cargo fmt)"
print_status $YELLOW "  • Linting (cargo clippy)"
print_status $YELLOW "  • Tests (cargo test)"
print_status $BLUE "💡 To run these checks manually: cargo fmt && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features"