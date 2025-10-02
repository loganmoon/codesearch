# Database Migrations

Migrations are automatically applied when the Postgres container starts via `docker-entrypoint-initdb.d`.

## Files
- `001_initial_schema.sql` - Core tables for entity metadata, versioning, and outbox pattern

## Manual Execution
To apply migrations to a running database:
```bash
docker exec -i codesearch-postgres psql -U codesearch -d codesearch < migrations/001_initial_schema.sql
```

## Verification
Check applied migrations:
```bash
docker exec codesearch-postgres psql -U codesearch -d codesearch -c "\dt"
```
