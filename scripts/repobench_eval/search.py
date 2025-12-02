"""REST API search wrappers for codesearch endpoints."""

import time
from dataclasses import dataclass
from typing import Any

import requests


DEFAULT_BASE_URL = "http://localhost:8080"


@dataclass
class SearchResult:
    """Single search result from codesearch API."""

    entity_id: str
    qualified_name: str
    name: str
    entity_type: str
    score: float
    file_path: str
    content: str | None = None


@dataclass
class SearchResponse:
    """Response from a search endpoint."""

    results: list[SearchResult]
    query_time_ms: int
    total_results: int


class CodesearchClient:
    """Client for codesearch REST API."""

    def __init__(self, base_url: str = DEFAULT_BASE_URL):
        self.base_url = base_url.rstrip("/")
        self.session = requests.Session()

    def _parse_results(self, results: list[dict[str, Any]]) -> list[SearchResult]:
        """Parse API results into SearchResult objects."""
        return [
            SearchResult(
                entity_id=r["entity_id"],
                qualified_name=r["qualified_name"],
                name=r["name"],
                entity_type=r["entity_type"],
                score=r["score"],
                file_path=r["file_path"],
                content=r.get("content"),
            )
            for r in results
        ]

    def search_semantic(
        self,
        repository_id: str,
        query: str,
        limit: int = 10,
        instruction: str | None = None,
    ) -> SearchResponse:
        """Execute semantic (vector) search.

        Args:
            repository_id: UUID of the repository to search
            query: Search query text
            limit: Maximum number of results
            instruction: Optional BGE instruction override

        Returns:
            SearchResponse with results and timing
        """
        start = time.perf_counter()

        query_spec = {"text": query}
        if instruction:
            query_spec["instruction"] = instruction

        response = self.session.post(
            f"{self.base_url}/api/v1/search/semantic",
            json={
                "repository_ids": [repository_id],
                "query": query_spec,
                "limit": limit,
            },
        )
        response.raise_for_status()

        elapsed_ms = int((time.perf_counter() - start) * 1000)
        data = response.json()

        return SearchResponse(
            results=self._parse_results(data["results"]),
            query_time_ms=elapsed_ms,
            total_results=data["metadata"]["total_results"],
        )

    def search_fulltext(
        self,
        repository_id: str,
        query: str,
        limit: int = 10,
    ) -> SearchResponse:
        """Execute full-text (BM25) search.

        Args:
            repository_id: UUID of the repository to search
            query: Search query text
            limit: Maximum number of results

        Returns:
            SearchResponse with results and timing
        """
        start = time.perf_counter()

        response = self.session.post(
            f"{self.base_url}/api/v1/search/fulltext",
            json={
                "repository_id": repository_id,
                "query": query,
                "limit": limit,
            },
        )
        response.raise_for_status()

        elapsed_ms = int((time.perf_counter() - start) * 1000)
        data = response.json()

        return SearchResponse(
            results=self._parse_results(data["results"]),
            query_time_ms=elapsed_ms,
            total_results=data["metadata"]["total_results"],
        )

    def search_unified(
        self,
        repository_id: str,
        query: str,
        limit: int = 10,
        enable_fulltext: bool = True,
        enable_semantic: bool = True,
        rrf_k: int = 60,
        instruction: str | None = None,
    ) -> SearchResponse:
        """Execute unified (hybrid) search with RRF fusion.

        Args:
            repository_id: UUID of the repository to search
            query: Search query text
            limit: Maximum number of results
            enable_fulltext: Enable BM25 component
            enable_semantic: Enable vector component
            rrf_k: RRF fusion constant (default 60)
            instruction: Optional BGE instruction override

        Returns:
            SearchResponse with results and timing
        """
        start = time.perf_counter()

        query_spec = {"text": query}
        if instruction:
            query_spec["instruction"] = instruction

        response = self.session.post(
            f"{self.base_url}/api/v1/search/unified",
            json={
                "repository_id": repository_id,
                "query": query_spec,
                "limit": limit,
                "enable_fulltext": enable_fulltext,
                "enable_semantic": enable_semantic,
                "rrf_k": rrf_k,
            },
        )
        response.raise_for_status()

        elapsed_ms = int((time.perf_counter() - start) * 1000)
        data = response.json()

        return SearchResponse(
            results=self._parse_results(data["results"]),
            query_time_ms=elapsed_ms,
            total_results=data["metadata"]["total_results"],
        )

    def get_repositories(self) -> list[dict[str, Any]]:
        """List all indexed repositories.

        Returns:
            List of repository info dictionaries
        """
        response = self.session.get(f"{self.base_url}/api/v1/repositories")
        response.raise_for_status()
        return response.json()["repositories"]

    def health_check(self) -> bool:
        """Check if the codesearch server is healthy.

        Returns:
            True if server is healthy, False otherwise
        """
        try:
            response = self.session.get(f"{self.base_url}/health")
            return response.status_code == 200
        except requests.RequestException:
            return False
