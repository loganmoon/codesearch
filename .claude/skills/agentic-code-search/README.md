# Agentic Code Search Skill

> **Status**: Planned - Full implementation pending

## Overview

This Claude Code skill enables agentic Retrieval-Augmented Generation (RAG) workflows by orchestrating iterative queries to the codesearch REST API.

## Concept

The skill performs intelligent, multi-step code search by:
1. **Query Planning** - Analyzing the user's goal and planning optimal search queries
2. **Iterative Execution** - Running semantic, full-text, unified, and graph queries
3. **Result Synthesis** - Aggregating and synthesizing findings into actionable insights

## Planned Architecture

```
index.ts (Main Entry Point)
├── types.ts (TypeScript interfaces)
├── search-client.ts (REST API client)
│   ├── searchSemantic()
│   ├── searchFulltext()
│   ├── searchUnified()
│   └── queryGraph()
├── query-planner.ts (AI-powered planning)
│   ├── planNextQuery()
│   └── isGoalSatisfied()
└── synthesis.ts (Result aggregation)
    └── synthesizeResults()
```

## Usage Example

```typescript
const result = await agenticCodeSearch({
  goal: "Find all authentication-related code and understand the flow",
  repository_id: "uuid",  // optional
  maxIterations: 5        // optional, default: 5
});
```

## API Endpoints Used

The skill will leverage all codesearch REST API endpoints:

- **POST** `/api/v1/search/semantic` - Vector-based semantic search
- **POST** `/api/v1/search/fulltext` - PostgreSQL full-text search
- **POST** `/api/v1/search/unified` - Hybrid search with RRF fusion
- **POST** `/api/v1/graph/query` - Neo4j graph traversal

## Implementation Plan

### Phase 1: Core Client
- [ ] Create `types.ts` with all interface definitions
- [ ] Implement `search-client.ts` with fetch-based REST calls
- [ ] Add error handling and retry logic

### Phase 2: Query Planning
- [ ] Implement `query-planner.ts` with Claude AI integration
- [ ] Add context tracking for iterative queries
- [ ] Implement goal satisfaction checking

### Phase 3: Result Synthesis
- [ ] Create `synthesis.ts` for result aggregation
- [ ] Implement deduplication and ranking
- [ ] Add structured output formatting

### Phase 4: Main Orchestrator
- [ ] Implement `index.ts` with main search loop
- [ ] Add SearchContext state management
- [ ] Implement follow-up query identification

### Phase 5: Testing & Documentation
- [ ] Unit tests for each module
- [ ] Integration tests with mock API
- [ ] Usage examples and documentation

## Example Queries

1. **Find Implementation**: "Find all database query code in the project"
2. **Understand Flow**: "How does the authentication system work end-to-end?"
3. **Find Patterns**: "Show me all error handling patterns in the codebase"
4. **Trace Dependencies**: "Find all callers of the validateToken function"

## Configuration

```typescript
interface AgenticSearchParams {
  goal: string;                    // What to find/understand
  repository_id?: string;          // Optional: specific repository
  maxIterations?: number;          // Default: 5
  verbose?: boolean;               // Enable detailed logging
}
```

## Next Steps

1. Complete TypeScript implementation following the architecture above
2. Integrate with Claude Code skill system
3. Test with real codesearch instances
4. Document best practices and examples

## Requirements

- codesearch server running on `http://localhost:3001`
- Node.js/TypeScript runtime (provided by Claude Code)
- Access to Claude API for query planning

## References

- Full implementation plan: `/docs/plans/rest-api-transition.md` (lines 1129-1485)
- REST API documentation: `http://localhost:3001/swagger-ui`
- Service layer: `crates/api-service/src/`
