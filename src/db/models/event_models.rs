use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Event model
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Event {
    pub id: Uuid,
    pub camera_id: Uuid,
    pub event_type: String,
    pub severity: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub duration: Option<i32>,
    pub confidence: Option<f32>,
    pub metadata: Option<serde_json::Value>,
    pub thumbnail_path: Option<String>,
    pub video_clip_path: Option<String>,
    pub acknowledged: Option<bool>,
    pub acknowledged_by: Option<Uuid>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}
