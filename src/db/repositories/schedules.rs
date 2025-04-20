use crate::{
    db::models::recording_schedule_models::{RecordingSchedule, RecordingScheduleDb},
    error::Error,
};
use anyhow::Result;
use chrono::{Datelike, Utc};
use sqlx::PgPool;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

/// Recording schedules repository for handling schedule operations
#[derive(Clone)]
pub struct SchedulesRepository {
    pool: Arc<PgPool>,
}

impl SchedulesRepository {
    /// Create a new schedules repository
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Create a new recording schedule
    pub async fn create(&self, schedule: &RecordingSchedule) -> Result<RecordingSchedule> {
        info!("Creating new recording schedule: {}", schedule.name);

        // Convert to database model
        let schedule_db = RecordingScheduleDb::from(schedule.clone());

        let result = sqlx::query_as::<_, RecordingScheduleDb>(
            r#"
            INSERT INTO recording_schedules (
                id, camera_id, name, enabled, days_of_week, start_time, end_time,
                created_at, updated_at, retention_days, recording_quality
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING id, camera_id, name, enabled, days_of_week, start_time, end_time,
                     created_at, updated_at, retention_days, recording_quality
            "#,
        )
        .bind(schedule_db.id)
        .bind(schedule_db.camera_id)
        .bind(&schedule_db.name)
        .bind(schedule_db.enabled)
        .bind(&schedule_db.days_of_week)
        .bind(&schedule_db.start_time)
        .bind(&schedule_db.end_time)
        .bind(schedule_db.created_at)
        .bind(schedule_db.updated_at)
        .bind(schedule_db.retention_days)
        .bind(&schedule_db.recording_quality)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create recording schedule: {}", e)))?;

        // Convert back to domain model
        Ok(RecordingSchedule::from(result))
    }

    /// Get recording schedule by ID
    pub async fn get_by_id(&self, id: &Uuid) -> Result<Option<RecordingSchedule>> {
        let result = sqlx::query_as::<_, RecordingScheduleDb>(
            r#"
            SELECT id, camera_id, name, enabled, days_of_week, start_time, end_time,
                   created_at, updated_at,  retention_days, recording_quality
            FROM recording_schedules
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get recording schedule by ID: {}", e)))?;

        // Convert to domain model if found
        Ok(result.map(RecordingSchedule::from))
    }

    /// Get recording schedules for a camera
    pub async fn get_by_camera(&self, camera_id: &Uuid) -> Result<Vec<RecordingSchedule>> {
        let result = sqlx::query_as::<_, RecordingScheduleDb>(
            r#"
            SELECT id, camera_id, name, enabled, days_of_week, start_time, end_time,
                   created_at, updated_at, retention_days, recording_quality
            FROM recording_schedules
            WHERE camera_id = $1
            ORDER BY name
            "#,
        )
        .bind(camera_id)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| {
            Error::Database(format!(
                "Failed to get recording schedules for camera: {}",
                e
            ))
        })?;

        // Convert all to domain models
        Ok(result.into_iter().map(RecordingSchedule::from).collect())
    }

    /// Get active recording schedules for current time
    pub async fn get_active_schedules(&self) -> Result<Vec<RecordingSchedule>> {
        // Get current time in UTC
        let now = Utc::now();

        // Extract day of week (0-6, where 0 is Sunday)
        let day_of_week = now.weekday().num_days_from_sunday() as i32;

        // Extract current time as HH:MM
        let current_time = now.format("%H:%M").to_string();

        let result = sqlx::query_as::<_, RecordingScheduleDb>(
            r#"
            SELECT id, camera_id, name, enabled, days_of_week, start_time, end_time,
                   created_at, updated_at, retention_days, recording_quality
            FROM recording_schedules
            WHERE enabled = true
            AND $1 = ANY(days_of_week)
            AND start_time <= $2
            AND end_time >= $2
            "#,
        )
        .bind(day_of_week)
        .bind(current_time)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get active recording schedules: {}", e)))?;

        // Convert all to domain models
        Ok(result.into_iter().map(RecordingSchedule::from).collect())
    }

    /// Update recording schedule
    pub async fn update(&self, schedule: &RecordingSchedule) -> Result<RecordingSchedule> {
        // Convert to database model
        let schedule_db = RecordingScheduleDb::from(schedule.clone());

        let result = sqlx::query_as::<_, RecordingScheduleDb>(
            r#"
            UPDATE recording_schedules
            SET camera_id = $1, name = $2, enabled = $3, days_of_week = $4,
                start_time = $5, end_time = $6, updated_at = $7,
                retention_days = $8, recording_quality = $9
            WHERE id = $10
            RETURNING id, camera_id, name, enabled, days_of_week, start_time, end_time,
                     created_at, updated_at, retention_days, recording_quality
            "#,
        )
        .bind(schedule_db.camera_id)
        .bind(&schedule_db.name)
        .bind(schedule_db.enabled)
        .bind(&schedule_db.days_of_week)
        .bind(&schedule_db.start_time)
        .bind(&schedule_db.end_time)
        .bind(Utc::now())
        .bind(schedule_db.retention_days)
        .bind(&schedule_db.recording_quality)
        .bind(schedule_db.id)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update recording schedule: {}", e)))?;

        // Convert back to domain model
        Ok(RecordingSchedule::from(result))
    }

    /// Delete recording schedule
    pub async fn delete(&self, id: &Uuid) -> Result<bool> {
        let result = sqlx::query(
            r#"
            DELETE FROM recording_schedules
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to delete recording schedule: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all recording schedules
    pub async fn get_all(&self) -> Result<Vec<RecordingSchedule>> {
        let result = sqlx::query_as::<_, RecordingScheduleDb>(
            r#"
            SELECT id, camera_id, name, enabled, days_of_week, start_time, end_time,
                   created_at, updated_at, retention_days, recording_quality
            FROM recording_schedules
            ORDER BY name
            "#,
        )
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get all recording schedules: {}", e)))?;

        // Convert all to domain models
        Ok(result.into_iter().map(RecordingSchedule::from).collect())
    }

    /// Enable or disable a recording schedule
    pub async fn set_enabled(&self, id: &Uuid, enabled: bool) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE recording_schedules
            SET enabled = $1, updated_at = $2
            WHERE id = $3
            "#,
        )
        .bind(enabled)
        .bind(Utc::now())
        .bind(id)
        .execute(&*self.pool)
        .await
        .map_err(|e| {
            Error::Database(format!("Failed to update recording schedule status: {}", e))
        })?;

        Ok(())
    }

    /// Get all enabled schedules
    pub async fn get_all_enabled(&self) -> Result<Vec<RecordingSchedule>> {
        let result = sqlx::query_as::<_, RecordingScheduleDb>(
            r#"
            SELECT id, camera_id, name, enabled, days_of_week, start_time, end_time,
                   created_at, updated_at,  retention_days, recording_quality
            FROM recording_schedules
            WHERE enabled = true
            ORDER BY name
            "#,
        )
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get enabled schedules: {}", e)))?;

        // Convert all to domain models
        Ok(result.into_iter().map(RecordingSchedule::from).collect())
    }
}
