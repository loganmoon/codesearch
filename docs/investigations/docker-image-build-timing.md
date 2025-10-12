# Investigation: Move Docker Image Building to build.rs

## Issue
Code review comment on PR #51: `build_outbox_processor_image()` in `infrastructure.rs:118` builds the Docker image at runtime. The reviewer suggests this should be done at build time using a `build.rs` script.

## Current Implementation
Location: `crates/cli/src/infrastructure.rs:118-149`

The `build_outbox_processor_image()` function:
- Runs `docker build` from the current working directory
- Builds the outbox-processor image using `Dockerfile.outbox-processor`
- Requires running from the codesearch source repository root on first use
- Is called during `ensure_shared_infrastructure()` before starting containers

```rust
fn build_outbox_processor_image() -> Result<()> {
    info!("Building outbox-processor Docker image...");

    let repo_root = std::env::current_dir().context("Failed to get current directory")?;
    let dockerfile_path = repo_root.join("Dockerfile.outbox-processor");

    if !dockerfile_path.exists() {
        return Err(anyhow!(
            "Dockerfile.outbox-processor not found at {}. \
             Make sure you're running from a codesearch repository.",
            dockerfile_path.display()
        ));
    }

    let output = Command::new("docker")
        .args([/* ... */])
        .current_dir(&repo_root)
        .output()
        .context("Failed to execute docker build")?;

    // ...
}
```

## Proposed Solution: Move to build.rs

Create `crates/cli/build.rs` that builds the Docker image during `cargo build`:

```rust
// crates/cli/build.rs
fn main() {
    // Only build the Docker image if:
    // 1. Docker is available
    // 2. The Dockerfile exists
    // 3. We're building in release mode (optional)

    if is_docker_available() && dockerfile_exists() {
        build_docker_image();
    } else {
        // Emit warning but don't fail the build
        println!("cargo:warning=Docker not available or Dockerfile not found; skipping image build");
    }
}
```

## Challenges and Considerations

### 1. Development vs. Distribution
**Problem**: build.rs runs during `cargo build`, but:
- **Developers**: Need to rebuild the image frequently as code changes
- **End users**: Installing via `cargo install` shouldn't require Docker at build time

**Potential Solutions**:
- A) Make Docker optional in build.rs; runtime check if image exists, build then
- B) Separate binary distribution (with pre-built image) from source builds
- C) Keep current runtime building, add optional build.rs for CI/release builds

### 2. Build Context and Dependencies
**Problem**: Docker build needs:
- The entire repository as build context
- Access to Cargo.toml, source files, etc.
- build.rs runs before the binary is built

**Implications**:
- build.rs would need to know about workspace structure
- Any source changes would trigger Docker rebuild (slow)
- Unclear how to handle this cleanly in a multi-crate workspace

### 3. Cross-Platform Concerns
**Problem**:
- Windows/Mac users might not have Docker installed during `cargo build`
- Docker might not be available in CI environments
- Build should not fail if Docker is unavailable

**Solutions**:
- Make Docker image building optional/conditional
- Provide pre-built images for common platforms
- Fall back to runtime building if image doesn't exist

### 4. Image Versioning
**Problem**: How to version the Docker image?
- Based on Cargo version?
- Git commit hash?
- Separate version tracking?

Current runtime approach doesn't version; it always builds `:latest`

### 5. Incremental Builds
**Problem**: Rust incremental builds are fast, but Docker builds are slow
- Every `cargo build` that touches outbox-processor code would rebuild the entire Docker image
- This could significantly slow down development iteration

## Recommendation

**Do NOT move to build.rs** for the following reasons:

1. **User Experience**: Users installing via `cargo install` should not need Docker at build time
2. **Development Speed**: Forcing Docker builds on every `cargo build` would slow down development
3. **Complexity**: The build.rs approach adds significant complexity for unclear benefit
4. **Current Design Works**: The runtime approach:
   - Only builds once per machine (cached)
   - Provides clear error messages if Docker isn't available
   - Doesn't impact development iteration speed
   - Works consistently across all platforms

**Alternative Improvements**:

1. **Cache the image better**: Check if image exists and skip building if already present
2. **Version the image**: Tag with git hash or version number
3. **CI pre-building**: In CI/CD, pre-build and push the image to a registry
4. **Optional pre-built images**: Provide downloadable images for users who don't want to build

## Implementation Plan (If Proceeding)

If we still want to move to build.rs despite the concerns:

1. Create `crates/cli/build.rs`
2. Add conditional Docker building (fail gracefully if unavailable)
3. Update `infrastructure.rs` to check for existing image before building
4. Add version tagging based on git hash or Cargo.toml version
5. Update CI to ensure Docker is available during builds
6. Document the Docker requirement for building from source

## Related Files
- `crates/cli/src/infrastructure.rs` - Current implementation
- `Dockerfile.outbox-processor` - The Dockerfile being built
- `crates/cli/Cargo.toml` - Where build.rs would be referenced
