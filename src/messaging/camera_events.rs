use crate::db::models::camera_models::Camera;
use crate::messaging::{broker::{MessageBroker, MessageBrokerTrait}, EventType};
use anyhow::Result;
use log::{info, warn};
use std::sync::Arc;
use uuid::Uuid;

/// Helper module for publishing camera-related events
pub struct CameraEvents {
    message_broker: Arc<MessageBroker>,
}

impl CameraEvents {
    /// Create a new camera events helper
    pub fn new(message_broker: Arc<MessageBroker>) -> Self {
        Self { message_broker }
    }

    /// Publish a camera discovered event
    pub async fn camera_discovered(&self, ip_address: &str, details: Option<serde_json::Value>) -> Result<()> {
        let mut payload = serde_json::json!({
            "ip_address": ip_address,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        if let Some(details) = details {
            if let serde_json::Value::Object(obj) = details {
                if let Some(obj_mut) = payload.as_object_mut() {
                    for (k, v) in obj {
                        obj_mut.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        self.message_broker
            .publish(EventType::CameraDiscovered, None, payload)
            .await?;

        info!("Published camera discovered event for {}", ip_address);
        Ok(())
    }

    /// Publish a camera connected event
    pub async fn camera_connected(&self, camera: &Camera) -> Result<()> {
        let payload = serde_json::json!({
            "camera_id": camera.id.to_string(),
            "name": camera.name,
            "ip_address": camera.ip_address,
            "model": camera.model,
            "manufacturer": camera.manufacturer,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        self.message_broker
            .publish(EventType::CameraConnected, Some(camera.id), payload)
            .await?;

        info!("Published camera connected event for {}", camera.id);
        Ok(())
    }

    /// Publish a camera disconnected event
    pub async fn camera_disconnected(&self, camera_id: Uuid, reason: Option<&str>) -> Result<()> {
        let payload = serde_json::json!({
            "camera_id": camera_id.to_string(),
            "reason": reason.unwrap_or("Unknown"),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        self.message_broker
            .publish(EventType::CameraDisconnected, Some(camera_id), payload)
            .await?;

        info!("Published camera disconnected event for {}", camera_id);
        Ok(())
    }

    /// Publish a camera status changed event
    pub async fn camera_status_changed(&self, camera_id: Uuid, old_status: &str, new_status: &str) -> Result<()> {
        let payload = serde_json::json!({
            "camera_id": camera_id.to_string(),
            "old_status": old_status,
            "new_status": new_status,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        self.message_broker
            .publish(EventType::CameraStatusChanged, Some(camera_id), payload)
            .await?;

        info!("Published camera status changed event for {}: {} -> {}", camera_id, old_status, new_status);
        Ok(())
    }

    /// Publish a camera settings updated event
    pub async fn camera_settings_updated(&self, camera: &Camera, updated_fields: &[&str]) -> Result<()> {
        let payload = serde_json::json!({
            "camera_id": camera.id.to_string(),
            "updated_fields": updated_fields,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        self.message_broker
            .publish(EventType::CameraSettingsUpdated, Some(camera.id), payload)
            .await?;

        info!("Published camera settings updated event for {}", camera.id);
        Ok(())
    }

    /// Publish a camera deleted event
    pub async fn camera_deleted(&self, camera_id: Uuid, camera_name: &str) -> Result<()> {
        let payload = serde_json::json!({
            "camera_id": camera_id.to_string(),
            "camera_name": camera_name,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        self.message_broker
            .publish(EventType::CameraDeleted, Some(camera_id), payload)
            .await?;

        info!("Published camera deleted event for {}", camera_id);
        Ok(())
    }
}