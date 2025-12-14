#!/usr/bin/env python3
"""
LLM-as-a-Judge Evaluation for Code Search

Uses DeepEval with Gemini 2.5 Pro to score search results and compute NDCG@K.
Runs both semantic and agentic search endpoints and compares results side-by-side.
"""

import argparse
import asyncio
import json
import os
import sys
from dataclasses import dataclass
from datetime import datetime
from typing import Optional

import aiohttp
import numpy as np
from sklearn.metrics import ndcg_score

# DeepEval imports
from deepeval.models import GeminiModel
from deepeval.metrics import AnswerRelevancyMetric
from deepeval.test_case import LLMTestCase


class RateLimitError(Exception):
    """Raised when API rate limit is exceeded."""
    pass


@dataclass
class ScoredResult:
    """A search result with LLM-assigned relevance score."""
    entity_id: str
    qualified_name: str
    entity_type: str
    file_path: str
    score: int  # 0, 1, or 2
    reasoning: str
    original_rank: int
    original_score: float


@dataclass
class QueryEvaluation:
    """Evaluation results for a single query."""
    query_id: str
    query_text: str
    scored_results: list[ScoredResult]
    ndcg_scores: dict[int, float]  # k -> NDCG@k, only for valid k values


@dataclass
class EndpointEvaluation:
    """Evaluation results for one endpoint."""
    endpoint: str
    query_results: list[QueryEvaluation]
    aggregate_metrics: dict


@dataclass
class ComparisonEvaluation:
    """Side-by-side comparison of semantic vs agentic search."""
    metadata: dict
    semantic: EndpointEvaluation
    agentic: EndpointEvaluation


class CodeSearchJudge:
    """LLM-as-a-judge evaluator using DeepEval with Gemini."""

    def __init__(self, model: str = "gemini-2.5-pro", max_concurrent: int = 10):
        api_key = os.environ.get("GOOGLE_API_KEY")
        if not api_key:
            raise ValueError("GOOGLE_API_KEY environment variable must be set")

        self.model = GeminiModel(
            model=model,
            api_key=api_key,
            temperature=0  # Deterministic for consistent scoring
        )
        self._model_name = model
        self._max_concurrent = max_concurrent

    def _create_metric(self) -> AnswerRelevancyMetric:
        """Create a new AnswerRelevancyMetric instance for concurrent use."""
        return AnswerRelevancyMetric(
            model=self.model,
            threshold=0.5,
            include_reason=True,
        )

    async def _score_single_result(
        self,
        query: str,
        result: dict,
        index: int,
        semaphore: asyncio.Semaphore,
    ) -> ScoredResult:
        """Score a single result asynchronously with concurrency control."""
        async with semaphore:
            code_content = result.get("content") or "[No content available]"
            entity_info = (
                f"{result.get('qualified_name', 'Unknown')} "
                f"({result.get('entity_type', 'Unknown')}) "
                f"in {result.get('file_path', 'Unknown')}"
            )
            full_context = f"Entity: {entity_info}\n\nCode:\n```\n{code_content}\n```"

            test_case = LLMTestCase(
                input=query,
                actual_output=full_context
            )

            try:
                # Create a fresh metric for this scoring task
                metric = self._create_metric()
                await metric.a_measure(test_case)

                # Convert 0-1 score to 0-2 scale for NDCG
                raw_score = metric.score if metric.score is not None else 0.0
                score = round(raw_score * 2)
                score = max(0, min(2, score))
                reason = metric.reason if metric.reason else "No reasoning provided"
            except Exception as e:
                error_str = str(e).lower()
                # Check for rate limit errors - must exit immediately
                if "429" in str(e) or "resource_exhausted" in error_str or "rate" in error_str and "limit" in error_str:
                    raise RateLimitError(f"API rate limit exceeded: {e}") from e
                print(f"    Warning: Failed to score result {index+1}: {e}", file=sys.stderr)
                score, reason = 0, f"Scoring failed: {e}"

            return ScoredResult(
                entity_id=result.get("entity_id", ""),
                qualified_name=result.get("qualified_name", ""),
                entity_type=result.get("entity_type", ""),
                file_path=result.get("file_path", ""),
                score=score,
                reasoning=reason,
                original_rank=index + 1,
                original_score=result.get("score", 0.0),
            )

    async def evaluate_query(
        self,
        query_id: str,
        query_text: str,
        results: list[dict],
        limit: int,
    ) -> QueryEvaluation:
        """Score all results for a query concurrently and compute NDCG.

        Args:
            query_id: Unique identifier for the query
            query_text: The search query text
            results: List of search results with entity details
            limit: The result limit - NDCG is only calculated for valid k <= limit

        Returns:
            QueryEvaluation with scores and NDCG metrics
        """
        # Use semaphore to limit concurrent API calls
        semaphore = asyncio.Semaphore(self._max_concurrent)

        # Score all results concurrently
        tasks = [
            self._score_single_result(query_text, result, i, semaphore)
            for i, result in enumerate(results)
        ]
        scored_results = await asyncio.gather(*tasks)

        # Calculate NDCG only at valid cutoffs (k <= limit)
        relevance_scores = [r.score for r in scored_results]
        ndcg_scores = {}
        for k in [5, 10, 20]:
            if k <= limit:
                ndcg_scores[k] = calculate_ndcg(relevance_scores, k=k)

        return QueryEvaluation(
            query_id=query_id,
            query_text=query_text,
            scored_results=list(scored_results),
            ndcg_scores=ndcg_scores,
        )


