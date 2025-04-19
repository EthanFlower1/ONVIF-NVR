use crate::{db::models::event_settings_models::EventSettings, error::Error};
use anyhow::Result;
use chrono::Utc;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

/// Camera event settings repository for handling event settings operations
#[derive(Clone)]
pub struct EventSettingsRepository {
    pool: Arc<PgPool>,
}

impl EventSettingsRepository {
    /// Create a new camera event settings repository
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Create new camera event settings
    pub async fn create(&self, settings: &EventSettings) -> Result<EventSettings> {
        info!(
            "Creating new camera event settings for camera: {}",
            settings.camera_id
        );

        // Convert to DB model
        let settings_db = EventSettings::from(settings.clone());

        let result = sqlx::query_as::<_, EventSettings>(
            r#"
            INSERT INTO camera_event_settings (
                id, camera_id, enabled, event_types, event_topic_expressions,
                trigger_recording, recording_duration, recording_quality, 
                created_at, updated_at, created_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING id, camera_id, enabled, event_types, event_topic_expressions,
                      trigger_recording, recording_duration, recording_quality,
                      created_at, updated_at, created_by
            "#,
        )
        .bind(settings_db.id)
        .bind(settings_db.camera_id)
        .bind(settings_db.enabled)
        .bind(&settings_db.event_types)
        .bind(&settings_db.event_topic_expressions)
        .bind(settings_db.trigger_recording)
        .bind(settings_db.recording_duration)
        .bind(settings_db.recording_quality)
        .bind(settings_db.created_at)
        .bind(settings_db.updated_at)
        .bind(settings_db.created_by)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create camera event settings: {}", e)))?;

        // Convert back to regular model
        Ok(EventSettings::from(result))
    }

    /// Get settings by ID
    pub async fn get_by_id(&self, id: &Uuid) -> Result<Option<EventSettings>> {
        let result = sqlx::query_as::<_, EventSettings>(
            r#"
            SELECT id, camera_id, enabled, event_types, event_topic_expressions,
                   trigger_recording, recording_duration, recording_quality, 
                   created_at, updated_at, created_by
            FROM camera_event_settings
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| {
            Error::Database(format!("Failed to get camera event settings by ID: {}", e))
        })?;

        Ok(result.map(EventSettings::from))
    }

    /// Get settings by camera ID
    pub async fn get_by_camera_id(&self, camera_id: &Uuid) -> Result<Option<EventSettings>> {
        let result = sqlx::query_as::<_, EventSettings>(
            r#"
            SELECT id, camera_id, enabled, event_types, event_topic_expressions,
                   trigger_recording, recording_duration, recording_quality, 
                   created_at, updated_at, created_by
            FROM camera_event_settings
            WHERE camera_id = $1
            "#,
        )
        .bind(camera_id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| {
            Error::Database(format!(
                "Failed to get camera event settings by camera ID: {}",
                e
            ))
        })?;

        Ok(result.map(EventSettings::from))
    }

    /// Update settings
    pub async fn update(&self, settings: &EventSettings) -> Result<EventSettings> {
        // Convert to DB model
        let settings_db = EventSettings::from(settings.clone());

        let result = sqlx::query_as::<_, EventSettings>(
            r#"
            UPDATE camera_event_settings
            SET enabled = $1, event_types = $2, event_topic_expressions = $3,
                trigger_recording = $4, recording_duration = $5, recording_quality = $6,
                updated_at = $7
            WHERE id = $8
            RETURNING id, camera_id, enabled, event_types, event_topic_expressions,
                      trigger_recording, recording_duration, recording_quality,
                      created_at, updated_at, created_by
            "#,
        )
        .bind(settings_db.enabled)
        .bind(&settings_db.event_types)
        .bind(&settings_db.event_topic_expressions)
        .bind(settings_db.trigger_recording)
        .bind(settings_db.recording_duration)
        .bind(settings_db.recording_quality)
        .bind(Utc::now())
        .bind(settings_db.id)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update camera event settings: {}", e)))?;

        Ok(EventSettings::from(result))
    }

    /// Delete settings
    pub async fn delete(&self, id: &Uuid) -> Result<bool> {
        let result = sqlx::query(
            r#"
            DELETE FROM camera_event_settings
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to delete camera event settings: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all enabled camera event settings
    pub async fn get_all_enabled(&self) -> Result<Vec<EventSettings>> {
        let result = sqlx::query_as::<_, EventSettings>(
            r#"
            SELECT id, camera_id, enabled, event_types, event_topic_expressions,
                   trigger_recording, recording_duration, recording_quality, 
                   created_at, updated_at, created_by
            FROM camera_event_settings
            WHERE enabled = true
            "#,
        )
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| {
            Error::Database(format!(
                "Failed to get all enabled camera event settings: {}",
                e
            ))
        })?;

        Ok(result.into_iter().map(EventSettings::from).collect())
    }
}

