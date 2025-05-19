use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RecordingSchedule {
    pub id: Uuid,
    pub camera_id: Uuid,
    pub stream_id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub days_of_week: Vec<i32>, // 0-6 for Sunday-Saturday (using i32 to match PostgreSQL INTEGER)
    pub start_time: String,     // "HH:MM" format
    pub end_time: String,       // "HH:MM" format
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub retention_days: i32, // How long to keep recordings
    pub record_on_motion: bool,   // Record on motion events
    pub record_on_audio: bool,    // Record on audio events
    pub record_on_analytics: bool, // Record on analytics events
    pub record_on_external: bool,  // Record on external events
    pub continuous_recording: bool, // Record continuously during scheduled times
}

/// Database-compatible recording schedule with proper array type
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RecordingScheduleDb {
    pub id: Uuid,
    pub camera_id: Uuid,
    pub stream_id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub days_of_week: Vec<i32>, // INTEGER[] in PostgreSQL
    pub start_time: String,
    pub end_time: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub retention_days: i32,
    pub record_on_motion: bool,
    pub record_on_audio: bool,
    pub record_on_analytics: bool,
    pub record_on_external: bool,
    pub continuous_recording: bool,
}

impl From<RecordingSchedule> for RecordingScheduleDb {
    fn from(schedule: RecordingSchedule) -> Self {
        Self {
            id: schedule.id,
            camera_id: schedule.camera_id,
            stream_id: schedule.stream_id,
            name: schedule.name,
            enabled: schedule.enabled,
            days_of_week: schedule.days_of_week,
            start_time: schedule.start_time,
            end_time: schedule.end_time,
            created_at: schedule.created_at,
            updated_at: schedule.updated_at,
            retention_days: schedule.retention_days,
            record_on_motion: schedule.record_on_motion,
            record_on_audio: schedule.record_on_audio,
            record_on_analytics: schedule.record_on_analytics,
            record_on_external: schedule.record_on_external,
            continuous_recording: schedule.continuous_recording,
        }
    }
}

impl From<RecordingScheduleDb> for RecordingSchedule {
    fn from(db: RecordingScheduleDb) -> Self {
        Self {
            id: db.id,
            camera_id: db.camera_id,
            stream_id: db.stream_id,
            name: db.name,
            enabled: db.enabled,
            days_of_week: db.days_of_week,
            start_time: db.start_time,
            end_time: db.end_time,
            created_at: db.created_at,
            updated_at: db.updated_at,
            retention_days: db.retention_days,
            record_on_motion: db.record_on_motion,
            record_on_audio: db.record_on_audio,
            record_on_analytics: db.record_on_analytics,
            record_on_external: db.record_on_external,
            continuous_recording: db.continuous_recording,
        }
    }
}
