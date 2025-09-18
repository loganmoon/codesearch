# Codesearch

A high-performance semantic code indexing system that enables intelligent code search through AST-based parsing, vector embeddings, and real-time file watching. Built with Rust for speed and reliability.

## Features

- **🔍 Semantic Code Search**: Search code by meaning, not just text matching
- **🌳 AST-Based Parsing**: Deep understanding of code structure across multiple languages
- **🚀 High Performance**: Built with Rust for blazing-fast indexing and search
- **🔄 Real-Time Updates**: Automatic re-indexing on file changes with intelligent file watching
- **🤖 MCP Integration**: Model Context Protocol server for AI assistant integration
- **📊 Vector Embeddings**: Support for both local and remote embedding models
- **🔧 Multi-Language Support**: Rust, Python, JavaScript/TypeScript, and Go

## Table of Contents

- [Installation](#installation)
- [Quick Start](#quick-start)
- [Usage](#usage)
- [Architecture](#architecture)
- [Configuration](#configuration)
- [Development](#development)
- [Contributing](#contributing)
- [License](#license)

## Installation

### Prerequisites

- Rust toolchain (1.75 or later)
- Docker and Docker Compose (for Qdrant vector database)
- Git (for repository integration features)

### Building from Source

```bash
# Clone the repository
git clone https://github.com/loganmoon/codesearch
cd codesearch

# Build the project
cargo build --release

# The binary will be available at ./target/release/codesearch
```

### Docker Setup

For running Qdrant vector database:

```bash
# Start Qdrant using Docker Compose
docker-compose up -d

## Quick Start

1. **Initialize Codesearch in your repository and create the index:**

```bash
codesearch index
```

This creates a `.codesearch/config.toml` configuration file in your repository and parses all supported code files and stores their semantic representations.

3. **Start the MCP server (optional):**

```bash
codesearch serve --port 8699
```

This enables integration with AI assistants and development tools.

4. **Search your code:**

```bash
codesearch search "function that handles authentication"
```

## Usage

### Command-Line Interface

```bash
# Index repository (with progress bar)
codesearch index --progress

# Force complete re-indexing
codesearch index --force

# Search indexed code
codesearch search "your search query" --limit 20

# Start MCP server
codesearch serve --host localhost --port 8699

# Enable verbose logging
codesearch --verbose <command>

# Use custom config file
codesearch --config path/to/config.toml <command>
```

### MCP Server Integration

The MCP (Model Context Protocol) server allows Codesearch to integrate with AI assistants and development tools:

```bash
# Start the server
codesearch serve

# The server provides semantic code search capabilities via the MCP protocol
# Configure your AI assistant to connect to http://localhost:8699
```

## Architecture

Codesearch is built as a modular Rust workspace with specialized crates:

### Core Components

- **`codesearch-core`**: Foundation types, entities, configuration, and error handling
- **`codesearch-languages`**: AST parsing and entity extraction for supported languages
- **`codesearch-embeddings`**: Vector embedding generation with local/remote provider support
- **`codesearch-indexer`**: Repository indexing with Git integration
- **`codesearch-watcher`**: Real-time file system monitoring with gitignore support
- **`codesearch-storage`**: Persistent storage layer for indexed data
- **`codesearch` (CLI)**: Command-line interface and MCP server

### Data Flow

1. **Parsing**: Source files are parsed into AST representations
2. **Entity Extraction**: Code entities (functions, classes, etc.) are extracted
3. **Embedding Generation**: Entities are converted to vector embeddings
4. **Storage**: Embeddings are stored in Qdrant vector database
5. **Search**: Queries are embedded and matched against stored vectors
6. **Retrieval**: Relevant code entities are returned with context

## Configuration

### Environment Variables

Create a `.env` file from the example:

```bash
cp .env.example .env
```

Key configuration options:

```bash
# Logging level (trace, debug, info, warn, error)
RUST_LOG=info

# Qdrant vector database
QDRANT_HOST=localhost
QDRANT_PORT=6334
QDRANT_COLLECTION=codesearch

# MCP server
APP_HOST=0.0.0.0
APP_PORT=8699

# Indexing
INDEX_BATCH_SIZE=100
INDEX_SHOW_PROGRESS=true

# File watching
WATCH_ENABLED=true
WATCH_DEBOUNCE_MS=500
```

### Configuration File

The `.codesearch/config.toml` file created by `codesearch index` contains:

- Repository-specific settings
- Ignore patterns for file watching
- Language-specific parsing options
- Embedding model configuration

## Development

### Building and Testing

```bash
# Build all crates
cargo build --workspace --all-targets

# Run all tests
cargo test --workspace

# Run tests for specific crate
cargo test --package codesearch-core

# Run with strict linting
cargo clippy --workspace

# Format code
cargo fmt

# Build optimized release version
cargo build --release --workspace --all-targets
```

### Project Structure

```
codesearch/
├── crates/
│   ├── core/           # Foundation types and configuration
│   ├── languages/      # Language parsers and AST handling
│   ├── embeddings/     # Embedding generation
│   ├── indexer/        # Repository indexing logic
│   ├── watcher/        # File system monitoring
│   ├── storage/        # Data persistence layer
│   └── cli/            # CLI and MCP server
├── docker-compose.yml  # Qdrant setup
├── Dockerfile          # Container build
└── README.md           # This file
```

### Code Quality Standards

- All code must pass `cargo clippy` with strict settings
- No `.unwrap()` or `.expect()` in production code
- All functions return `Result` types for error handling
- Comprehensive test coverage for core functionality
- Documentation for all public APIs

## Contributing

Contributions are welcome! Please follow these guidelines:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes with descriptive messages
4. Ensure all tests pass and code is formatted
5. Push to your fork and open a Pull Request

Please read the [CLAUDE.md](./CLAUDE.md) file for detailed development guidelines and architecture principles.

## License

This project is dual-licensed under either:

- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)

at your option.

## Acknowledgments

Built with excellent Rust crates including:
- [tree-sitter](https://tree-sitter.github.io/) for AST parsing
- [Qdrant](https://qdrant.tech/) for vector storage
- [tokio](https://tokio.rs/) for async runtime
- [clap](https://clap.rs/) for CLI interface

---

For bug reports and feature requests, please open an issue on [GitHub](https://github.com/loganmoon/codesearch/issues).