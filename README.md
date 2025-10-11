# codesearch

## Multi-Repository Setup

To index and serve multiple repositories simultaneously using shared infrastructure:

### Step 1: Start Shared Infrastructure (One-time)

1. Copy the infrastructure directory to a location of your choice:
   ```bash
   cp -r infrastructure ~/codesearch-infrastructure
   cd ~/codesearch-infrastructure
   ```

2. If your codesearch repository is not in the parent directory, update the `outbox-processor` build context in `docker-compose.yml` to point to your codesearch source.

3. Start shared services:
   ```bash
   docker compose up -d
   ```

4. Verify services are healthy:
   ```bash
   docker ps  # All containers should show "healthy"
   curl http://localhost:6333/health  # Qdrant should respond
   psql -h localhost -U codesearch -d codesearch -c "SELECT 1;"  # Postgres should respond
   ```

### Step 2: Index Each Repository

For each repository you want to index:

1. Navigate to repository:
   ```bash
   cd /path/to/repo-a
   ```

2. Set environment variables (create `.env.codesearch` file):
   ```bash
   cat > .env.codesearch <<EOF
   export POSTGRES_HOST=localhost
   export QDRANT_HOST=localhost
   export POSTGRES_PASSWORD=codesearch
   export CODESEARCH__STORAGE__AUTO_START_DEPS=false
   EOF
   ```

3. Index the repository:
   ```bash
   source .env.codesearch
   codesearch index
   ```

4. Start server (optional, for MCP):
   ```bash
   codesearch serve
   ```

### Verifying Multi-Repository Setup

1. Check PostgreSQL for multiple repositories:
   ```bash
   psql -h localhost -U codesearch -d codesearch -c \
     "SELECT repository_name, collection_name FROM repositories;"
   ```
   Should show multiple rows (one per indexed repository)

2. Check Qdrant for multiple collections:
   ```bash
   curl -s http://localhost:6333/collections | jq '.result.collections'
   ```
   Should show multiple collections (one per repository)

3. Monitor outbox processor logs:
   ```bash
   docker logs -f codesearch-outbox-processor
   ```
   Should show processing entries from multiple collections