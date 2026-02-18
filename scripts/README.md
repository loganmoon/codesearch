# Development Scripts

This directory contains development and maintenance scripts for the codesearch project.

## Git Hooks

### Installation

To install git hooks for this project, run from the project root:

```bash
./scripts/install-hooks.sh
```

### Pre-commit Checks

The pre-commit hook enforces the following quality standards:

1. **Branch Protection**: Prevents direct commits to `main` branch
2. **Code Formatting**: Runs `cargo fmt --check` to ensure consistent formatting
3. **Linting**: Runs `cargo clippy --workspace --all-targets --no-default-features -- -D warnings`
4. **Testing**: Runs lib-only unit tests (`cargo test --workspace --lib --no-default-features`)
5. **TODO Detection**: Warns about TODO/FIXME comments (non-blocking)

### Manual Quality Checks

You can run the same checks manually:

```bash
# Format code
cargo fmt

# Run linting
cargo clippy --workspace --all-targets --no-default-features -- -D warnings

# Run tests (lib only, matching pre-commit hook)
cargo test --workspace --lib --no-default-features

# Run all checks at once (matches pre-commit hook)
cargo fmt --check && cargo clippy --workspace --all-targets --no-default-features -- -D warnings && cargo test --workspace --lib --no-default-features
```

### Hook Files

- `hooks/pre-commit`: Main pre-commit hook with all quality checks
- `hooks/pre-merge-commit`: Prevents direct merges to main branch
- `install-hooks.sh`: Script to install hooks for new contributors

### Bypassing Hooks (Not Recommended)

If you need to bypass hooks temporarily (strongly discouraged):

```bash
git commit --no-verify -m "emergency commit"
```

**Note**: This should only be used in genuine emergencies as it bypasses all quality checks.

### Branch Protection

Direct commits and merges to the `main` branch are blocked. This project uses git worktrees for parallel development:

1. Create a worktree from the parent directory: `git worktree add feat--your-feature -b feat/your-feature`
2. Work within the worktree directory: `cd feat--your-feature`
3. Make your changes and commit
4. Push to remote: `git push -u origin feat/your-feature`
5. Create a pull request for review
6. Merge through the pull request interface

**Important:** Never use `git checkout <branch>` inside a worktree. Each worktree is tied to its specific branch.