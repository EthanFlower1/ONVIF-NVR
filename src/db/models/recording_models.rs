use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Recording model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    pub id: Uuid,
    pub camera_id: Uuid,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub file_path: PathBuf,
    pub file_size: u64,
    pub duration: u64,
    pub format: String,
    pub resolution: String,
    pub fps: u32,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RecordingDb {
    pub id: Uuid,
    pub camera_id: Uuid,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub file_path: String,
    pub file_size: i64,
    pub duration: i64,
    pub format: String,
    pub resolution: String,
    pub fps: i32,
    pub metadata: Option<serde_json::Value>,
}

impl From<RecordingDb> for Recording {
    fn from(db: RecordingDb) -> Self {
        Self {
            id: db.id,
            camera_id: db.camera_id,
            start_time: db.start_time,
            end_time: db.end_time,
            file_path: PathBuf::from(db.file_path),
            file_size: db.file_size as u64,
            duration: db.duration as u64,
            format: db.format,
            resolution: db.resolution,
            fps: db.fps as u32,
            metadata: db.metadata,
        }
    }
}

impl From<Recording> for RecordingDb {
    fn from(r: Recording) -> Self {
        Self {
            id: r.id,
            camera_id: r.camera_id,
            start_time: r.start_time,
            end_time: r.end_time,
            file_path: r.file_path.to_string_lossy().to_string(),
            file_size: r.file_size as i64,
            duration: r.duration as i64,
            format: r.format,
            resolution: r.resolution,
            fps: r.fps as i32,
            metadata: r.metadata,
        }
    }
}

/// Search query model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingSearchQuery {
    pub camera_ids: Option<Vec<Uuid>>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[cfg(test)]
impl Default for RecordingSearchQuery {
    fn default() -> Self {
        Self {
            camera_ids: None,
            start_time: None,
            end_time: None,
            limit: None,
            offset: None,
        }
    }
}