def calculate_ndcg(relevance_scores: list[int], k: int) -> float:
    """Calculate NDCG@K from graded relevance scores.

    Uses the standard formula with gains: 2^rel - 1

    Args:
        relevance_scores: List of scores (0, 1, 2) in retrieval order
        k: Cutoff position

    Returns:
        NDCG@K score between 0 and 1
    """
    if not relevance_scores or k <= 0:
        return 0.0

    # Pad or truncate to k
    scores = relevance_scores[:k]
    if len(scores) < k:
        scores = scores + [0] * (k - len(scores))

    # Create ideal ordering (sorted descending)
    ideal_scores = sorted(scores, reverse=True)

    # If all scores are 0, NDCG is undefined; return 0
    if max(scores) == 0:
        return 0.0

    # Use sklearn's ndcg_score which expects 2D arrays
    y_true = np.array([ideal_scores])
    y_score = np.array([scores])

    return float(ndcg_score(y_true, y_score, k=k))


def calculate_aggregate_metrics(query_results: list[QueryEvaluation], limit: int) -> dict:
    """Calculate aggregate metrics from query results.

    Args:
        query_results: List of query evaluations
        limit: The result limit - determines which NDCG@k values are available
    """
    if not query_results:
        return {}

    # Determine which k values we have based on limit
    k_values = [k for k in [5, 10, 20] if k <= limit]
    if not k_values:
        return {"num_queries": len(query_results)}

    metrics = {"num_queries": len(query_results), "k_values": k_values}

    for k in k_values:
        values = [q.ndcg_scores.get(k, 0.0) for q in query_results]
        metrics[f"mean_ndcg_{k}"] = float(np.mean(values))
        metrics[f"std_ndcg_{k}"] = float(np.std(values))
        metrics[f"min_ndcg_{k}"] = float(np.min(values))
        metrics[f"max_ndcg_{k}"] = float(np.max(values))

    return metrics


