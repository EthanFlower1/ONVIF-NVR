use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::recording_schedule_models::RecordingQuality;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EventSettings {
    pub id: Uuid,
    pub camera_id: Uuid,
    pub enabled: bool,
    pub event_types: Vec<String>, // Event types to subscribe to
    pub event_topic_expressions: Vec<String>, // ONVIF topic expressions
    pub trigger_recording: bool,  // Whether to trigger recording on events
    pub recording_duration: u64,  // Duration to record in seconds when event triggered
    pub recording_quality: RecordingQuality, // Recording quality for event recordings
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Uuid, // User ID
}
