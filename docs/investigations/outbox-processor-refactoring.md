# Investigation: Outbox Processor Batch Logic Refactoring

## Issue
Code review comment on PR #51 (processor.rs:77):

> This and process_collection_batch() are very wrong. There should be a single query that starts a transaction, grouping by repository_id, without any extra "pre-check" query. Entities for all repos in the fetched batch should be processed, rather than querying repo by repo. We also need to incorporate a cursor or offset and limit to ensure that we process all available batches in order by created_at, regardless of how many unprocessed entities exist in the outbox.

## Current Implementation

### Two-Query Approach (WRONG)

**Query 1**: Get distinct collections with pending entries
```rust
async fn process_batch(&self) -> Result<()> {
    // Query 1: Get distinct collections
    let collections_with_work: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT collection_name
         FROM entity_outbox
         WHERE target_store = $1 AND processed_at IS NULL
         LIMIT 100"
    )
    .fetch_all(...)
    .await?;

    // Then loop through each collection
    for collection_name in collections_with_work {
        self.process_collection_batch(&collection_name).await?;
    }
}
```

**Query 2**: For each collection, fetch and lock entries
```rust
async fn process_collection_batch(&self, collection_name: &str) -> Result<()> {
    let mut tx = /* start transaction */;

    // Query 2: Lock and fetch for THIS collection
    let entries: Vec<OutboxEntry> = sqlx::query_as(
        "SELECT /* ... */
         FROM entity_outbox
         WHERE target_store = $1
           AND collection_name = $2
           AND processed_at IS NULL
         ORDER BY created_at ASC
         LIMIT $3
         FOR UPDATE SKIP LOCKED"
    )
    .fetch_all(&mut *tx)
    .await?;

    // Process entries and commit
}
```

### Problems with Current Approach

1. **Inefficient**: N+1 query problem - one query to get collections, then one query per collection
2. **Unfair processing**: Processes collections one by one; if collection A has 1M entries and collection B has 10, collection B waits
3. **No global ordering**: Entries are ordered within each collection, but not across collections
4. **Resource usage**: Each collection gets its own transaction
5. **Incomplete pagination**: LIMIT is per-collection, not global

## Proposed Solution

### Single-Query Approach with Global Pagination

```rust
async fn process_batch(&self) -> Result<()> {
    // Single transaction for the entire batch
    let mut tx = self.postgres_client.get_pool().begin().await?;

    // Single query: Fetch batch across ALL collections, ordered by created_at
    let entries: Vec<OutboxEntry> = sqlx::query_as(
        "SELECT outbox_id, repository_id, entity_id, operation, target_store,
                payload, created_at, processed_at, retry_count, last_error,
                collection_name
         FROM entity_outbox
         WHERE target_store = $1
           AND processed_at IS NULL
         ORDER BY created_at ASC
         LIMIT $2
         FOR UPDATE SKIP LOCKED"
    )
    .bind(TargetStore::Qdrant.to_string())
    .bind(self.batch_size)
    .fetch_all(&mut *tx)
    .await?;

    if entries.is_empty() {
        tx.commit().await?;
        return Ok(());
    }

    // Group entries by collection_name for processing
    let mut entries_by_collection: HashMap<String, Vec<OutboxEntry>> = HashMap::new();
    for entry in entries {
        entries_by_collection
            .entry(entry.collection_name.clone())
            .or_insert_with(Vec::new)
            .push(entry);
    }

    // Process all collections in this batch
    for (collection_name, collection_entries) in entries_by_collection {
        let storage_client = self
            .get_or_create_client_for_collection(&collection_name)
            .await?;

        // Separate INSERT/UPDATE from DELETE
        let mut insert_update_entries = Vec::new();
        let mut delete_entries = Vec::new();
        let mut failed_entry_ids = Vec::new();

        for entry in collection_entries {
            if entry.retry_count >= self.max_retries {
                failed_entry_ids.push(entry.outbox_id);
                continue;
            }

            match entry.operation.as_str() {
                "INSERT" | "UPDATE" => insert_update_entries.push(entry),
                "DELETE" => delete_entries.push(entry),
                _ => failed_entry_ids.push(entry.outbox_id),
            }
        }

        // Bulk mark failed entries
        if !failed_entry_ids.is_empty() {
            self.bulk_mark_processed_in_tx(&mut tx, &failed_entry_ids).await?;
        }

        // Process INSERT/UPDATE
        if !insert_update_entries.is_empty() {
            if let Err(e) = self
                .write_to_qdrant_insert_update(&storage_client, &insert_update_entries)
                .await
            {
                // Roll back and record failures
                tx.rollback().await?;
                let ids: Vec<Uuid> = insert_update_entries.iter().map(|e| e.outbox_id).collect();
                let mut failure_tx = self.postgres_client.get_pool().begin().await?;
                self.bulk_record_failures_in_tx(&mut failure_tx, &ids, &e.to_string()).await?;
                failure_tx.commit().await?;
                return Err(e);
            }

            let ids: Vec<Uuid> = insert_update_entries.iter().map(|e| e.outbox_id).collect();
            self.bulk_mark_processed_in_tx(&mut tx, &ids).await?;
        }

        // Process DELETE
        if !delete_entries.is_empty() {
            if let Err(e) = self
                .write_to_qdrant_delete(&storage_client, &delete_entries)
                .await
            {
                // Roll back and record failures
                tx.rollback().await?;
                let ids: Vec<Uuid> = delete_entries.iter().map(|e| e.outbox_id).collect();
                let mut failure_tx = self.postgres_client.get_pool().begin().await?;
                self.bulk_record_failures_in_tx(&mut failure_tx, &ids, &e.to_string()).await?;
                failure_tx.commit().await?;
                return Err(e);
            }

            let ids: Vec<Uuid> = delete_entries.iter().map(|e| e.outbox_id).collect();
            self.bulk_mark_processed_in_tx(&mut tx, &ids).await?;
        }
    }

    // Commit the entire batch across all collections
    tx.commit().await?;

    Ok(())
}
```

