use codesearch_core::entities::CodeEntity;
use codesearch_core::error::{Error, Result};
use sqlx::PgPool;
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
    pub entity_id: String,
    pub operation: String,
    pub target_store: String,
    pub payload: serde_json::Value,
    pub version_id: Uuid,
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
        let existing: Option<(Uuid,)> = sqlx::query_as(
            "SELECT repository_id FROM repositories WHERE collection_name = $1",
        )
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
        let record: Option<(Uuid,)> = sqlx::query_as(
            "SELECT repository_id FROM repositories WHERE collection_name = $1",
        )
        .bind(collection_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to query repository: {e}")))?;

        Ok(record.map(|(id,)| id))
    }

    /// Store entity metadata and version (within existing transaction if provided)
    pub async fn store_entity_metadata(
        &self,
        entity: &CodeEntity,
        qdrant_point_id: Uuid,
        git_commit_hash: Option<String>,
    ) -> Result<Uuid> {
        tracing::debug!(
            "Storing entity {} with point_id {} and git_commit {:?}",
            entity.entity_id,
            qdrant_point_id,
            git_commit_hash
        );

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to begin transaction: {e}")))?;

        // Get next version number
        let version_number: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version_number), 0) + 1 FROM entity_versions WHERE entity_id = $1",
        )
        .bind(&entity.entity_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to get version number: {e}")))?;

        // Insert version
        let version_id: Uuid = sqlx::query_scalar(
            "INSERT INTO entity_versions (
                entity_id, version_number, git_commit_hash, file_path, qualified_name,
                entity_type, language, entity_data, line_range
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::int4range)
            RETURNING version_id",
        )
        .bind(&entity.entity_id)
        .bind(version_number)
        .bind(&git_commit_hash)
        .bind(
            entity
                .file_path
                .to_str()
                .ok_or_else(|| Error::storage("Invalid file path"))?,
        )
        .bind(&entity.qualified_name)
        .bind(entity.entity_type.to_string())
        .bind(format!("{:?}", entity.language))
        .bind(
            serde_json::to_value(entity)
                .map_err(|e| Error::storage(format!("Failed to serialize entity: {e}")))?,
        )
        .bind(format!("[{},{})", entity.line_range.0, entity.line_range.1))
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to insert version: {e}")))?;

        // Upsert metadata
        sqlx::query(
            "INSERT INTO entity_metadata (
                entity_id, file_path, qualified_name, entity_type, language,
                qdrant_point_id, current_version_id, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())
            ON CONFLICT (entity_id) DO UPDATE SET
                qdrant_point_id = EXCLUDED.qdrant_point_id,
                current_version_id = EXCLUDED.current_version_id,
                updated_at = NOW()",
        )
        .bind(&entity.entity_id)
        .bind(
            entity
                .file_path
                .to_str()
                .ok_or_else(|| Error::storage("Invalid file path"))?,
        )
        .bind(&entity.qualified_name)
        .bind(entity.entity_type.to_string())
        .bind(format!("{:?}", entity.language))
        .bind(qdrant_point_id)
        .bind(version_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to upsert metadata: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        tracing::debug!(
            "Successfully stored entity {} with version_id {}",
            entity.entity_id,
            version_id
        );

        Ok(version_id)
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

    /// Get entity version history
    pub async fn get_entity_versions(&self, entity_id: &str) -> Result<Vec<EntityVersionRow>> {
        let versions = sqlx::query_as::<_, EntityVersionRow>(
            "SELECT version_id, entity_id, version_number, indexed_at, git_commit_hash,
                    file_path, qualified_name, entity_type, language, entity_data,
                    lower(line_range) as line_start, upper(line_range) as line_end
             FROM entity_versions
             WHERE entity_id = $1
             ORDER BY version_number DESC",
        )
        .bind(entity_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get entity versions: {e}")))?;

        Ok(versions)
    }

    /// Write outbox entry for entity operation
    pub async fn write_outbox_entry(
        &self,
        entity_id: &str,
        operation: OutboxOperation,
        target_store: TargetStore,
        payload: serde_json::Value,
        version_id: Uuid,
    ) -> Result<Uuid> {
        let outbox_id: Uuid = sqlx::query_scalar(
            "INSERT INTO entity_outbox (
                entity_id, operation, target_store, payload, version_id
            ) VALUES ($1, $2, $3, $4, $5)
            RETURNING outbox_id",
        )
        .bind(entity_id)
        .bind(operation.to_string())
        .bind(target_store.to_string())
        .bind(payload)
        .bind(version_id)
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
            "SELECT outbox_id, entity_id, operation, target_store, payload, version_id,
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

#[derive(sqlx::FromRow)]
pub struct EntityVersionRow {
    pub version_id: Uuid,
    pub entity_id: String,
    pub version_number: i32,
    pub indexed_at: chrono::DateTime<chrono::Utc>,
    pub git_commit_hash: Option<String>,
    pub file_path: String,
    pub qualified_name: String,
    pub entity_type: String,
    pub language: String,
    pub entity_data: serde_json::Value,
    pub line_start: i32,
    pub line_end: i32,
}
