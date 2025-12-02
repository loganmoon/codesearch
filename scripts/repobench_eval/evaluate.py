"""Main evaluation pipeline for RepoBench-R retrieval benchmark."""

import json
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from datasets import Dataset
from tqdm import tqdm

from .dataset import iterate_cases
from .metrics import AggregateMetrics, EvalMetrics, aggregate_metrics, compute_all_metrics
from .search import CodesearchClient


@dataclass
class CaseResult:
    """Evaluation result for a single test case."""

    case_idx: int
    query: str
    gold_snippet_idx: int
    gold_entity_pattern: str
    metrics: dict[str, EvalMetrics]
    query_times_ms: dict[str, int]


@dataclass
class EvaluationReport:
    """Full evaluation report with metadata and results."""

    metadata: dict[str, Any]
    aggregate_metrics: dict[str, AggregateMetrics]
    per_case_results: list[CaseResult]


def evaluate_single_case(
    client: CodesearchClient,
    repository_id: str,
    case_idx: int,
    query: str,
    gold_snippet_idx: int,
    search_limit: int = 10,
) -> CaseResult:
    """Evaluate a single test case across all search types.

    Args:
        client: CodesearchClient instance
        repository_id: UUID of the indexed repository
        case_idx: Index of the test case
        query: Query text (the "code" field from RepoBench-R)
        gold_snippet_idx: Index of the gold snippet in context list
        search_limit: Maximum results to retrieve per search

    Returns:
        CaseResult with metrics for all search types
    """
    gold_entity_pattern = f"snippet_{case_idx}_{gold_snippet_idx}"

    metrics: dict[str, EvalMetrics] = {}
    query_times_ms: dict[str, int] = {}

    # Semantic search
    semantic_response = client.search_semantic(repository_id, query, limit=search_limit)
    metrics["semantic"] = compute_all_metrics(semantic_response.results, gold_entity_pattern)
    query_times_ms["semantic"] = semantic_response.query_time_ms

    # Fulltext search
    fulltext_response = client.search_fulltext(repository_id, query, limit=search_limit)
    metrics["fulltext"] = compute_all_metrics(fulltext_response.results, gold_entity_pattern)
    query_times_ms["fulltext"] = fulltext_response.query_time_ms

    # Unified (hybrid) search
    unified_response = client.search_unified(repository_id, query, limit=search_limit)
    metrics["unified"] = compute_all_metrics(unified_response.results, gold_entity_pattern)
    query_times_ms["unified"] = unified_response.query_time_ms

    return CaseResult(
        case_idx=case_idx,
        query=query[:200] + "..." if len(query) > 200 else query,  # Truncate for report
        gold_snippet_idx=gold_snippet_idx,
        gold_entity_pattern=gold_entity_pattern,
        metrics=metrics,
        query_times_ms=query_times_ms,
    )


def run_evaluation_queries(
    dataset: Dataset,
    repository_id: str,
    subset: str,
    split: str,
    base_url: str = "http://localhost:8080",
) -> EvaluationReport:
    """Run evaluation queries against a running codesearch server.

    This is the second phase of evaluation, after setup has created and indexed
    the synthetic repository.

    Args:
        dataset: Loaded RepoBench-R dataset
        repository_id: UUID of the indexed repository
        subset: Dataset subset name (for metadata)
        split: Dataset split name (for metadata)
        base_url: Codesearch REST API base URL

    Returns:
        EvaluationReport with all results
    """
    client = CodesearchClient(base_url)

    if not client.health_check():
        raise RuntimeError(
            f"Codesearch server at {base_url} is not healthy.\n"
            f"Start it with: codesearch serve"
        )

    case_results: list[CaseResult] = []

    for case_idx, query, gold_idx, _context in tqdm(
        iterate_cases(dataset),
        total=len(dataset),
        desc="Evaluating cases",
    ):
        result = evaluate_single_case(
            client=client,
            repository_id=repository_id,
            case_idx=case_idx,
            query=query,
            gold_snippet_idx=gold_idx,
        )
        case_results.append(result)

    # Aggregate metrics per search type
    search_types = ["semantic", "fulltext", "unified"]
    aggregate_results: dict[str, AggregateMetrics] = {}

    for search_type in search_types:
        metrics_list = [r.metrics[search_type] for r in case_results]
        times_list = [r.query_times_ms[search_type] for r in case_results]
        aggregate_results[search_type] = aggregate_metrics(metrics_list, times_list)

    return EvaluationReport(
        metadata={
            "dataset": "tianyang/repobench-r",
            "subset": subset,
            "split": split,
            "num_cases": len(case_results),
            "repository_id": repository_id,
            "base_url": base_url,
            "timestamp": datetime.now(timezone.utc).isoformat(),
        },
        aggregate_metrics=aggregate_results,
        per_case_results=case_results,
    )


def save_report(report: EvaluationReport, output_path: Path) -> None:
    """Save evaluation report to JSON file.

    Args:
        report: EvaluationReport to save
        output_path: Path to output JSON file
    """
    output_path.parent.mkdir(parents=True, exist_ok=True)

    # Convert dataclasses to dicts for JSON serialization
    report_dict = {
        "metadata": report.metadata,
        "aggregate_metrics": {
            k: asdict(v) for k, v in report.aggregate_metrics.items()
        },
        "per_case_results": [
            {
                "case_idx": r.case_idx,
                "query": r.query,
                "gold_snippet_idx": r.gold_snippet_idx,
                "gold_entity_pattern": r.gold_entity_pattern,
                "metrics": {k: asdict(v) for k, v in r.metrics.items()},
                "query_times_ms": r.query_times_ms,
            }
            for r in report.per_case_results
        ],
    }

    with open(output_path, "w") as f:
        json.dump(report_dict, f, indent=2)

    print(f"\nReport saved to: {output_path}")


def print_summary(report: EvaluationReport) -> None:
    """Print a summary table of evaluation results.

    Args:
        report: EvaluationReport to summarize
    """
    print("\n" + "=" * 80)
    print("RepoBench-R Evaluation Results")
    print("=" * 80)
    print(f"Dataset: {report.metadata['dataset']} ({report.metadata['subset']}, {report.metadata['split']})")
    print(f"Cases: {report.metadata['num_cases']}")
    print()

    # Header
    print(f"{'Search Type':<12} | {'MRR':>6} | {'R@1':>6} | {'R@5':>6} | {'R@10':>6} | {'NDCG@10':>7} | {'Latency':>8} | {'Found':>6}")
    print("-" * 80)

    # Rows
    for search_type in ["semantic", "fulltext", "unified"]:
        m = report.aggregate_metrics[search_type]
        print(
            f"{search_type:<12} | {m.mrr:>6.3f} | {m.recall_at_1:>6.3f} | "
            f"{m.recall_at_5:>6.3f} | {m.recall_at_10:>6.3f} | {m.ndcg_at_10:>7.3f} | "
            f"{m.avg_query_time_ms:>6.0f}ms | {m.gold_found_rate:>5.1%}"
        )

    print("=" * 80)
