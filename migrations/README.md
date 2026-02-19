# Database Migrations

Migrations are automatically applied when the Postgres container starts via `docker-entrypoint-initdb.d`.

## Files

| Migration | Description |
|-----------|-------------|
| `001_initial_schema.sql` | UUID extension, entity metadata, outbox, and repository tables |
| `002_simplified_entity_storage.sql` | Consolidate Postgres as single source of truth, remove version history |
| `003_add_last_indexed_commit.sql` | Track last indexed git commit for catch-up indexing |
| `004_add_collection_to_outbox.sql` | Add collection_name to outbox for multi-repository support |
| `005_optimize_outbox_global_ordering.sql` | Global ordering index for single-query batch processing |
| `006_embedding_cache.sql` | Entity embeddings table with content-based deduplication (XxHash3_128) |
| `007_add_bm25_statistics.sql` | BM25 statistics columns (avgdl, token counts) for hybrid search |
| `008_add_sparse_embeddings_and_repository_to_entity_embeddings.sql` | Repository-aware deduplication for dense and sparse embeddings |
| `009_fix_sparse_embedding_schema.sql` | Fix sparse embedding schema (separate indices/values columns) |
| `010_deterministic_repository_ids.sql` | Change repository_id from UUID v4 to deterministic UUID v5 |
| `012_neo4j_integration.sql` | Graph-ready flag and per-repository Neo4j database name |
| `013_add_pending_relationship_resolution.sql` | Flag to track when relationships need resolution |
| `014_add_content_column_and_fts.sql` | Dedicated content column with full-text search GIN index |
| `015_optimize_fts_with_generated_column.sql` | Store tsvector in generated column for FTS optimization |
| `016_pending_relationships.sql` | Pending relationships table for unresolved entity relationships |
| `017_optimize_outbox_global_order_simple.sql` | Optimized index for global ordering by created_at |
| `018_remove_fts.sql` | Remove FTS infrastructure (replaced by Granite sparse embeddings) |
| `019_add_path_entity_identifier.sql` | Path entity identifier for import resolution and cross-file references |

Note: Migration 011 does not exist (numbering gap).

## Manual Execution

To apply a specific migration to a running database:
```bash
docker exec -i codesearch-postgres psql -U codesearch -d codesearch < migrations/001_initial_schema.sql
```

## Verification

Check applied migrations:
```bash
docker exec codesearch-postgres psql -U codesearch -d codesearch -c "\dt"
```
