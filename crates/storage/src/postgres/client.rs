use codesearch_core::entities::CodeEntity;
use codesearch_core::error::{Error, Result};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub enum OutboxOperation {
    Insert,
    Update,
    Delete,
}

impl std::fmt::Display for OutboxOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Insert => write!(f, "INSERT"),
            Self::Update => write!(f, "UPDATE"),
            Self::Delete => write!(f, "DELETE"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TargetStore {
    Qdrant,
    Neo4j,
}

impl std::fmt::Display for TargetStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Qdrant => write!(f, "qdrant"),
            Self::Neo4j => write!(f, "neo4j"),
        }
    }
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct OutboxEntry {
    pub outbox_id: Uuid,
    pub repository_id: Uuid,
    pub entity_id: String,
    pub operation: String,
    pub target_store: String,
    pub payload: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub processed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub retry_count: i32,
    pub last_error: Option<String>,
}

pub struct PostgresClient {
    pool: PgPool,
}

impl PostgresClient {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Run database migrations
    pub async fn run_migrations(&self) -> Result<()> {
        sqlx::migrate!("../../migrations")
            .run(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to run migrations: {e}")))?;
        Ok(())
    }

    /// Ensure repository exists, return repository_id
    pub async fn ensure_repository(
        &self,
        repository_path: &std::path::Path,
        collection_name: &str,
        repository_name: Option<&str>,
    ) -> Result<Uuid> {
        let repo_path_str = repository_path
            .to_str()
            .ok_or_else(|| Error::storage("Invalid repository path"))?;

        // Try to find existing repository
        let existing: Option<(Uuid,)> =
            sqlx::query_as("SELECT repository_id FROM repositories WHERE collection_name = $1")
                .bind(collection_name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to query repository: {e}")))?;

        if let Some((repository_id,)) = existing {
            return Ok(repository_id);
        }

        // Create new repository
        let repo_name = repository_name
            .or_else(|| repository_path.file_name()?.to_str())
            .unwrap_or("unknown");

        let (repository_id,): (Uuid,) = sqlx::query_as(
            "INSERT INTO repositories (repository_path, repository_name, collection_name, created_at, updated_at)
             VALUES ($1, $2, $3, NOW(), NOW())
             RETURNING repository_id",
        )
        .bind(repo_path_str)
        .bind(repo_name)
        .bind(collection_name)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to insert repository: {e}")))?;

        Ok(repository_id)
    }

    /// Get repository by collection name
    pub async fn get_repository_id(&self, collection_name: &str) -> Result<Option<Uuid>> {
        let record: Option<(Uuid,)> =
            sqlx::query_as("SELECT repository_id FROM repositories WHERE collection_name = $1")
                .bind(collection_name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to query repository: {e}")))?;

        Ok(record.map(|(id,)| id))
    }

    /// Store or update entity metadata (simplified - no version history)
    pub async fn store_entity_metadata(
        &self,
        repository_id: Uuid,
        entity: &CodeEntity,
        git_commit_hash: Option<String>,
        qdrant_point_id: Uuid,
    ) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        // Serialize entity to JSONB
        let entity_json = serde_json::to_value(entity)
            .map_err(|e| Error::storage(format!("Failed to serialize entity: {e}")))?;

        // Upsert entity_metadata
        sqlx::query(
            "INSERT INTO entity_metadata (
                entity_id, repository_id, qualified_name, name, parent_scope,
                entity_type, language, file_path, line_range, visibility,
                entity_data, git_commit_hash, qdrant_point_id,
                indexed_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::int4range, $10, $11, $12, $13, NOW(), NOW())
            ON CONFLICT (repository_id, entity_id)
            DO UPDATE SET
                qualified_name = EXCLUDED.qualified_name,
                name = EXCLUDED.name,
                parent_scope = EXCLUDED.parent_scope,
                entity_type = EXCLUDED.entity_type,
                language = EXCLUDED.language,
                file_path = EXCLUDED.file_path,
                line_range = EXCLUDED.line_range,
                visibility = EXCLUDED.visibility,
                entity_data = EXCLUDED.entity_data,
                git_commit_hash = EXCLUDED.git_commit_hash,
                qdrant_point_id = EXCLUDED.qdrant_point_id,
                updated_at = NOW(),
                deleted_at = NULL",
        )
        .bind(&entity.entity_id)
        .bind(repository_id)
        .bind(&entity.qualified_name)
        .bind(&entity.name)
        .bind(&entity.parent_scope)
        .bind(format!("{:?}", entity.entity_type))
        .bind(entity.language.to_string())
        .bind(
            entity
                .file_path
                .to_str()
                .ok_or_else(|| Error::storage("Invalid file path"))?,
        )
        .bind(format!("[{},{})", entity.line_range.0, entity.line_range.1))
        .bind(format!("{:?}", entity.visibility))
        .bind(entity_json)
        .bind(git_commit_hash)
        .bind(qdrant_point_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to upsert entity metadata: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        Ok(())
    }

    /// Get all entity IDs for a file path
    pub async fn get_entities_for_file(&self, file_path: &str) -> Result<Vec<String>> {
        let entity_ids: Vec<String> = sqlx::query_scalar(
            "SELECT entity_id FROM entity_metadata WHERE file_path = $1 AND deleted_at IS NULL",
        )
        .bind(file_path)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get entities for file: {e}")))?;

        Ok(entity_ids)
    }

    /// Get file snapshot (list of entity IDs in file)
    pub async fn get_file_snapshot(
        &self,
        repository_id: Uuid,
        file_path: &str,
    ) -> Result<Option<Vec<String>>> {
        let record: Option<(Vec<String>,)> = sqlx::query_as(
            "SELECT entity_ids FROM file_entity_snapshots
             WHERE repository_id = $1 AND file_path = $2",
        )
        .bind(repository_id)
        .bind(file_path)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get file snapshot: {e}")))?;

        Ok(record.map(|(ids,)| ids))
    }

    /// Update file snapshot with current entity IDs
    pub async fn update_file_snapshot(
        &self,
        repository_id: Uuid,
        file_path: &str,
        entity_ids: Vec<String>,
        git_commit_hash: Option<String>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO file_entity_snapshots (repository_id, file_path, entity_ids, git_commit_hash, indexed_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (repository_id, file_path)
             DO UPDATE SET
                entity_ids = EXCLUDED.entity_ids,
                git_commit_hash = EXCLUDED.git_commit_hash,
                indexed_at = NOW()",
        )
        .bind(repository_id)
        .bind(file_path)
        .bind(&entity_ids)
        .bind(git_commit_hash)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to update file snapshot: {e}")))?;

        Ok(())
    }

    /// Batch fetch entities by (repository_id, entity_id) pairs
    pub async fn get_entities_by_ids(
        &self,
        entity_refs: &[(Uuid, String)],
    ) -> Result<Vec<CodeEntity>> {
        if entity_refs.is_empty() {
            return Ok(Vec::new());
        }

        // Build VALUES clause for batch query
        let mut query = String::from(
            "SELECT entity_data FROM entity_metadata WHERE (repository_id, entity_id) IN (",
        );

        for (i, _) in entity_refs.iter().enumerate() {
            if i > 0 {
                query.push_str(", ");
            }
            query.push_str(&format!("(${}, ${})", i * 2 + 1, i * 2 + 2));
        }
        query.push_str(") AND deleted_at IS NULL");

        // Build query dynamically
        let mut sql_query = sqlx::query(&query);
        for (repo_id, entity_id) in entity_refs {
            sql_query = sql_query.bind(repo_id).bind(entity_id);
        }

        let rows = sql_query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to fetch entities: {e}")))?;

        let mut entities = Vec::new();
        for row in rows {
            let entity_json: serde_json::Value = row
                .try_get("entity_data")
                .map_err(|e| Error::storage(format!("Failed to extract entity_data: {e}")))?;
            let entity: CodeEntity = serde_json::from_value(entity_json)
                .map_err(|e| Error::storage(format!("Failed to deserialize entity: {e}")))?;
            entities.push(entity);
        }

        Ok(entities)
    }

    /// Mark entities as deleted (soft delete)
    pub async fn mark_entities_deleted(
        &self,
        repository_id: Uuid,
        entity_ids: &[String],
    ) -> Result<()> {
        if entity_ids.is_empty() {
            return Ok(());
        }

        // Build IN clause for batch update
        let mut query = String::from(
            "UPDATE entity_metadata SET deleted_at = NOW(), updated_at = NOW()
             WHERE repository_id = $1 AND entity_id IN (",
        );

        for (i, _) in entity_ids.iter().enumerate() {
            if i > 0 {
                query.push_str(", ");
            }
            query.push_str(&format!("${}", i + 2));
        }
        query.push(')');

        // Execute batch update
        let mut sql_query = sqlx::query(&query).bind(repository_id);
        for entity_id in entity_ids {
            sql_query = sql_query.bind(entity_id);
        }

        let result = sql_query
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to mark entities as deleted: {e}")))?;

        tracing::info!("Marked {} entities as deleted", result.rows_affected());

        Ok(())
    }

    /// Write outbox entry for entity operation
    pub async fn write_outbox_entry(
        &self,
        repository_id: Uuid,
        entity_id: &str,
        operation: OutboxOperation,
        target_store: TargetStore,
        payload: serde_json::Value,
    ) -> Result<Uuid> {
        let outbox_id: Uuid = sqlx::query_scalar(
            "INSERT INTO entity_outbox (
                repository_id, entity_id, operation, target_store, payload, created_at
            ) VALUES ($1, $2, $3, $4, $5, NOW())
            RETURNING outbox_id",
        )
        .bind(repository_id)
        .bind(entity_id)
        .bind(operation.to_string())
        .bind(target_store.to_string())
        .bind(payload)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to write outbox entry: {e}")))?;

        Ok(outbox_id)
    }

    /// Get unprocessed outbox entries for a target store
    pub async fn get_unprocessed_outbox_entries(
        &self,
        target_store: TargetStore,
        limit: i64,
    ) -> Result<Vec<OutboxEntry>> {
        let entries = sqlx::query_as::<_, OutboxEntry>(
            "SELECT outbox_id, repository_id, entity_id, operation, target_store, payload,
                    created_at, processed_at, retry_count, last_error
             FROM entity_outbox
             WHERE target_store = $1 AND processed_at IS NULL
             ORDER BY created_at ASC
             LIMIT $2",
        )
        .bind(target_store.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get outbox entries: {e}")))?;

        Ok(entries)
    }

    /// Mark outbox entry as processed
    pub async fn mark_outbox_processed(&self, outbox_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE entity_outbox SET processed_at = NOW() WHERE outbox_id = $1")
            .bind(outbox_id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to mark outbox processed: {e}")))?;

        Ok(())
    }

    /// Increment retry count and record error
    pub async fn record_outbox_failure(&self, outbox_id: Uuid, error: &str) -> Result<()> {
        sqlx::query(
            "UPDATE entity_outbox
             SET retry_count = retry_count + 1, last_error = $2
             WHERE outbox_id = $1",
        )
        .bind(outbox_id)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to record outbox failure: {e}")))?;

        Ok(())
    }
}
