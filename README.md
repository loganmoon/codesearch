# codesearch

## Multi-Repository Support

Codesearch automatically manages shared infrastructure for multiple repositories.

### First Use

Simply run from any repository:
```bash
cd /path/to/repo
codesearch index
```

The first run will automatically:
1. Create infrastructure directory at `~/.codesearch/infrastructure/`
2. Download and start containers (PostgreSQL, Qdrant, vLLM, outbox-processor)
3. Wait for all services to be healthy (may take 1-2 minutes for vLLM to load models)
4. Index your repository

**Security**: All services are bound to 127.0.0.1 only (not accessible from network).

### Subsequent Repositories

From any other repository, just run:
```bash
cd /path/to/another-repo
codesearch index
```

It will detect existing infrastructure and connect automatically. No configuration needed!

### How It Works

The first `codesearch index` or `codesearch serve` command will:
- Detect no shared infrastructure exists
- Acquire a lock file (`~/.codesearch/.infrastructure.lock`) to prevent race conditions
- Write `docker-compose.yml` to `~/.codesearch/infrastructure/`
- Start all services and wait for health checks
- Release the lock and proceed with indexing

Subsequent commands detect running infrastructure and connect directly.

### Verifying Multi-Repository Setup

Check PostgreSQL for multiple repositories:
```bash
docker exec codesearch-postgres psql -U codesearch -d codesearch -c \
  "SELECT repository_name, collection_name FROM repositories;"
```
Should show multiple rows (one per indexed repository).

Check Qdrant for multiple collections:
```bash
curl -s http://localhost:6333/collections | jq '.result.collections'
```
Should show multiple collections (one per repository).

### Manual Control (Optional)

View infrastructure status:
```bash
docker ps | grep codesearch
```

Stop shared infrastructure:
```bash
cd ~/.codesearch/infrastructure
docker compose down
```

Restart shared infrastructure:
```bash
cd ~/.codesearch/infrastructure
docker compose up -d
```

### Troubleshooting

**Lock timeout**: If initialization hangs, remove stale lock:
```bash
rm ~/.codesearch/.infrastructure.lock
```

**Port conflicts**: Ensure ports 5432, 6333, 6334, and 8000 are available.

**vLLM startup**: First run may take 1-2 minutes for vLLM to download and load the embedding model.