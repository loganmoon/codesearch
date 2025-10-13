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
3. **Linting**: Runs `cargo clippy` with strict warnings as errors
4. **Testing**: Runs the full test suite to ensure no regressions
5. **TODO Detection**: Warns about TODO/FIXME comments (non-blocking)

### Manual Quality Checks

You can run the same checks manually:

```bash
# Format code
cargo fmt

# Run linting
cargo clippy --workspace --all-targets --no-default-features -- -D warnings

# Run tests
cargo test --workspace --no-default-features

# Run all checks at once
cargo fmt && cargo clippy --workspace --all-targets --no-default-features -- -D warnings && cargo test --workspace --no-default-features
```

## Development Workflows

### Outbox Processor Development Mode

For rapid iteration when developing the outbox processor:

#### Setup (One-time)

1. Install cargo-watch:
   ```bash
   cargo install cargo-watch
   ```

2. Build initial binary:
   ```bash
   cargo build --release --bin outbox-processor
   ```

#### Usage

**Terminal 1: Start auto-rebuild watch**
```bash
./scripts/dev-watch-outbox.sh
```

**Terminal 2: Start infrastructure with development mode**
```bash
cd infrastructure
docker compose -f docker-compose.yml -f docker-compose.dev.yml up
```

#### How It Works

- cargo-watch monitors source files and rebuilds on changes (10-30 seconds)
- Docker container mounts the binary via volume
- Container restarts automatically pick up new binary
- No Docker rebuild needed during development

#### When to Use Each Mode

- **Development mode** (volume mount): Active development on outbox processor
- **Optimized Docker** (cached layers): Testing containerization, integration testing
- **Skip build** (existing image): Working on other components

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

Direct commits and merges to the `main` branch are blocked. Use this workflow instead:

1. Create a feature branch: `git checkout -b feat/your-feature`
2. Make your changes and commit: `git add . && git commit -m "your changes"`
3. Push to remote: `git push -u origin feat/your-feature`
4. Create a pull request for review
5. Merge through the pull request interface

This ensures all code is reviewed and maintains the quality of the main branch.