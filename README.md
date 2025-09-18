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

## Installation

### Using Docker

```bash
# Clone the repository
git clone https://github.com/yourusername/codesearch.git
cd codesearch

# Build and run with Docker Compose
docker-compose up --build

# Or build the Docker image directly
docker build -t codesearch .
```

### From Source

```bash
# Clone the repository
git clone https://github.com/yourusername/codesearch.git
cd codesearch

# Build the project
cargo build --release

# Install to PATH (optional)
cargo install --path crates/cli
```

## Quick Start

1. **Initialize a repository for indexing:**

```bash
# Navigate to your project directory
cd /path/to/your/project

# Index the repository
codesearch index
```

2. **Search for code:**

```bash
# Search for specific code patterns
codesearch search "function that handles authentication"

# Limit results
codesearch search "database connection" --limit 5
```

3. **Start the MCP server:**

```bash
# Start the Model Context Protocol server
codesearch serve

# Or specify a custom port
codesearch serve --port 8080
```

## Configuration

Configuration is stored in `.codesearch/config.toml` within your repository:

```toml
[indexing]
ignore_patterns = [
    "target/",
    "node_modules/",
    ".git/",
    "*.min.js"
]
languages = ["rust", "python", "javascript", "typescript", "go"]

[embeddings]
provider = "embed_anything"  # or "openai"
model = "all-MiniLM-L6-v2"

[storage]
provider = "qdrant"
host = "localhost"
port = 6334
collection_name = "code_entities"

[server]
host = "localhost"
port = 8699
```

## Architecture

The project is organized as a Rust workspace with the following crates:

- **`core`**: Foundation types, entities, configuration, and error handling
- **`languages`**: AST parsing and entity extraction for supported languages
- **`embeddings`**: Vector embedding providers (local and remote)
- **`indexer`**: Repository indexing logic with Git integration
- **`watcher`**: Real-time file system monitoring
- **`storage`**: Persistent storage layer for indexed data
- **`cli`**: Command-line interface and MCP server

## Development

### Building

```bash
# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Run with verbose logging
RUST_LOG=debug cargo run -- index
```

### Adding Language Support

To add support for a new language:

1. Add the tree-sitter grammar to `crates/languages/Cargo.toml`
2. Create a new module in `crates/languages/src/`
3. Implement the entity extraction logic
4. Register the language in the extractor factory

## MCP Integration

Codesearch implements the Model Context Protocol (MCP) for seamless integration with AI assistants:

```bash
# Start the MCP server
codesearch serve

# The server communicates via stdio by default
# Configure your AI assistant to connect to the codesearch MCP server
```

### Available MCP Tools

- `search_code`: Search for code entities by semantic meaning
- `get_entity`: Retrieve detailed information about a specific code entity
- `list_files`: List indexed files in the repository
- `get_file_entities`: Get all entities within a specific file

## Storage Backends

### Qdrant (Default)

Codesearch uses Qdrant for vector storage:

```bash
# Start Qdrant using Docker
docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant

# Configure in .codesearch/config.toml
[storage]
provider = "qdrant"
host = "localhost"
port = 6334
```

### Custom Storage

Implement the `StorageClient` trait to add custom storage backends.

## Performance

Codesearch is designed for performance:

- **Parallel Processing**: Files are processed in parallel batches
- **Memory Management**: Configurable memory limits for large repositories

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Built with [tree-sitter](https://tree-sitter.github.io/) for robust AST parsing
- Vector storage powered by [Qdrant](https://qdrant.tech/)
- Embeddings via [embed_anything](https://github.com/StarlightSearch/EmbedAnything) (which uses [Candle](https://github.com/huggingface/candle) under the hood) and [OpenAI](https://openai.com/)