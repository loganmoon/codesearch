# End-to-End Testing Guide

This guide covers the comprehensive E2E test suite for codesearch, which validates the complete pipeline (CLI → Indexer → Embeddings → Storage) using isolated Qdrant containers.

## Overview

The E2E test suite provides:
- Isolated test environments with temporary Qdrant containers
- Guaranteed cleanup via RAII (Drop trait)
- Configurable verbose logging for debugging
- Reusable test fixtures and utilities
- Coverage of happy path, error cases, and edge cases

## Test Structure

```
crates/cli/tests/
├── e2e/                      # Test infrastructure
│   ├── mod.rs               # Module exports
│   ├── containers.rs        # Qdrant container management
│   ├── fixtures.rs          # Test repository builders
│   ├── logging.rs           # Test logging utilities
│   ├── cleanup.rs           # Cleanup verification
│   └── assertions.rs        # Domain-specific assertions
├── e2e_tests.rs             # Main E2E test suite
└── e2e_benchmarks.rs        # Performance benchmarks
```

## Running Tests

### Prerequisites

- Docker installed and running
- Rust toolchain
- No other Qdrant instances using ports 6333-6334

### Basic Tests (Mock Embeddings)

Run the complete E2E test suite with mock embeddings:

```bash
cargo test --test e2e_tests
```

This runs all tests including:
- Init command tests
- Index command tests
- Search functionality tests
- Error handling tests
- Edge case tests
- Concurrent execution tests

### Individual Test Suites

Run specific test categories:

```bash
# Core functionality
cargo test --test e2e_tests test_init_creates_collection
cargo test --test e2e_tests test_index_stores_entities

# Error handling
cargo test --test e2e_tests test_index_without_init
cargo test --test e2e_tests test_init_with_unreachable_qdrant

# Edge cases
cargo test --test e2e_tests test_empty_repository
cargo test --test e2e_tests test_large_entity_is_skipped

# Concurrent execution
cargo test --test e2e_tests test_concurrent_indexing
```

### Tests with Real Embeddings (Requires vLLM)

Some tests require a running vLLM server and are marked `#[ignore]`:

```bash
# Start vLLM first
docker compose up -d vllm-embeddings

# Run ignored tests
cargo test --test e2e_tests test_complete_pipeline_with_real_embeddings -- --ignored
```

### Verbose Logging

Enable detailed logging for debugging:

```bash
# Using CODESEARCH_TEST_LOG (test-specific)
CODESEARCH_TEST_LOG=debug cargo test --test e2e_tests

# Using RUST_LOG (general)
RUST_LOG=debug cargo test --test e2e_tests

# Trace-level logging
CODESEARCH_TEST_LOG=trace cargo test --test e2e_tests -- --nocapture
```

### Performance Benchmarks

Run performance benchmarks separately:

```bash
cargo test --test e2e_benchmarks -- --ignored --nocapture
```

Available benchmarks:
- `benchmark_indexing_speed` - Measures indexing throughput
- `benchmark_search_latency` - Measures search latency (p50, p95, p99)
- `benchmark_large_repository` - Tests with 100 files

## Test Isolation

Each test uses isolated resources:

1. **Unique Qdrant Container**: Each test gets its own container with UUID-based name
2. **Dynamic Ports**: Ports allocated via `portpicker` to avoid conflicts
3. **Temporary Storage**: All data stored in `/tmp/qdrant-test-*` directories
4. **Automatic Cleanup**: Resources cleaned up via `Drop` trait implementation

This allows tests to run:
- Concurrently without conflicts
- Repeatedly without state pollution
- Safely without affecting the host system

## Test Fixtures

Pre-built test repositories are available:

### simple_rust_repo()
- 1 file with 2-3 functions, 1 struct
- Expected: 3-5 entities
- Use for: Quick tests, basic functionality

### multi_file_rust_repo()
- 3 files: main.rs, lib.rs, utils.rs
- Expected: 10-15 entities
- Use for: Testing multiple file handling, module extraction

### complex_rust_repo()
- 4+ files with nested modules, traits, implementations
- Expected: 20-30 entities
- Use for: Realistic scenarios, comprehensive testing

### Custom Repositories

Create custom test repositories using `TestRepositoryBuilder`:

```rust
use e2e::TestRepositoryBuilder;

let repo = TestRepositoryBuilder::new("custom")
    .with_rust_file("lib.rs", "pub fn test() {}")
    .with_rust_file("main.rs", "fn main() {}")
    .with_git_init(true)
    .build()
    .await?;
```

