use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Recording event type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RecordingEventType {
    /// Continuous recording (scheduled)
    Continuous,
    /// Motion-triggered recording
    Motion,
    /// Audio-triggered recording
    Audio,
    /// External event-triggered recording (e.g., alarm input)
    External,
    /// Manual recording (user initiated)
    Manual,
    /// Analytics-triggered recording (e.g., line crossing)
    Analytics,
}

impl std::fmt::Display for RecordingEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordingEventType::Continuous => write!(f, "continuous"),
            RecordingEventType::Motion => write!(f, "motion"),
            RecordingEventType::Audio => write!(f, "audio"), 
            RecordingEventType::External => write!(f, "external"),
            RecordingEventType::Manual => write!(f, "manual"),
            RecordingEventType::Analytics => write!(f, "analytics"),
        }
    }
}

impl sqlx::Type<sqlx::Postgres> for RecordingEventType {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("text")
    }
}

// Implement encoding for database storage
impl sqlx::Encode<'_, sqlx::Postgres> for RecordingEventType {
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync + 'static>> {
        // Use the Display implementation to convert to string
        let s = self.to_string();
        <&str as sqlx::Encode<sqlx::Postgres>>::encode_by_ref(&s.as_str(), buf)
    }
}

// Implement decoding from database
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for RecordingEventType {
    fn decode(
        value: sqlx::postgres::PgValueRef<'r>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let text = <String as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(match text.as_str() {
            "continuous" => RecordingEventType::Continuous,
            "motion" => RecordingEventType::Motion,
            "audio" => RecordingEventType::Audio,
            "external" => RecordingEventType::External,
            "manual" => RecordingEventType::Manual,
            "analytics" => RecordingEventType::Analytics,
            _ => RecordingEventType::Continuous, // Default to continuous
        })
    }
}

impl Default for RecordingEventType {
    fn default() -> Self {
        RecordingEventType::Continuous
    }
}

/// Recording model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    pub id: Uuid,
    pub camera_id: Uuid,
    pub stream_id: Uuid,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub file_path: PathBuf,
    pub file_size: u64,
    pub duration: u64,
    pub format: String,
    pub resolution: String,
    pub fps: u32,
    pub event_type: RecordingEventType,
    pub metadata: Option<serde_json::Value>,
    pub schedule_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RecordingDb {
    pub id: Uuid,
    pub camera_id: Uuid,
    pub stream_id: Uuid,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub file_path: String,
    pub file_size: i64,
    pub duration: i64,
    pub format: String,
    pub resolution: String,
    pub fps: i32,
    pub event_type: RecordingEventType,
    pub metadata: Option<serde_json::Value>,
    pub schedule_id: Option<Uuid>,
}

impl From<RecordingDb> for Recording {
    fn from(db: RecordingDb) -> Self {
        Self {
            id: db.id,
            camera_id: db.camera_id,
            stream_id: db.stream_id,
            start_time: db.start_time,
            end_time: db.end_time,
            file_path: PathBuf::from(db.file_path),
            file_size: db.file_size as u64,
            duration: db.duration as u64,
            format: db.format,
            resolution: db.resolution,
            fps: db.fps as u32,
            event_type: db.event_type,
            metadata: db.metadata,
            schedule_id: db.schedule_id,
        }
    }
}

impl From<Recording> for RecordingDb {
    fn from(r: Recording) -> Self {
        Self {
            id: r.id,
            camera_id: r.camera_id,
            stream_id: r.stream_id,
            start_time: r.start_time,
            end_time: r.end_time,
            file_path: r.file_path.to_string_lossy().to_string(),
            file_size: r.file_size as i64,
            duration: r.duration as i64,
            format: r.format,
            resolution: r.resolution,
            fps: r.fps as i32,
            event_type: r.event_type,
            metadata: r.metadata,
            schedule_id: r.schedule_id,
        }
    }
}

/// Search query model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingSearchQuery {
    pub camera_ids: Option<Vec<Uuid>>,
    pub stream_ids: Option<Vec<Uuid>>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub event_types: Option<Vec<RecordingEventType>>,
    pub schedule_id: Option<Uuid>,
    pub min_duration: Option<u64>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[cfg(test)]
impl Default for RecordingSearchQuery {
    fn default() -> Self {
        Self {
            camera_ids: None,
            stream_ids: None,
            start_time: None,
            end_time: None,
            event_types: None,
            schedule_id: None,
            min_duration: None,
            limit: None,
            offset: None,
        }
    }
}

/// Recording statistics model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingStats {
    pub total_count: i64,
    pub total_size_bytes: i64,
    pub total_duration_seconds: i64,
    pub oldest_recording: Option<DateTime<Utc>>,
    pub newest_recording: Option<DateTime<Utc>>,
}

/// Database query result for recording stats
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RecordingStatsDb {
    pub total_count: Option<i64>,
    pub total_size: Option<i64>,
    pub total_duration: Option<i64>,
    pub oldest: Option<DateTime<Utc>>,
    pub newest: Option<DateTime<Utc>>,
}