async def fetch_search_results(
    session: aiohttp.ClientSession,
    api_url: str,
    query: str,
    repository_ids: list[str],
    endpoint: str,
    limit: int,
) -> list[dict]:
    """Fetch search results from the codesearch API.

    Args:
        session: aiohttp client session
        api_url: Base URL for the API
        query: Search query text
        repository_ids: List of repository UUIDs to search
        endpoint: Either 'semantic' or 'agentic'
        limit: Maximum number of results

    Returns:
        List of entity results from the API
    """
    if endpoint == "semantic":
        url = f"{api_url}/api/v1/search/semantic"
        payload = {
            "repository_ids": repository_ids,
            "query": {
                "text": query,
                "instruction": "Represent this query for retrieving the specific code entity that handles this functionality",
            },
            "limit": limit,
        }
    elif endpoint == "agentic":
        url = f"{api_url}/api/v1/search/agentic"
        payload = {
            "query": query,
            "repository_ids": repository_ids,
        }
    else:
        raise ValueError(f"Unknown endpoint: {endpoint}")

    async with session.post(url, json=payload) as response:
        if response.status != 200:
            text = await response.text()
            raise RuntimeError(f"API error {response.status}: {text}")

        data = await response.json()
        return data.get("results", [])


async def get_repository_id(
    session: aiohttp.ClientSession,
    api_url: str,
    repo_name: str,
) -> Optional[str]:
    """Get repository UUID by name from the API.

    Args:
        session: aiohttp client session
        api_url: Base URL for the API
        repo_name: Repository name to look up

    Returns:
        Repository UUID string or None if not found
    """
    url = f"{api_url}/api/v1/repositories"

    async with session.get(url) as response:
        if response.status != 200:
            return None

        data = await response.json()
        for repo in data.get("repositories", []):
            if repo_name in repo.get("repository_name", ""):
                return repo.get("repository_id")

        return None


def load_queries(path: str) -> list[dict]:
    """Load evaluation queries from JSON file.

    Args:
        path: Path to the evaluation JSON file

    Returns:
        List of query dictionaries
    """
    with open(path, "r") as f:
        data = json.load(f)
    return data.get("queries", [])


def save_results(evaluation: ComparisonEvaluation, output_path: str) -> None:
    """Save evaluation results to JSON file.

    Args:
        evaluation: The comparison evaluation results
        output_path: Path to write the output JSON
    """
    def serialize_endpoint(ep: EndpointEvaluation) -> dict:
        return {
            "endpoint": ep.endpoint,
            "aggregate_metrics": ep.aggregate_metrics,
            "per_query_results": [
                {
                    "query_id": q.query_id,
                    "query": q.query_text,
                    "ndcg_scores": q.ndcg_scores,
                    "scored_results": [
                        {
                            "entity_id": r.entity_id,
                            "qualified_name": r.qualified_name,
                            "entity_type": r.entity_type,
                            "file_path": r.file_path,
                            "score": r.score,
                            "reasoning": r.reasoning,
                            "original_rank": r.original_rank,
                            "original_score": r.original_score,
                        }
                        for r in q.scored_results
                    ],
                }
                for q in ep.query_results
            ],
        }

    output = {
        "metadata": evaluation.metadata,
        "semantic": serialize_endpoint(evaluation.semantic),
        "agentic": serialize_endpoint(evaluation.agentic),
    }

    with open(output_path, "w") as f:
        json.dump(output, f, indent=2)


