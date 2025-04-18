use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingSchedule {
    pub id: Uuid,
    pub camera_id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub days_of_week: Vec<i32>, // 0-6 for Sunday-Saturday (using i32 to match PostgreSQL INTEGER)
    pub start_time: String,     // "HH:MM" format
    pub end_time: String,       // "HH:MM" format
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Uuid,    // User ID
    pub retention_days: i32, // How long to keep recordings
    pub recording_quality: RecordingQuality,
}

/// Database-compatible recording schedule with proper array type
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RecordingScheduleDb {
    pub id: Uuid,
    pub camera_id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub days_of_week: Vec<i32>, // INTEGER[] in PostgreSQL
    pub start_time: String,
    pub end_time: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Uuid,
    pub retention_days: i32,
    pub recording_quality: RecordingQuality,
}

impl From<RecordingSchedule> for RecordingScheduleDb {
    fn from(schedule: RecordingSchedule) -> Self {
        Self {
            id: schedule.id,
            camera_id: schedule.camera_id,
            name: schedule.name,
            enabled: schedule.enabled,
            days_of_week: schedule.days_of_week,
            start_time: schedule.start_time,
            end_time: schedule.end_time,
            created_at: schedule.created_at,
            updated_at: schedule.updated_at,
            created_by: schedule.created_by,
            retention_days: schedule.retention_days,
            recording_quality: schedule.recording_quality,
        }
    }
}

impl From<RecordingScheduleDb> for RecordingSchedule {
    fn from(db: RecordingScheduleDb) -> Self {
        Self {
            id: db.id,
            camera_id: db.camera_id,
            name: db.name,
            enabled: db.enabled,
            days_of_week: db.days_of_week,
            start_time: db.start_time,
            end_time: db.end_time,
            created_at: db.created_at,
            updated_at: db.updated_at,
            created_by: db.created_by,
            retention_days: db.retention_days,
            recording_quality: db.recording_quality,
        }
    }
}

/// Recording quality settings
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "recording_quality", rename_all = "lowercase")]
pub enum RecordingQuality {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "original")]
    Original,
}
