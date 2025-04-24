use crate::{
    db::models::recording_models::{
        Recording, RecordingDb, RecordingEventType, RecordingSearchQuery, RecordingStats,
        RecordingStatsDb,
    },
    error::Error,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use log::{error, info};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// Recordings repository for handling recording operations
#[derive(Clone)]
pub struct RecordingsRepository {
    pool: Arc<PgPool>,
}

impl RecordingsRepository {
    /// Create a new recordings repository
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Create a new recording
    pub async fn create(&self, recording: &Recording) -> Result<Recording> {
        let recording_db = RecordingDb::from(recording.clone());

        let result = sqlx::query_as::<_, RecordingDb>(
            r#"
            INSERT INTO recordings (
                id, camera_id, stream_id, schedule_id, start_time, end_time, 
                file_path, file_size, duration, format, resolution, fps, 
                event_type, created_at, metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            RETURNING id, camera_id, stream_id, schedule_id, start_time, end_time, 
                     file_path, file_size, duration, format, resolution, fps,
                     event_type, metadata
            "#,
        )
        .bind(recording_db.id)
        .bind(recording_db.camera_id)
        .bind(recording_db.stream_id)
        .bind(recording_db.schedule_id)
        .bind(recording_db.start_time)
        .bind(recording_db.end_time)
        .bind(&recording_db.file_path)
        .bind(recording_db.file_size)
        .bind(recording_db.duration)
        .bind(&recording_db.format)
        .bind(&recording_db.resolution)
        .bind(recording_db.fps)
        .bind(recording_db.event_type)
        .bind(Utc::now())
        .bind(&recording_db.metadata)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create recording: {}", e)))?;

        Ok(Recording::from(result))
    }

    /// Get recording by ID
    pub async fn get_by_id(&self, id: &Uuid) -> Result<Option<Recording>> {
        let result = sqlx::query_as::<_, RecordingDb>(
            r#"
            SELECT id, camera_id, stream_id, schedule_id, start_time, end_time, file_path, file_size,
                   duration, format, resolution, fps, event_type, metadata
            FROM recordings
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get recording by ID: {}", e)))?;

        Ok(result.map(Recording::from))
    }

    /// Update recording
    pub async fn update(&self, recording: &Recording) -> Result<Recording> {
        let recording_db = RecordingDb::from(recording.clone());
        let result = sqlx::query_as::<_, RecordingDb>(
            r#"
            UPDATE recordings
            SET end_time = $1, file_size = $2, duration = $3, metadata = $4
            WHERE id = $5
            RETURNING id, camera_id, stream_id, schedule_id, start_time, end_time, file_path, file_size,
                     duration, format, resolution, fps, event_type, metadata
            "#,
        )
        .bind(recording_db.end_time)
        .bind(recording_db.file_size)
        .bind(recording_db.duration)
        .bind(&recording_db.metadata)
        .bind(recording_db.id)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update recording: {}", e)))?;

        Ok(Recording::from(result))
    }

    /// Delete recording
    pub async fn delete(&self, id: &Uuid) -> Result<bool> {
        let result = sqlx::query(
            r#"
            DELETE FROM recordings
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to delete recording: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }

    /// Delete recording with file
    pub async fn _delete_with_file(&self, id: &Uuid) -> Result<bool> {
        // Get the file path first
        let recording = self.get_by_id(id).await?;
        if let Some(recording) = recording {
            // Delete the file
            if let Err(e) = std::fs::remove_file(&recording.file_path) {
                // Log error but continue with DB deletion
                error!(
                    "Failed to delete recording file {}: {}",
                    recording.file_path.display(),
                    e
                );
            }

            // Delete from database
            self.delete(id).await
        } else {
            Ok(false)
        }
    }

    /// Search recordings with advanced filters
    pub async fn search(&self, query: &RecordingSearchQuery) -> Result<Vec<Recording>> {
        // Build dynamic query
        let mut sql = String::from(
            r#"
            SELECT id, camera_id, stream_id, schedule_id, start_time, end_time, file_path, file_size,
                   duration, format, resolution, fps, event_type, metadata
            FROM recordings
            WHERE 1=1
            "#,
        );

        let mut args: Vec<QueryArg> = Vec::new();
        let mut param_index = 1;

        // Add camera IDs filter
        if let Some(camera_ids) = &query.camera_ids {
            if !camera_ids.is_empty() {
                sql.push_str(&format!(" AND camera_id = ${}", param_index));
                args.push(QueryArg::Uuid(camera_ids[0]));
                param_index += 1;
            }
        }

        // Add stream IDs filter
        if let Some(stream_ids) = &query.stream_ids {
            if !stream_ids.is_empty() {
                sql.push_str(&format!(" AND stream_id = ${}", param_index));
                args.push(QueryArg::Uuid(stream_ids[0]));
                param_index += 1;
            }
        }

        // Add schedule ID filter
        if let Some(schedule_id) = &query.schedule_id {
            sql.push_str(&format!(" AND schedule_id = ${}", param_index));
            args.push(QueryArg::Uuid(*schedule_id));
            param_index += 1;
        }

        // Add time range filters
        if let Some(start_time) = &query.start_time {
            sql.push_str(&format!(" AND start_time >= ${}", param_index));
            args.push(QueryArg::DateTime(*start_time));
            param_index += 1;
        }

        if let Some(end_time) = &query.end_time {
            sql.push_str(&format!(" AND start_time <= ${}", param_index));
            args.push(QueryArg::DateTime(*end_time));
            param_index += 1;
        }

        // Add event type filter
        if let Some(event_types) = &query.event_types {
            if !event_types.is_empty() {
                // Convert event types to strings
                let event_type_strings: Vec<String> = event_types
                    .iter()
                    .map(|et| match et {
                        RecordingEventType::Continuous => "continuous".to_string(),
                        RecordingEventType::Motion => "motion".to_string(),
                        RecordingEventType::Audio => "audio".to_string(),
                        RecordingEventType::External => "external".to_string(),
                        RecordingEventType::Manual => "manual".to_string(),
                        RecordingEventType::Analytics => "analytics".to_string(),
                    })
                    .collect();

                sql.push_str(&format!(" AND event_type = ANY(${:?})", param_index));
                args.push(QueryArg::StringArray(event_type_strings));
                param_index += 1;
            }
        }

        // Add min duration filter
        if let Some(min_duration) = &query.min_duration {
            sql.push_str(&format!(" AND duration >= ${}", param_index));
            args.push(QueryArg::I64(*min_duration as i64));
            param_index += 1;
        }

        // Add order by
        sql.push_str(" ORDER BY start_time DESC");

        // Add limit and offset
        if let Some(limit) = &query.limit {
            sql.push_str(&format!(" LIMIT ${}", param_index));
            args.push(QueryArg::I64(*limit as i64));
            param_index += 1;
        } else {
            sql.push_str(" LIMIT 100"); // Default limit
        }

        if let Some(offset) = &query.offset {
            sql.push_str(&format!(" OFFSET ${}", param_index));
            args.push(QueryArg::I64(*offset as i64));
        }

        // Execute the query
        let mut query_builder = sqlx::query_as::<_, RecordingDb>(&sql);

        // Add the parameters
        for arg in args {
            query_builder = arg.apply_to_query(query_builder);
        }

        let result = query_builder
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to search recordings: {}", e)))?;

        Ok(result.into_iter().map(Recording::from).collect())
    }

    /// Get recordings for a camera
    pub async fn _get_by_camera(
        &self,
        camera_id: &Uuid,
        limit: Option<i64>,
    ) -> Result<Vec<Recording>> {
        let limit = limit.unwrap_or(100);

        let result = sqlx::query_as::<_, RecordingDb>(
            r#"
            SELECT id, camera_id, stream_id, schedule_id, start_time, end_time, file_path, file_size,
                   duration, format, resolution, fps, event_type, metadata
            FROM recordings
            WHERE camera_id = $1
            ORDER BY start_time DESC
            LIMIT $2
            "#,
        )
        .bind(camera_id)
        .bind(limit)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get recordings for camera: {}", e)))?;

        Ok(result.into_iter().map(Recording::from).collect())
    }

    /// Get recordings for a stream
    pub async fn _get_by_stream(
        &self,
        stream_id: &Uuid,
        limit: Option<i64>,
    ) -> Result<Vec<Recording>> {
        let limit = limit.unwrap_or(100);

        let result = sqlx::query_as::<_, RecordingDb>(
            r#"
            SELECT id, camera_id, stream_id, schedule_id, start_time, end_time, file_path, file_size,
                   duration, format, resolution, fps, event_type, metadata
            FROM recordings
            WHERE stream_id = $1
            ORDER BY start_time DESC
            LIMIT $2
            "#,
        )
        .bind(stream_id)
        .bind(limit)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get recordings for stream: {}", e)))?;

        Ok(result.into_iter().map(Recording::from).collect())
    }

    /// Get recordings older than a specified date for retention management
    pub async fn _get_expired_recordings(&self, retention_days: i32) -> Result<Vec<Recording>> {
        let cutoff_date = Utc::now() - chrono::Duration::days(retention_days as i64);

        let result = sqlx::query_as::<_, RecordingDb>(
            r#"
            SELECT id, camera_id, stream_id, schedule_id, start_time, end_time, file_path, file_size,
                   duration, format, resolution, fps, event_type, metadata
            FROM recordings
            WHERE start_time < $1
            "#,
        )
        .bind(cutoff_date)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get expired recordings: {}", e)))?;

        Ok(result.into_iter().map(Recording::from).collect())
    }

    /// Get recordings stats
    pub async fn get_stats(&self, camera_id: Option<Uuid>) -> Result<RecordingStats> {
        let stats = if let Some(camera_id) = camera_id {
            sqlx::query_as::<_, RecordingStatsDb>(
                r#"
                SELECT 
                    COUNT(*) as total_count,
                    COALESCE(SUM(file_size), 0) as total_size,
                    COALESCE(SUM(duration), 0) as total_duration,
                    MIN(start_time) as oldest,
                    MAX(start_time) as newest
                FROM recordings
                WHERE camera_id = $1
                "#,
            )
            .bind(camera_id)
            .fetch_one(&*self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to get recording stats: {}", e)))?
        } else {
            sqlx::query_as::<_, RecordingStatsDb>(
                r#"
                SELECT 
                    COUNT(*) as total_count,
                    COALESCE(SUM(file_size), 0) as total_size,
                    COALESCE(SUM(duration), 0) as total_duration,
                    MIN(start_time) as oldest,
                    MAX(start_time) as newest
                FROM recordings
                "#,
            )
            .fetch_one(&*self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to get recording stats: {}", e)))?
        };

        Ok(RecordingStats {
            total_count: stats.total_count.unwrap_or(0),
            total_size_bytes: stats.total_size.unwrap_or(0),
            total_duration_seconds: stats.total_duration.unwrap_or(0),
            oldest_recording: stats.oldest,
            newest_recording: stats.newest,
        })
    }

    /// Delete recordings older than a specified date
    pub async fn _delete_older_than(
        &self,
        cutoff_date: DateTime<Utc>,
        camera_id: Option<Uuid>,
    ) -> Result<u64> {
        let result = if let Some(camera_id) = camera_id {
            sqlx::query(
                r#"
                DELETE FROM recordings
                WHERE start_time < $1 AND camera_id = $2
                "#,
            )
            .bind(cutoff_date)
            .bind(camera_id)
            .execute(&*self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to delete old recordings: {}", e)))?
        } else {
            sqlx::query(
                r#"
                DELETE FROM recordings
                WHERE start_time < $1
                "#,
            )
            .bind(cutoff_date)
            .execute(&*self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to delete old recordings: {}", e)))?
        };

        info!("Deleted {} old recordings", result.rows_affected());
        Ok(result.rows_affected())
    }

    /// Delete recordings for a camera with their files
    pub async fn prune_recordings_by_camera(
        &self,
        camera_id: &Uuid,
        older_than_days: Option<i32>,
    ) -> Result<u64> {
        // Get recordings to delete
        let recordings = if let Some(days) = older_than_days {
            let cutoff_date = Utc::now() - chrono::Duration::days(days as i64);
            self.get_recordings_to_prune(Some(*camera_id), Some(cutoff_date))
                .await?
        } else {
            self.get_recordings_to_prune(Some(*camera_id), None).await?
        };

        let mut delete_count = 0;
        for recording in recordings {
            // Delete the file
            if let Err(e) = std::fs::remove_file(&recording.file_path) {
                error!(
                    "Failed to delete recording file {}: {}",
                    recording.file_path.display(),
                    e
                );
                continue;
            }

            // Delete from database
            if let Ok(deleted) = self.delete(&recording.id).await {
                if deleted {
                    delete_count += 1;
                }
            }
        }

        info!(
            "Pruned {} recordings for camera {}",
            delete_count, camera_id
        );
        Ok(delete_count)
    }

    /// Get recordings to prune
    pub async fn get_recordings_to_prune(
        &self,
        camera_id: Option<Uuid>,
        older_than: Option<DateTime<Utc>>,
    ) -> Result<Vec<Recording>> {
        let mut sql = String::from(
            r#"
            SELECT id, camera_id, stream_id, schedule_id, start_time, end_time, file_path, file_size,
                   duration, format, resolution, fps, event_type, metadata
            FROM recordings
            WHERE 1=1
            "#,
        );

        let mut args: Vec<QueryArg> = Vec::new();
        let mut param_index = 1;

        // Add camera ID filter
        if let Some(camera_id) = camera_id {
            sql.push_str(&format!(" AND camera_id = ${}", param_index));
            args.push(QueryArg::Uuid(camera_id));
            param_index += 1;
        }

        // Add time filter
        if let Some(cutoff_date) = older_than {
            sql.push_str(&format!(" AND start_time < ${}", param_index));
            args.push(QueryArg::DateTime(cutoff_date));
        }

        // Add order by
        sql.push_str(" ORDER BY start_time ASC");

        // Execute the query
        let mut query_builder = sqlx::query_as::<_, RecordingDb>(&sql);

        // Add the parameters
        for arg in args {
            query_builder = arg.apply_to_query(query_builder);
        }

        let result = query_builder
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to get recordings to prune: {}", e)))?;

        Ok(result.into_iter().map(Recording::from).collect())
    }
}

/// Helper enum for dynamic query parameters
enum QueryArg {
    Uuid(Uuid),
    DateTime(DateTime<Utc>),
    I64(i64),
    StringArray(Vec<String>),
}

impl QueryArg {
    // Apply this argument to a query builder
    fn apply_to_query<'a, T>(
        self,
        builder: sqlx::query::QueryAs<'a, sqlx::Postgres, T, sqlx::postgres::PgArguments>,
    ) -> sqlx::query::QueryAs<'a, sqlx::Postgres, T, sqlx::postgres::PgArguments> {
        match self {
            QueryArg::Uuid(uuid) => builder.bind(uuid),
            QueryArg::DateTime(dt) => builder.bind(dt),
            QueryArg::I64(i) => builder.bind(i),
            QueryArg::StringArray(arr) => builder.bind(arr),
        }
    }
}