## Custom Assertions

Domain-specific assertions for E2E tests:

```rust
use e2e::*;

// Assert collection exists
assert_collection_exists(&qdrant, "my_collection").await?;

// Assert exact point count
assert_point_count(&qdrant, "my_collection", 42).await?;

// Assert minimum point count
assert_min_point_count(&qdrant, "my_collection", 10).await?;

// Assert entity exists
let expected = ExpectedEntity::new("my_function", EntityType::Function, "main.rs");
assert_entity_in_qdrant(&qdrant, "my_collection", &expected).await?;

// Assert vector dimensions
assert_vector_dimensions(&qdrant, "my_collection", 384).await?;
```

## Troubleshooting

### Orphaned Containers

If tests fail or are interrupted, containers may not be cleaned up:

```bash
# List orphaned test containers
docker ps -a | grep qdrant-test-

# Remove all test containers
docker rm -f $(docker ps -aq --filter name=qdrant-test-)
```

### Orphaned /tmp Directories

Check for leftover temporary directories:

```bash
# List orphaned directories
ls -la /tmp/qdrant-test-* /tmp/codesearch-test-*

# Remove all test directories
sudo rm -rf /tmp/qdrant-test-* /tmp/codesearch-test-*
```

### Port Conflicts

If tests fail with "address already in use":

1. Check for running Qdrant instances: `docker ps | grep qdrant`
2. Stop conflicting containers: `docker stop <container_id>`
3. The test suite uses dynamic port allocation to avoid conflicts

### Container Startup Failures

If containers fail to start:

1. Check Docker is running: `docker ps`
2. Check Docker has sufficient resources
3. Review container logs for specific test:
   ```bash
   CODESEARCH_TEST_LOG=debug cargo test --test e2e_tests test_name -- --nocapture
   ```

### Test Timeouts

If tests hang:

1. Container health checks may be failing
2. Check Qdrant container logs: `docker logs <container_name>`
3. Verify network connectivity to Docker containers

## Cleanup Verification

The test suite includes cleanup verification utilities:

```rust
use e2e::cleanup::*;

// Verify no orphaned containers
verify_no_orphaned_containers()?;

// Verify no orphaned temp directories
verify_no_orphaned_temp_dirs()?;

// Clean up all orphaned resources
cleanup_all_orphaned_resources()?;
```

## Performance Expectations

With mock embeddings:
- Simple repository (3-5 entities): < 5 seconds
- Multi-file repository (10-15 entities): < 10 seconds
- Complex repository (20-30 entities): < 15 seconds
- Container startup: ~3 seconds (health check polling)

With real embeddings (vLLM):
- Add 100-500ms per entity for embedding generation
- Total time: 30 seconds - 3 minutes depending on repository size

## Writing New E2E Tests

### Template for New Tests

```rust
#[tokio::test]
async fn test_my_feature() -> Result<()> {
    init_test_logging();

    let qdrant = TestQdrant::start().await?;
    let repo = simple_rust_repo().await?;

    let collection_name = format!("test_collection_{}", Uuid::new_v4());
    let config_path = create_test_config(repo.path(), &qdrant, Some(&collection_name))?;

    // Run init
    run_cli(repo.path(), &["init", "--config", config_path.to_str().unwrap()])?;

    // Your test logic here

    // Verify results
    assert_collection_exists(&qdrant, &collection_name).await?;

    Ok(())
}
```

### Best Practices

1. **Always use `init_test_logging()`** at the start of tests
2. **Use unique collection names** with UUID to avoid conflicts
3. **Clean up explicitly** if creating resources outside TestQdrant
4. **Use fixtures** for common repository patterns
5. **Use custom assertions** for readability
6. **Mark expensive tests** with `#[ignore]` and document requirements

## Integration with CI/CD

The E2E test suite is designed for manual testing, but can be integrated into CI:

```yaml
# Example GitHub Actions workflow
- name: Run E2E Tests
  run: |
    docker pull qdrant/qdrant
    cargo test --test e2e_tests

- name: Cleanup
  if: always()
  run: |
    docker rm -f $(docker ps -aq --filter name=qdrant-test-) || true
```

## References

- [Original Plan](../docs/plans/e2e_test_suite.md)
- [Container Management](../crates/cli/tests/e2e/containers.rs)
- [Test Fixtures](../crates/cli/tests/e2e/fixtures.rs)
- [Assertions](../crates/cli/tests/e2e/assertions.rs)
