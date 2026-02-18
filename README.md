# Codesearch

A semantic code search engine that indexes codebases using AST-based entity extraction, vector embeddings, and graph relationships.

## Quick Start

**Prerequisites:**
- Rust 1.91+
- Docker and Docker Compose
- Jina API key (free tier available at jina.ai) or NVIDIA GPU for self-hosted embeddings

**Installation:**
```bash
cargo install --path crates/cli
```

**First Index:**
```bash
export JINA_API_KEY="your-api-key"
cd /path/to/your/repo
codesearch index
```

**Start Server:**
```bash
codesearch serve
```

**Search:**
```bash
curl -X POST http://localhost:3000/api/v1/search/semantic \
  -H "Content-Type: application/json" \
  -d '{"repository_ids": ["..."], "query": {"text": "authentication handler"}, "limit": 10}'
```

## Configuration

Configuration file: `~/.codesearch/config.toml`

### Embedding Providers

**Jina (default)** - Zero-config, no GPU required:
```toml
[embeddings]
provider = "jina"
api_key = "your-jina-api-key"  # or set JINA_API_KEY env var
model = "jina-embeddings-v3"
```

**LocalApi (vLLM)** - Self-hosted, requires GPU:
```toml
[embeddings]
provider = "localapi"
api_base_url = "http://localhost:8000/v1"
model = "BAAI/bge-large-en-v1.5"
```

### Sparse Embeddings

Sparse embeddings are used alongside dense embeddings for hybrid search. Configured separately:
```toml
[sparse_embeddings]
provider = "granite"  # "granite" (default, learned sparse, requires model download) or "bm25"
device = "auto"       # "auto", "cpu", "cuda", "metal"
top_k = 256           # Sparse dimensions to keep
```

### Reranking

Optional cross-encoder reranking for improved relevance:
```toml
[reranking]
enabled = true
provider = "jina"
api_key = "your-jina-api-key"  # or set JINA_API_KEY env var
```

### Key Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `embeddings.provider` | `jina` | `jina`, `localapi`, or `mock` |
| `embeddings.embedding_dimension` | `1024` | Vector dimension size |
| `sparse_embeddings.provider` | `granite` | `granite` (learned sparse) or `bm25` |
| `reranking.enabled` | `false` | Enable cross-encoder reranking |
| `reranking.candidates` | `100` | Number of candidates to rerank |
| `languages.enabled` | `["rust"]` | Enabled languages (`rust`, `javascript`, `typescript`) |
| `server.port` | `3000` | REST API server port |

## CLI Commands

| Command | Description |
|---------|-------------|
| `codesearch index` | Index the current repository. Use `--force` to re-index from scratch. |
| `codesearch serve` | Start the REST API server. Use `--enable_agentic` for agentic search (requires `ANTHROPIC_API_KEY`). |
| `codesearch drop` | Drop all indexed data for the current repository. |
| `codesearch cache stats` | Show embedding cache statistics. |
| `codesearch cache clear` | Clear embedding cache. Use `--model <name>` to clear for a specific model only. |
| `codesearch mcp` | Start MCP server for Claude Code integration (stdio transport). |

## Architecture

**Crates:**
- `core` - Foundation types, configuration, error handling
- `languages` - AST parsing with spec-driven YAML configuration (Rust, JavaScript/TypeScript)
- `embeddings` - Vector embedding providers (Jina, vLLM/OpenAI API)
- `reranking` - Cross-encoder result reranking
- `indexer` - Repository indexing with Git integration
- `outbox-processor` - Reliable event processing and consistency
- `watcher` - Real-time file system monitoring
- `storage` - Postgres, Qdrant, Neo4j persistence
- `agentic-search` - Multi-agent query orchestration
- `mcp-server` - Model Context Protocol implementation
- `server` - REST API server
- `cli` - Command-line interface

**Infrastructure:**
- PostgreSQL - Entity metadata storage
- Qdrant - Dense and sparse vector storage with hybrid search
- Neo4j - Code relationship graph (calls, imports, inherits)

## API Reference

### Search Endpoints

**Semantic Search** (recommended):
```
POST /api/v1/search/semantic
```
Hybrid search combining dense embeddings + sparse retrieval (Granite learned sparse or BM25) with RRF fusion.

**Agentic Search**:
```
POST /api/v1/search/agentic
```
Multi-agent orchestration using LLMs to answer complex queries by traversing the code graph and aggregating results. Requires `ANTHROPIC_API_KEY` env var.

**Graph Query**:
```
POST /api/v1/graph/query
```
Query code relationships (e.g., "functions that call X").

### Management Endpoints

```
GET  /api/v1/repositories           # List indexed repositories
POST /api/v1/entities/batch          # Batch entity retrieval by ID
POST /api/v1/embed                   # Generate embeddings
GET  /health                         # Health check
```

## Development

**Build:**
```bash
cargo build --workspace --all-targets
```

**Test:**
```bash
cargo test --workspace --no-default-features                        # Unit tests
cargo test --manifest-path crates/e2e-tests/Cargo.toml -- --ignored # E2E tests (requires running infrastructure)
```

**Lint:**
```bash
cargo fmt --check
cargo clippy --workspace --all-targets --no-default-features -- -D warnings
```

**Run from source:**
```bash
cargo run -- index
cargo run -- serve
```

## Infrastructure Management

The CLI automatically manages infrastructure in `~/.codesearch/infrastructure` (starts on first `codesearch index`). To manage manually:

```bash
# Check status
docker ps --filter "name=codesearch"

# Stop all services
cd ~/.codesearch/infrastructure && docker compose down

# View logs
docker compose -f ~/.codesearch/infrastructure/docker-compose.yml logs -f
```

## License

MIT