## Benefits of New Approach

1. **Fair Processing**: All collections processed proportionally based on oldest entries
2. **Global Ordering**: Entries processed in strict `created_at` order across all collections
3. **Efficient**: Single query to fetch batch, single transaction for entire batch
4. **Simpler**: Eliminates `process_collection_batch()` function
5. **Predictable**: LIMIT applies globally, not per-collection

## Edge Cases and Considerations

### 1. Failed Entries in One Collection
**Question**: If collection A fails Qdrant write, but collection B succeeds, what happens?

**Current Behavior**: Collections are independent; A failure doesn't affect B

**New Behavior**: All collections in the batch share a transaction
- If any Qdrant write fails, the entire batch is rolled back
- Failures are recorded in a separate transaction
- Next batch will retry the same entries

**Trade-off**: More atomic (all-or-nothing per batch) but potentially slower recovery from isolated failures

### 2. Collection Client Caching
**Current**: Clients cached in DashMap

**New**: Same caching strategy works; we create clients as needed for each collection in the batch

### 3. Large Batches with Many Collections
**Question**: What if batch_size=100 spans 50 different collections?

**Answer**: Fine - we fetch 100 entries total, group by collection, process each collection's subset

**Performance**: Should be similar or better (fewer total queries, single transaction)

### 4. Repository ID
**Code Review Mention**: "grouping by repository_id"

**Current Model**:
- `entity_outbox` has both `repository_id` and `collection_name`
- Each repository has its own collection in Qdrant
- Grouping by `collection_name` IS grouping by repository

**Action**: The proposed solution groups by `collection_name` which is correct

## Alternative Approaches Considered

### Option B: Cursor-Based Pagination
```rust
// Track last processed created_at timestamp
let entries: Vec<OutboxEntry> = sqlx::query_as(
    "SELECT /* ... */
     FROM entity_outbox
     WHERE target_store = $1
       AND processed_at IS NULL
       AND created_at > $2  -- Cursor
     ORDER BY created_at ASC
     LIMIT $3
     FOR UPDATE SKIP LOCKED"
)
.bind(TargetStore::Qdrant.to_string())
.bind(last_processed_timestamp)
.bind(self.batch_size)
```

**Pros**: Can resume from exact point after failure
**Cons**: Need to persist cursor state somewhere; adds complexity

**Decision**: Not needed - `processed_at IS NULL` naturally provides cursor behavior

### Option C: Window Functions for Fair Distribution
```rust
"SELECT /* ... */
 FROM (
     SELECT *,
            ROW_NUMBER() OVER (PARTITION BY collection_name ORDER BY created_at) as rn
     FROM entity_outbox
     WHERE target_store = $1 AND processed_at IS NULL
 ) t
 WHERE rn <= $2  -- Limit per collection
 ORDER BY created_at ASC
 FOR UPDATE SKIP LOCKED"
```

**Pros**: Guarantees equal distribution across collections
**Cons**: Complex query; doesn't respect global ordering

**Decision**: Not needed - global ordering is more important than fairness

## Migration Path

1. **Keep old code**: Rename current functions with `_v1` suffix
2. **Implement new code**: Add new `process_batch()` implementation
3. **Add feature flag**: Allow switching between implementations via config
4. **Test thoroughly**: Run both in parallel on test data, compare results
5. **Deploy gradually**: Enable for single repository first, then expand
6. **Remove old code**: After successful deployment, remove v1 implementation

## Testing Strategy

### Unit Tests (Mocked)
- Verify grouping logic works correctly
- Verify entries grouped by collection
- Verify transaction handling

### Integration Tests (Real DB)
- Create entries across multiple collections
- Verify processing order (oldest first globally)
- Verify all collections processed in single batch
- Verify transaction rollback behavior

### Load Tests
- 10,000 entries across 10 collections
- Measure processing time vs. current implementation
- Verify no memory leaks with large batches

## Performance Impact

**Expected**:
- Fewer queries = lower latency
- Single transaction = less overhead
- Better concurrency with `FOR UPDATE SKIP LOCKED`

**Potential Regression**:
- Larger transactions held longer
- If one collection's Qdrant is slow, entire batch waits

**Mitigation**:
- Keep batch_size reasonable (100-500)
- Monitor transaction duration
- Add timeout on Qdrant operations

## Implementation Checklist

- [ ] Update `process_batch()` to use single query
- [ ] Remove `process_collection_batch()` function
- [ ] Update logging to show entries grouped by collection
- [ ] Add integration tests
- [ ] Update documentation
- [ ] Benchmark against current implementation
- [ ] Deploy to staging
- [ ] Monitor for issues
- [ ] Deploy to production

## Related Files
- `crates/outbox-processor/src/processor.rs` - Main implementation
- `crates/storage/src/postgres.rs` - Database schema
- `migrations/` - Schema definitions