def print_comparison_table(semantic: EndpointEvaluation, agentic: EndpointEvaluation, limit: int) -> None:
    """Print a side-by-side comparison table of metrics."""
    sem = semantic.aggregate_metrics
    agt = agentic.aggregate_metrics

    if not sem or not agt:
        print("Insufficient data for comparison")
        return

    k_values = sem.get('k_values', [k for k in [5, 10, 20] if k <= limit])
    primary_k = max(k_values) if k_values else 10

    print("\n" + "=" * 70)
    print("SIDE-BY-SIDE COMPARISON")
    print("=" * 70)
    print(f"{'Metric':<25} {'Semantic':>20} {'Agentic':>20}")
    print("-" * 70)
    print(f"{'Queries evaluated':<25} {sem['num_queries']:>20} {agt['num_queries']:>20}")
    print("-" * 70)

    for k in k_values:
        sem_val = sem.get(f'mean_ndcg_{k}', 0.0)
        agt_val = agt.get(f'mean_ndcg_{k}', 0.0)
        print(f"{f'Mean NDCG@{k}':<25} {sem_val:>20.4f} {agt_val:>20.4f}")

    print("-" * 70)
    sem_std = sem.get(f'std_ndcg_{primary_k}', 0.0)
    agt_std = agt.get(f'std_ndcg_{primary_k}', 0.0)
    sem_min = sem.get(f'min_ndcg_{primary_k}', 0.0)
    agt_min = agt.get(f'min_ndcg_{primary_k}', 0.0)
    sem_max = sem.get(f'max_ndcg_{primary_k}', 0.0)
    agt_max = agt.get(f'max_ndcg_{primary_k}', 0.0)

    print(f"{f'Std NDCG@{primary_k}':<25} {sem_std:>20.4f} {agt_std:>20.4f}")
    print(f"{f'Min NDCG@{primary_k}':<25} {sem_min:>20.4f} {agt_min:>20.4f}")
    print(f"{f'Max NDCG@{primary_k}':<25} {sem_max:>20.4f} {agt_max:>20.4f}")
    print("=" * 70)

    # Highlight winner using the primary k value
    sem_ndcg = sem.get(f'mean_ndcg_{primary_k}', 0.0)
    agt_ndcg = agt.get(f'mean_ndcg_{primary_k}', 0.0)
    diff = agt_ndcg - sem_ndcg
    if abs(diff) < 0.01:
        print("Result: Roughly equivalent performance")
    elif diff > 0:
        pct = (diff / sem_ndcg * 100) if sem_ndcg > 0 else 0
        print(f"Result: Agentic search is better by {diff:.4f} NDCG@{primary_k} ({pct:.1f}% improvement)")
    else:
        pct = (-diff / agt_ndcg * 100) if agt_ndcg > 0 else 0
        print(f"Result: Semantic search is better by {-diff:.4f} NDCG@{primary_k} ({pct:.1f}% improvement)")


def print_per_query_comparison(semantic: EndpointEvaluation, agentic: EndpointEvaluation, limit: int) -> None:
    """Print per-query NDCG comparison."""
    # Use the highest valid k value
    primary_k = max(k for k in [5, 10, 20] if k <= limit)

    print("\n" + "=" * 90)
    print(f"PER-QUERY NDCG@{primary_k} COMPARISON")
    print("=" * 90)
    print(f"{'Query ID':<12} {'Semantic':>12} {'Agentic':>12} {'Delta':>12} {'Winner':<15}")
    print("-" * 90)

    sem_by_id = {q.query_id: q for q in semantic.query_results}
    agt_by_id = {q.query_id: q for q in agentic.query_results}

    all_ids = sorted(set(sem_by_id.keys()) | set(agt_by_id.keys()))

    sem_wins = 0
    agt_wins = 0
    ties = 0

    for qid in all_ids:
        sem_q = sem_by_id.get(qid)
        agt_q = agt_by_id.get(qid)

        sem_ndcg = sem_q.ndcg_scores.get(primary_k, 0.0) if sem_q else 0.0
        agt_ndcg = agt_q.ndcg_scores.get(primary_k, 0.0) if agt_q else 0.0
        delta = agt_ndcg - sem_ndcg

        if abs(delta) < 0.01:
            winner = "Tie"
            ties += 1
        elif delta > 0:
            winner = "Agentic"
            agt_wins += 1
        else:
            winner = "Semantic"
            sem_wins += 1

        print(f"{qid:<12} {sem_ndcg:>12.4f} {agt_ndcg:>12.4f} {delta:>+12.4f} {winner:<15}")

    print("-" * 90)
    print(f"Summary: Semantic wins {sem_wins}, Agentic wins {agt_wins}, Ties {ties}")
    print("=" * 90)


