# Multi-stage Dockerfile for Codesearch application

# Stage 1: Builder
FROM rust:1.88.0-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /build

# Copy workspace configuration first (for better caching)
COPY Cargo.toml Cargo.lock ./

# Copy all crate directories
COPY crates/ ./crates/

# Build the application in release mode
RUN cargo build --release --workspace --bin codesearch

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN useradd -m -u 1001 -U codesearch

# Create workspace directory
RUN mkdir -p /workspace && chown codesearch:codesearch /workspace

# Copy binary from builder
COPY --from=builder /build/target/release/codesearch /usr/local/bin/codesearch

# Set ownership and permissions
RUN chmod +x /usr/local/bin/codesearch

# Switch to non-root user
USER codesearch

# Set working directory
WORKDIR /workspace

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD codesearch --help || exit 1

# Default command
ENTRYPOINT ["codesearch"]
CMD ["--help"]