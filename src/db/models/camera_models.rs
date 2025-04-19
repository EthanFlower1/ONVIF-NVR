use super::stream_models::{Stream, StreamReference};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Camera model
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Camera {
    pub id: Uuid,
    pub name: String,
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub ip_address: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub onvif_endpoint: Option<String>,
    pub status: String,
    pub primary_stream_id: Option<Uuid>,
    pub sub_stream_id: Option<Uuid>,
    pub firmware_version: Option<String>,
    pub serial_number: Option<String>,
    pub hardware_id: Option<String>,
    pub mac_address: Option<String>,

    pub ptz_supported: Option<bool>,
    pub audio_supported: Option<bool>,
    pub analytics_supported: Option<bool>,

    // Events support
    pub events_supported: Option<serde_json::Value>,
    pub event_notification_endpoint: Option<String>,
    // Storage information
    pub has_local_storage: Option<bool>,
    pub storage_type: Option<String>,
    pub storage_capacity_gb: Option<i32>,
    pub storage_used_gb: Option<i32>,
    pub retention_days: Option<i32>,
    pub recording_mode: Option<String>,
    // Analytics information
    pub analytics_capabilities: Option<serde_json::Value>,
    pub ai_processor_type: Option<String>,
    pub ai_processor_model: Option<String>,
    pub object_detection_supported: Option<bool>,
    pub face_detection_supported: Option<bool>,
    pub license_plate_recognition_supported: Option<bool>,
    pub person_tracking_supported: Option<bool>,
    pub line_crossing_supported: Option<bool>,
    pub zone_intrusion_supported: Option<bool>,
    pub object_classification_supported: Option<bool>,
    pub behavior_analysis_supported: Option<bool>,
    // Original fields (mapped to our new structure)
    pub capabilities: Option<serde_json::Value>,
    pub profiles: Option<serde_json::Value>,
    pub last_updated: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Uuid,
}
impl Camera {
    pub(crate) fn default() -> Camera {
        Camera {
            id: Uuid::new_v4(),
            name: String::new(),
            model: None,
            manufacturer: None,
            ip_address: String::new(),
            username: None,
            password: None,
            onvif_endpoint: None,
            status: "discovered".to_string(),
            primary_stream_id: None,
            sub_stream_id: None,
            firmware_version: None,
            serial_number: None,
            hardware_id: None,
            mac_address: None,
            ptz_supported: None,
            audio_supported: None,
            analytics_supported: None,
            events_supported: None,
            event_notification_endpoint: None,
            has_local_storage: None,
            storage_type: None,
            storage_capacity_gb: None,
            storage_used_gb: None,
            retention_days: None,
            recording_mode: None,
            analytics_capabilities: None,
            ai_processor_type: None,
            ai_processor_model: None,
            object_detection_supported: None,
            face_detection_supported: None,
            license_plate_recognition_supported: None,
            person_tracking_supported: None,
            line_crossing_supported: None,
            zone_intrusion_supported: None,
            object_classification_supported: None,
            behavior_analysis_supported: None,
            capabilities: None,
            profiles: None,
            last_updated: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: Uuid::nil(), // System user ID
        }
    }
}

/// Helper struct for camera with streams
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CameraWithStreams {
    pub camera: Camera,
    pub streams: Vec<Stream>,
    pub stream_references: Vec<StreamReference>,
}