async def evaluate_single_query(
    query_idx: int,
    query: dict,
    total_queries: int,
    session: aiohttp.ClientSession,
    api_url: str,
    repository_ids: list[str],
    limit: int,
    judge: CodeSearchJudge,
    query_semaphore: asyncio.Semaphore,
) -> tuple[Optional[QueryEvaluation], Optional[QueryEvaluation]]:
    """Evaluate a single query against both endpoints.

    Returns:
        Tuple of (semantic_result, agentic_result), either may be None on error
    """
    async with query_semaphore:
        query_id = query.get("id", f"query_{query_idx}")
        query_text = query.get("query", "")

        print(f"\n[{query_idx+1}/{total_queries}] Query: {query_id}")
        print(f"  {query_text[:70]}...")

        semantic_eval = None
        agentic_eval = None

        # Determine primary k for logging
        primary_k = max(k for k in [5, 10, 20] if k <= limit)

        for endpoint in ["semantic", "agentic"]:
            print(f"  {endpoint.capitalize()}:", end=" ", flush=True)

            try:
                results = await fetch_search_results(
                    session, api_url, query_text, repository_ids, endpoint, limit
                )
                print(f"{len(results)} results", end="", flush=True)
            except Exception as e:
                print(f"ERROR: {e}", file=sys.stderr)
                continue

            if not results:
                print(" (empty)")
                continue

            # Score results with LLM (async concurrent scoring)
            evaluation = await judge.evaluate_query(query_id, query_text, results, limit)

            if endpoint == "semantic":
                semantic_eval = evaluation
            else:
                agentic_eval = evaluation

            ndcg_val = evaluation.ndcg_scores.get(primary_k, 0.0)
            print(f" -> NDCG@{primary_k}={ndcg_val:.3f}")

        return (semantic_eval, agentic_eval)


async def run_evaluation(
    queries_path: str,
    api_url: str,
    output_path: str,
    limit: int,
    repo_name: str,
    max_queries: Optional[int] = None,
    max_concurrent: int = 10,
    max_query_concurrency: int = 4,
) -> ComparisonEvaluation:
    """Run the full evaluation pipeline for both endpoints.

    Args:
        queries_path: Path to evaluation queries JSON
        api_url: Base URL for the codesearch API
        output_path: Path to write results
        limit: Number of results to retrieve per query
        repo_name: Repository name to search
        max_queries: Optional limit on number of queries to evaluate
        max_concurrent: Maximum concurrent LLM API calls per query
        max_query_concurrency: Maximum queries to process in parallel (default 4)

    Returns:
        ComparisonEvaluation with results from both endpoints
    """
    # Load queries
    queries = load_queries(queries_path)
    if max_queries:
        queries = queries[:max_queries]

    print(f"Loaded {len(queries)} queries from {queries_path}")

    # Initialize the judge with concurrency control
    judge = CodeSearchJudge(max_concurrent=max_concurrent)
    print(f"Initialized LLM judge with {judge._model_name}")
    print(f"Concurrency: {max_query_concurrency} queries, {max_concurrent} LLM calls per query")

    async with aiohttp.ClientSession() as session:
        # Get repository ID
        repo_id = await get_repository_id(session, api_url, repo_name)
        if not repo_id:
            raise RuntimeError(f"Repository '{repo_name}' not found")
        print(f"Found repository: {repo_name} ({repo_id})")

        repository_ids = [repo_id]

        # Semaphore to limit concurrent query processing
        query_semaphore = asyncio.Semaphore(max_query_concurrency)

        # Process all queries in parallel (bounded by semaphore)
        tasks = [
            evaluate_single_query(
                i, query, len(queries), session, api_url, repository_ids,
                limit, judge, query_semaphore
            )
            for i, query in enumerate(queries)
        ]

        results = await asyncio.gather(*tasks)

        # Collect results
        semantic_results = [r[0] for r in results if r[0] is not None]
        agentic_results = [r[1] for r in results if r[1] is not None]

    # Calculate aggregate metrics
    semantic_eval = EndpointEvaluation(
        endpoint="semantic",
        query_results=semantic_results,
        aggregate_metrics=calculate_aggregate_metrics(semantic_results, limit),
    )

    agentic_eval = EndpointEvaluation(
        endpoint="agentic",
        query_results=agentic_results,
        aggregate_metrics=calculate_aggregate_metrics(agentic_results, limit),
    )

    # Build comparison evaluation
    comparison = ComparisonEvaluation(
        metadata={
            "judge_model": judge._model_name,
            "timestamp": datetime.now().isoformat(),
            "limit": limit,
            "repository": repo_name,
        },
        semantic=semantic_eval,
        agentic=agentic_eval,
    )

    # Save results
    save_results(comparison, output_path)
    print(f"\nResults saved to {output_path}")

    return comparison


