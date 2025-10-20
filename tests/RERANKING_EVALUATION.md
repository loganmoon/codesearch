# Reranking Evaluation Test

This document describes how to use the reranking evaluation test suite to measure the performance improvement of the reranking feature.

## Overview

The evaluation test generates synthetic queries from indexed code entities and compares search results with and without reranking enabled. It measures:

- **Ranking improvements**: How often reranking moves the expected result higher in the ranking
- **Average rank improvement**: The average change in position for expected results
- **Latency overhead**: The performance cost of reranking
- **Recall metrics**: Whether expected results appear in top-k results

## Prerequisites

Before running the evaluation:

1. **Index the codesearch repository**:
   ```bash
   cd /home/logan/code/codesearch  # Or wherever your codesearch repo is
   codesearch index
   ```

   **Note**: The test automatically detects the workspace root, so it will look up the repository at the codesearch workspace root regardless of where you run the test from.

2. **Ensure infrastructure is running**:
   - Postgres database
   - Qdrant vector store
   - vLLM server with reranker model (if testing reranking)

3. **Configure reranking** in `~/.codesearch/config.toml`:
   ```toml
   [reranking]
   enabled = true
   model = "BAAI/bge-reranker-v2-m3"
   api_base_url = "http://localhost:8001"
   timeout_secs = 30
   candidates = 50
   top_k = 10
   ```

## Running the Evaluation

From the repository root:

```bash
cargo test --package codesearch-e2e-tests --test test_reranking_evaluation -- --ignored --nocapture
```

Flags:
- `--ignored`: Required because this is a long-running integration test
- `--nocapture`: Shows real-time progress output

## Test Process

The test follows these steps:

1. **Query Generation** (automated):
   - Fetches all indexed entities from the repository
   - Generates 100 test queries of different types:
     - **Exact name queries**: Entity name directly
     - **Semantic queries**: Natural language description
     - **Signature queries**: Function signature patterns
     - **Documentation queries**: Excerpts from doc comments

2. **Baseline Search**:
   - Runs all queries with reranking disabled
   - Uses only vector similarity scores

3. **Reranking Search**:
   - Runs all queries with reranking enabled
   - Retrieves 50 candidates, reranks to top-10

4. **Comparison & Metrics**:
   - Compares rankings for each query
   - Identifies improvements and degradations
   - Calculates aggregate statistics

5. **Report Generation**:
   - Prints summary to console
   - Saves detailed results to `/path/to/codesearch/target/reranking_evaluation_report.json` (workspace root)

## Interpreting Results

### Console Output

```
=== Evaluation Report ===

Total Queries: 100
Queries Improved: 42 (42.0%)
Queries Degraded: 12 (12.0%)
Queries Unchanged: 38 (38.0%)
Queries Not Found (Baseline): 5
Queries Not Found (Reranking): 3

Average Rank Improvement: 1.24
Average Baseline Latency: 45.2ms
Average Reranking Latency: 78.5ms
Latency Overhead: 73.7%

=== Top 10 Improvements ===
  5 -> 1 (improvement: +4) - SearchExecutor::new
  8 -> 2 (improvement: +6) - QueryGenerator::generate_queries
  ...
```

### Understanding Metrics

- **Queries Improved**: Expected result moved to a higher position (lower rank number)
- **Queries Degraded**: Expected result moved to a lower position (higher rank number)
- **Queries Unchanged**: Expected result stayed at the same position
- **Average Rank Improvement**: Positive means reranking improved rankings on average
- **Latency Overhead**: Percentage increase in query latency with reranking

### JSON Report

The detailed JSON report is saved to the workspace root at `target/reranking_evaluation_report.json` and contains:

```json
{
  "total_queries": 100,
  "queries_improved": 42,
  "average_rank_improvement": 1.24,
  "query_comparisons": [
    {
      "query": "search_similar",
      "query_type": "ExactName",
      "expected_entity": "search_similar",
      "baseline_rank": 5,
      "reranking_rank": 1,
      "rank_improvement": 4,
      "baseline_score": 0.84,
      "reranking_score": 0.96,
      "baseline_latency_ms": 42,
      "reranking_latency_ms": 73
    },
    ...
  ]
}
```

## Customizing the Test

You can modify the test parameters by editing `tests/tests/test_reranking_evaluation.rs`:

```rust
// Change number of test queries (default: 100)
let target_query_count = 200;

// Change result limit (default: 10)
let limit = 20;

// Change candidate count for reranking
let candidates_limit = if use_reranking && self.reranker.is_some() {
    100  // default is 50
} else {
    limit
};
```

## Troubleshooting

### "Repository not indexed" Error

Ensure you've run `codesearch index` in the repository directory first.

### Reranker Connection Errors

If reranking tests fail:
1. Check that vLLM is running: `docker ps | grep vllm`
2. Verify the reranker API is accessible: `curl http://localhost:8001/health`
3. Check vLLM logs: `docker logs codesearch-vllm`

### Out of Memory Errors

If generating embeddings fails:
- Reduce `target_query_count` in the test
- Reduce `candidates_limit` for reranking
- Check vLLM memory usage

## Future Enhancements

Potential improvements to the evaluation suite:

1. **Ground truth validation**: Manual review of generated queries to ensure quality
2. **Cross-repository testing**: Test across multiple repositories
3. **Query diversity metrics**: Measure variety of generated queries
4. **Relevance scoring**: Add human judgment or automated relevance assessment
5. **Continuous monitoring**: Track reranking performance over time
