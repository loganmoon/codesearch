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
    ndcg_5: float
    ndcg_10: float
    ndcg_20: float


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
        results: list[dict]
    ) -> QueryEvaluation:
        """Score all results for a query concurrently and compute NDCG.

        Args:
            query_id: Unique identifier for the query
            query_text: The search query text
            results: List of search results with entity details

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

        # Calculate NDCG at different cutoffs
        relevance_scores = [r.score for r in scored_results]
        ndcg_5 = calculate_ndcg(relevance_scores, k=5)
        ndcg_10 = calculate_ndcg(relevance_scores, k=10)
        ndcg_20 = calculate_ndcg(relevance_scores, k=20)

        return QueryEvaluation(
            query_id=query_id,
            query_text=query_text,
            scored_results=list(scored_results),
            ndcg_5=ndcg_5,
            ndcg_10=ndcg_10,
            ndcg_20=ndcg_20,
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


def calculate_aggregate_metrics(query_results: list[QueryEvaluation]) -> dict:
    """Calculate aggregate metrics from query results."""
    if not query_results:
        return {}

    ndcg_5_values = [q.ndcg_5 for q in query_results]
    ndcg_10_values = [q.ndcg_10 for q in query_results]
    ndcg_20_values = [q.ndcg_20 for q in query_results]

    return {
        "num_queries": len(query_results),
        "mean_ndcg_5": float(np.mean(ndcg_5_values)),
        "mean_ndcg_10": float(np.mean(ndcg_10_values)),
        "mean_ndcg_20": float(np.mean(ndcg_20_values)),
        "std_ndcg_5": float(np.std(ndcg_5_values)),
        "std_ndcg_10": float(np.std(ndcg_10_values)),
        "std_ndcg_20": float(np.std(ndcg_20_values)),
        "min_ndcg_10": float(np.min(ndcg_10_values)),
        "max_ndcg_10": float(np.max(ndcg_10_values)),
    }


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
                    "ndcg_5": q.ndcg_5,
                    "ndcg_10": q.ndcg_10,
                    "ndcg_20": q.ndcg_20,
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


def print_comparison_table(semantic: EndpointEvaluation, agentic: EndpointEvaluation) -> None:
    """Print a side-by-side comparison table of metrics."""
    sem = semantic.aggregate_metrics
    agt = agentic.aggregate_metrics

    if not sem or not agt:
        print("Insufficient data for comparison")
        return

    print("\n" + "=" * 70)
    print("SIDE-BY-SIDE COMPARISON")
    print("=" * 70)
    print(f"{'Metric':<25} {'Semantic':>20} {'Agentic':>20}")
    print("-" * 70)
    print(f"{'Queries evaluated':<25} {sem['num_queries']:>20} {agt['num_queries']:>20}")
    print("-" * 70)
    print(f"{'Mean NDCG@5':<25} {sem['mean_ndcg_5']:>20.4f} {agt['mean_ndcg_5']:>20.4f}")
    print(f"{'Mean NDCG@10':<25} {sem['mean_ndcg_10']:>20.4f} {agt['mean_ndcg_10']:>20.4f}")
    print(f"{'Mean NDCG@20':<25} {sem['mean_ndcg_20']:>20.4f} {agt['mean_ndcg_20']:>20.4f}")
    print("-" * 70)
    print(f"{'Std NDCG@10':<25} {sem['std_ndcg_10']:>20.4f} {agt['std_ndcg_10']:>20.4f}")
    print(f"{'Min NDCG@10':<25} {sem['min_ndcg_10']:>20.4f} {agt['min_ndcg_10']:>20.4f}")
    print(f"{'Max NDCG@10':<25} {sem['max_ndcg_10']:>20.4f} {agt['max_ndcg_10']:>20.4f}")
    print("=" * 70)

    # Highlight winner
    sem_ndcg10 = sem['mean_ndcg_10']
    agt_ndcg10 = agt['mean_ndcg_10']
    diff = agt_ndcg10 - sem_ndcg10
    if abs(diff) < 0.01:
        print("Result: Roughly equivalent performance")
    elif diff > 0:
        print(f"Result: Agentic search is better by {diff:.4f} NDCG@10 ({diff/sem_ndcg10*100:.1f}% improvement)")
    else:
        print(f"Result: Semantic search is better by {-diff:.4f} NDCG@10 ({-diff/agt_ndcg10*100:.1f}% improvement)")


def print_per_query_comparison(semantic: EndpointEvaluation, agentic: EndpointEvaluation) -> None:
    """Print per-query NDCG comparison."""
    print("\n" + "=" * 90)
    print("PER-QUERY NDCG@10 COMPARISON")
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

        sem_ndcg = sem_q.ndcg_10 if sem_q else 0.0
        agt_ndcg = agt_q.ndcg_10 if agt_q else 0.0
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


async def run_evaluation(
    queries_path: str,
    api_url: str,
    output_path: str,
    limit: int,
    repo_name: str,
    max_queries: Optional[int] = None,
    max_concurrent: int = 10,
) -> ComparisonEvaluation:
    """Run the full evaluation pipeline for both endpoints.

    Args:
        queries_path: Path to evaluation queries JSON
        api_url: Base URL for the codesearch API
        output_path: Path to write results
        limit: Number of results to retrieve per query
        repo_name: Repository name to search
        max_queries: Optional limit on number of queries to evaluate
        max_concurrent: Maximum concurrent LLM API calls

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
    print(f"Initialized LLM judge with {judge._model_name} (max {max_concurrent} concurrent)")

    async with aiohttp.ClientSession() as session:
        # Get repository ID
        repo_id = await get_repository_id(session, api_url, repo_name)
        if not repo_id:
            raise RuntimeError(f"Repository '{repo_name}' not found")
        print(f"Found repository: {repo_name} ({repo_id})")

        repository_ids = [repo_id]

        semantic_results = []
        agentic_results = []

        for i, query in enumerate(queries):
            query_id = query.get("id", f"query_{i}")
            query_text = query.get("query", "")

            print(f"\n[{i+1}/{len(queries)}] Query: {query_id}")
            print(f"  {query_text[:70]}...")

            # Fetch results from both endpoints
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
                evaluation = await judge.evaluate_query(query_id, query_text, results)

                if endpoint == "semantic":
                    semantic_results.append(evaluation)
                else:
                    agentic_results.append(evaluation)

                print(f" -> NDCG@10={evaluation.ndcg_10:.3f}")

    # Calculate aggregate metrics
    semantic_eval = EndpointEvaluation(
        endpoint="semantic",
        query_results=semantic_results,
        aggregate_metrics=calculate_aggregate_metrics(semantic_results),
    )

    agentic_eval = EndpointEvaluation(
        endpoint="agentic",
        query_results=agentic_results,
        aggregate_metrics=calculate_aggregate_metrics(agentic_results),
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
        help="Maximum concurrent LLM API calls (default: 10)",
    )

    args = parser.parse_args()

    print("=" * 70)
    print("LLM-as-a-Judge Code Search Evaluation")
    print("Comparing: Semantic Search vs Agentic Search")
    print("=" * 70)
    print(f"Queries:    {args.queries}")
    print(f"API URL:    {args.api_url}")
    print(f"Repository: {args.repo}")
    print(f"Limit:      {args.limit} results per query")
    print("=" * 70)

    comparison = asyncio.run(run_evaluation(
        queries_path=args.queries,
        api_url=args.api_url,
        output_path=args.output,
        limit=args.limit,
        repo_name=args.repo,
        max_queries=args.max_queries,
        max_concurrent=args.max_concurrent,
    ))

    # Print comparison tables
    print_comparison_table(comparison.semantic, comparison.agentic)
    print_per_query_comparison(comparison.semantic, comparison.agentic)


if __name__ == "__main__":
    main()