def main():
    parser = argparse.ArgumentParser(
        description="LLM-as-a-Judge evaluation comparing semantic vs agentic code search"
    )
    parser.add_argument(
        "--queries",
        type=str,
        default="../data/nushell_semantic_eval.json",
        help="Path to evaluation queries JSON file",
    )
    parser.add_argument(
        "--api-url",
        type=str,
        default="http://localhost:3000",
        help="Base URL for codesearch API",
    )
    parser.add_argument(
        "--output",
        type=str,
        default="../data/llm_judge_results.json",
        help="Path to write evaluation results",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=20,
        help="Number of results to retrieve per query",
    )
    parser.add_argument(
        "--repo",
        type=str,
        default="nushell",
        help="Repository name to search",
    )
    parser.add_argument(
        "--max-queries",
        type=int,
        default=None,
        help="Maximum number of queries to evaluate (for testing)",
    )
    parser.add_argument(
        "--max-concurrent",
        type=int,
        default=10,
        help="Maximum concurrent LLM API calls per query (default: 10)",
    )
    parser.add_argument(
        "--max-query-concurrency",
        type=int,
        default=4,
        help="Maximum queries to process in parallel (default: 4, max recommended due to API rate limits)",
    )

    args = parser.parse_args()

    # Determine which NDCG@k values will be computed
    k_values = [k for k in [5, 10, 20] if k <= args.limit]

    print("=" * 70)
    print("LLM-as-a-Judge Code Search Evaluation")
    print("Comparing: Semantic Search vs Agentic Search")
    print("=" * 70)
    print(f"Queries:    {args.queries}")
    print(f"API URL:    {args.api_url}")
    print(f"Repository: {args.repo}")
    print(f"Limit:      {args.limit} results per query")
    print(f"NDCG@k:     {', '.join(f'@{k}' for k in k_values)}")
    print(f"Parallelism: {args.max_query_concurrency} queries, {args.max_concurrent} LLM calls/query")
    print("=" * 70)

    try:
        comparison = asyncio.run(run_evaluation(
            queries_path=args.queries,
            api_url=args.api_url,
            output_path=args.output,
            limit=args.limit,
            repo_name=args.repo,
            max_queries=args.max_queries,
            max_concurrent=args.max_concurrent,
            max_query_concurrency=args.max_query_concurrency,
        ))
    except RateLimitError as e:
        print(f"\n{'='*70}", file=sys.stderr)
        print("FATAL: Rate limit exceeded - stopping evaluation", file=sys.stderr)
        print(f"{'='*70}", file=sys.stderr)
        print(f"Error: {e}", file=sys.stderr)
        print("\nSuggestions:", file=sys.stderr)
        print("  - Wait a few minutes before retrying", file=sys.stderr)
        print("  - Reduce --max-query-concurrency (current: {})".format(args.max_query_concurrency), file=sys.stderr)
        print("  - Reduce --max-concurrent (current: {})".format(args.max_concurrent), file=sys.stderr)
        sys.exit(1)

    # Print comparison tables
    print_comparison_table(comparison.semantic, comparison.agentic, args.limit)
    print_per_query_comparison(comparison.semantic, comparison.agentic, args.limit)


if __name__ == "__main__":
    main()
