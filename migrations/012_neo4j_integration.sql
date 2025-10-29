-- Add graph_ready flag to track relationship resolution status
ALTER TABLE repositories ADD COLUMN IF NOT EXISTS graph_ready BOOLEAN DEFAULT FALSE;

-- Add neo4j database name for per-repository isolation
ALTER TABLE repositories ADD COLUMN IF NOT EXISTS neo4j_database_name VARCHAR(255);

-- Create index for graph_ready queries
CREATE INDEX IF NOT EXISTS idx_repositories_graph_ready ON repositories(graph_ready);

-- Add comment explaining the columns
COMMENT ON COLUMN repositories.graph_ready IS 'True when all Neo4j relationships have been resolved';
COMMENT ON COLUMN repositories.neo4j_database_name IS 'Neo4j database name for this repository (e.g., codesearch_<uuid>)';
