use codesearch_core::entities::CodeEntity;
use codesearch_core::error::{Error, Result};
use sqlx::PgPool;
use uuid::Uuid;

pub struct PostgresClient {
    pool: PgPool,
}

impl PostgresClient {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Store entity metadata and version (within existing transaction if provided)
    pub async fn store_entity_metadata(
        &self,
        entity: &CodeEntity,
        qdrant_point_id: Uuid,
        git_commit_hash: Option<String>,
    ) -> Result<Uuid> {
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
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
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
        .bind(entity.language.to_string())
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
        .bind(entity.language.to_string())
        .bind(qdrant_point_id)
        .bind(version_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to upsert metadata: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

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
