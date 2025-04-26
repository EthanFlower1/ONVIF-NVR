use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use uuid::Uuid;

/// Event types supported by the system
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EventType {
    // Camera events
    CameraDiscovered,
    CameraConnected,
    CameraDisconnected,
    CameraStatusChanged,
    CameraSettingsUpdated,
    CameraDeleted,
    
    // Stream events
    StreamStarted,
    StreamStopped,
    StreamError,
    
    // Recording events
    RecordingStarted,
    RecordingStopped,
    RecordingCompleted,
    RecordingError,
    RecordingDeleted,
    
    // Storage events
    StorageCleanupStarted,
    StorageCleanupCompleted,
    StorageLimitReached,
    
    // Motion detection events
    MotionDetected,
    MotionStopped,
    
    // Analytics events
    ObjectDetected,
    LineDetected,
    ZoneIntrusion,
    FaceDetected,
    
    // System events
    SystemStartup,
    SystemShutdown,
    
    // Custom event
    Custom(String),
}

impl Display for EventType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CameraDiscovered => write!(f, "camera.discovered"),
            Self::CameraConnected => write!(f, "camera.connected"),
            Self::CameraDisconnected => write!(f, "camera.disconnected"),
            Self::CameraStatusChanged => write!(f, "camera.status_changed"),
            Self::CameraSettingsUpdated => write!(f, "camera.settings_updated"),
            Self::CameraDeleted => write!(f, "camera.deleted"),
            Self::StreamStarted => write!(f, "stream.started"),
            Self::StreamStopped => write!(f, "stream.stopped"),
            Self::StreamError => write!(f, "stream.error"),
            Self::RecordingStarted => write!(f, "recording.started"),
            Self::RecordingStopped => write!(f, "recording.stopped"),
            Self::RecordingCompleted => write!(f, "recording.completed"),
            Self::RecordingError => write!(f, "recording.error"),
            Self::RecordingDeleted => write!(f, "recording.deleted"),
            Self::StorageCleanupStarted => write!(f, "storage.cleanup_started"),
            Self::StorageCleanupCompleted => write!(f, "storage.cleanup_completed"),
            Self::StorageLimitReached => write!(f, "storage.limit_reached"),
            Self::MotionDetected => write!(f, "motion.detected"),
            Self::MotionStopped => write!(f, "motion.stopped"),
            Self::ObjectDetected => write!(f, "analytics.object_detected"),
            Self::LineDetected => write!(f, "analytics.line_detected"),
            Self::ZoneIntrusion => write!(f, "analytics.zone_intrusion"),
            Self::FaceDetected => write!(f, "analytics.face_detected"),
            Self::SystemStartup => write!(f, "system.startup"),
            Self::SystemShutdown => write!(f, "system.shutdown"),
            Self::Custom(name) => write!(f, "custom.{}", name),
        }
    }
}

/// Event message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMessage {
    /// Unique event ID
    pub id: Uuid,
    /// Event type
    pub event_type: EventType,
    /// Event source ID (e.g., camera ID)
    pub source_id: Option<Uuid>,
    /// Event timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Event data payload
    pub payload: serde_json::Value,
}

impl EventMessage {
    /// Create a new event message
    pub fn new<T: Serialize>(
        event_type: EventType,
        source_id: Option<Uuid>,
        payload: T,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            id: Uuid::new_v4(),
            event_type,
            source_id,
            timestamp: chrono::Utc::now(),
            payload: serde_json::to_value(payload)?,
        })
    }
    
    /// Create a new event message with empty payload
    pub fn new_empty(event_type: EventType, source_id: Option<Uuid>) -> Self {
        Self {
            id: Uuid::new_v4(),
            event_type,
            source_id,
            timestamp: chrono::Utc::now(),
            payload: serde_json::Value::Null,
        }
    }
    
    /// Get the routing key for the event
    pub fn routing_key(&self) -> String {
        match &self.source_id {
            Some(id) => format!("{}.{}", self.event_type, id),
            None => self.event_type.to_string(),
        }
    }
}