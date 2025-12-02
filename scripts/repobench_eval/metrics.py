"""IR metric computation for retrieval evaluation."""

import math
from dataclasses import dataclass

from .search import SearchResult


@dataclass
class EvalMetrics:
    """Computed IR metrics for a single query."""

    mrr: float
    recall_at_1: float
    recall_at_5: float
    recall_at_10: float
    ndcg_at_10: float
    gold_rank: int | None  # Rank of gold result (1-indexed), None if not found


def find_gold_rank(results: list[SearchResult], gold_entity_pattern: str) -> int | None:
    """Find the rank of the gold entity in search results.

    Args:
        results: List of search results
        gold_entity_pattern: Pattern to match in entity name (e.g., "snippet_0_3")

    Returns:
        1-indexed rank of gold entity, or None if not found
    """
    for i, result in enumerate(results):
        # Check if the pattern appears in the entity name or qualified name
        if gold_entity_pattern in result.name or gold_entity_pattern in result.qualified_name:
            return i + 1
    return None


def compute_mrr(results: list[SearchResult], gold_entity_pattern: str) -> float:
    """Compute Mean Reciprocal Rank.

    MRR = 1/rank of first relevant result, or 0 if none found.

    Args:
        results: List of search results
        gold_entity_pattern: Pattern to match the gold entity

    Returns:
        MRR score (0.0 to 1.0)
    """
    rank = find_gold_rank(results, gold_entity_pattern)
    if rank is None:
        return 0.0
    return 1.0 / rank


def compute_recall_at_k(
    results: list[SearchResult],
    gold_entity_pattern: str,
    k: int,
) -> float:
    """Compute Recall@k (binary: is gold in top-k?).

    Args:
        results: List of search results
        gold_entity_pattern: Pattern to match the gold entity
        k: Cutoff rank

    Returns:
        1.0 if gold in top-k, 0.0 otherwise
    """
    rank = find_gold_rank(results[:k], gold_entity_pattern)
    return 1.0 if rank is not None else 0.0


def compute_ndcg_at_k(
    results: list[SearchResult],
    gold_entity_pattern: str,
    k: int,
) -> float:
    """Compute NDCG@k with binary relevance.

    For binary relevance (gold=1, others=0), this simplifies to:
    - DCG = 1/log2(rank+1) if gold found in top-k, else 0
    - IDCG = 1/log2(2) = 1.0 (best case: gold at rank 1)

    Args:
        results: List of search results
        gold_entity_pattern: Pattern to match the gold entity
        k: Cutoff rank

    Returns:
        NDCG@k score (0.0 to 1.0)
    """
    rank = find_gold_rank(results[:k], gold_entity_pattern)

    if rank is None:
        return 0.0

    # DCG with binary relevance
    dcg = 1.0 / math.log2(rank + 1)

    # IDCG for binary relevance with 1 relevant doc (gold at rank 1)
    idcg = 1.0 / math.log2(2)

    return dcg / idcg


def compute_all_metrics(
    results: list[SearchResult],
    gold_entity_pattern: str,
) -> EvalMetrics:
    """Compute all IR metrics for a single query.

    Args:
        results: List of search results
        gold_entity_pattern: Pattern to match the gold entity

    Returns:
        EvalMetrics with all computed metrics
    """
    gold_rank = find_gold_rank(results, gold_entity_pattern)

    return EvalMetrics(
        mrr=compute_mrr(results, gold_entity_pattern),
        recall_at_1=compute_recall_at_k(results, gold_entity_pattern, 1),
        recall_at_5=compute_recall_at_k(results, gold_entity_pattern, 5),
        recall_at_10=compute_recall_at_k(results, gold_entity_pattern, 10),
        ndcg_at_10=compute_ndcg_at_k(results, gold_entity_pattern, 10),
        gold_rank=gold_rank,
    )


@dataclass
class AggregateMetrics:
    """Aggregated metrics across multiple queries."""

    count: int
    mrr: float
    recall_at_1: float
    recall_at_5: float
    recall_at_10: float
    ndcg_at_10: float
    avg_query_time_ms: float
    gold_found_count: int
    gold_found_rate: float


def aggregate_metrics(
    metrics_list: list[EvalMetrics],
    query_times_ms: list[int],
) -> AggregateMetrics:
    """Aggregate metrics across multiple queries.

    Args:
        metrics_list: List of per-query metrics
        query_times_ms: List of query times in milliseconds

    Returns:
        AggregateMetrics with mean values
    """
    if not metrics_list:
        return AggregateMetrics(
            count=0,
            mrr=0.0,
            recall_at_1=0.0,
            recall_at_5=0.0,
            recall_at_10=0.0,
            ndcg_at_10=0.0,
            avg_query_time_ms=0.0,
            gold_found_count=0,
            gold_found_rate=0.0,
        )

    n = len(metrics_list)
    gold_found = sum(1 for m in metrics_list if m.gold_rank is not None)

    return AggregateMetrics(
        count=n,
        mrr=sum(m.mrr for m in metrics_list) / n,
        recall_at_1=sum(m.recall_at_1 for m in metrics_list) / n,
        recall_at_5=sum(m.recall_at_5 for m in metrics_list) / n,
        recall_at_10=sum(m.recall_at_10 for m in metrics_list) / n,
        ndcg_at_10=sum(m.ndcg_at_10 for m in metrics_list) / n,
        avg_query_time_ms=sum(query_times_ms) / n if query_times_ms else 0.0,
        gold_found_count=gold_found,
        gold_found_rate=gold_found / n,
    )
