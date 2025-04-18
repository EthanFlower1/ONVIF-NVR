use crate::{
    db::models::{Camera, CameraDb},
    error::Error,
};
use anyhow::Result;
use chrono::Utc;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

/// Cameras repository for handling camera operations
#[derive(Clone)]
pub struct CamerasRepository {
    pool: Arc<PgPool>,
}

impl CamerasRepository {
    /// Create a new cameras repository
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Create a new camera
    pub async fn create(&self, camera: &Camera, created_by: &Uuid) -> Result<Camera> {
        info!("Creating new camera: {}", camera.name);

        // Convert Camera to CameraDb for database compatibility
        let camera_db = CameraDb::from(camera.clone());

        let result = sqlx::query_as::<_, CameraDb>(
            r#"
            INSERT INTO cameras (
                id, name, model, manufacturer, ip_address, username, password, 
                streams, onvif_endpoint, status, created_at, updated_at, created_by,
                firmware_version, serial_number, hardware_id, mac_address, 
                ptz_supported, audio_supported, analytics_supported, 
                capabilities, profiles, stream_details, last_updated
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                    $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24)
            RETURNING id, name, model, manufacturer, ip_address, username, password, 
                     streams, onvif_endpoint, status, firmware_version, serial_number,
                     hardware_id, mac_address, ptz_supported, audio_supported,
                     analytics_supported, capabilities, profiles, stream_details, last_updated
            "#,
        )
        .bind(camera_db.id)
        .bind(&camera_db.name)
        .bind(&camera_db.model)
        .bind(&camera_db.manufacturer)
        .bind(&camera_db.ip_address)
        .bind(&camera_db.username)
        .bind(&camera_db.password)
        .bind(&camera_db.streams)
        .bind(&camera_db.onvif_endpoint)
        .bind(&camera_db.status)
        .bind(Utc::now())
        .bind(Utc::now())
        .bind(created_by)
        // Extended fields
        .bind(&camera_db.firmware_version)
        .bind(&camera_db.serial_number)
        .bind(&camera_db.hardware_id)
        .bind(&camera_db.mac_address)
        .bind(camera_db.ptz_supported)
        .bind(camera_db.audio_supported)
        .bind(camera_db.analytics_supported)
        .bind(&camera_db.capabilities)
        .bind(&camera_db.profiles)
        .bind(&camera_db.stream_details)
        .bind(camera_db.last_updated)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create camera: {}", e)))?;

        // Convert back to Camera
        Ok(Camera::from(result))
    }

    /// Get camera by ID
    pub async fn get_by_id(&self, id: &Uuid) -> Result<Option<Camera>> {
        let result = sqlx::query_as::<_, CameraDb>(
            r#"
            SELECT id, name, model, manufacturer, ip_address, username, password, 
                   streams, onvif_endpoint, status, firmware_version, serial_number, 
                   hardware_id, mac_address, ptz_supported, audio_supported, 
                   analytics_supported, capabilities, profiles, stream_details, last_updated
            FROM cameras
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get camera by ID: {}", e)))?;

        // Convert CameraDb to Camera if result exists
        Ok(result.map(Camera::from))
    }

    /// Get camera by IP address
    pub async fn get_by_ip(&self, ip_address: &str) -> Result<Option<Camera>> {
        let result = sqlx::query_as::<_, CameraDb>(
            r#"
            SELECT id, name, model, manufacturer, ip_address, username, password, 
                   streams, onvif_endpoint, status, firmware_version, serial_number, 
                   hardware_id, mac_address, ptz_supported, audio_supported, 
                   analytics_supported, capabilities, profiles, stream_details, last_updated
            FROM cameras
            WHERE ip_address = $1
            "#,
        )
        .bind(ip_address)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get camera by IP: {}", e)))?;

        // Convert CameraDb to Camera if result exists
        Ok(result.map(Camera::from))
    }

    /// Update camera
    pub async fn update(&self, camera: &Camera) -> Result<Camera> {
        // Convert Camera to CameraDb
        let camera_db = CameraDb::from(camera.clone());

        let result = sqlx::query_as::<_, CameraDb>(
            r#"
            UPDATE cameras
            SET name = $1, model = $2, manufacturer = $3, ip_address = $4, 
                username = $5, password = $6, streams = $7, onvif_endpoint = $8, 
                status = $9, updated_at = $10, 
                -- Extended fields
                firmware_version = $11, serial_number = $12, hardware_id = $13,
                mac_address = $14, ptz_supported = $15, audio_supported = $16,
                analytics_supported = $17, capabilities = $18, profiles = $19,
                stream_details = $20, last_updated = $21
            WHERE id = $22
            RETURNING id, name, model, manufacturer, ip_address, username, password, 
                     streams, onvif_endpoint, status, firmware_version, serial_number,
                     hardware_id, mac_address, ptz_supported, audio_supported,
                     analytics_supported, capabilities, profiles, stream_details, last_updated
            "#,
        )
        .bind(&camera_db.name)
        .bind(&camera_db.model)
        .bind(&camera_db.manufacturer)
        .bind(&camera_db.ip_address)
        .bind(&camera_db.username)
        .bind(&camera_db.password)
        .bind(&camera_db.streams)
        .bind(&camera_db.onvif_endpoint)
        .bind(&camera_db.status)
        .bind(Utc::now())
        // Extended fields
        .bind(&camera_db.firmware_version)
        .bind(&camera_db.serial_number)
        .bind(&camera_db.hardware_id)
        .bind(&camera_db.mac_address)
        .bind(camera_db.ptz_supported)
        .bind(camera_db.audio_supported)
        .bind(camera_db.analytics_supported)
        .bind(&camera_db.capabilities)
        .bind(&camera_db.profiles)
        .bind(&camera_db.stream_details)
        .bind(camera_db.last_updated)
        // WHERE clause
        .bind(camera_db.id)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update camera: {}", e)))?;

        // Convert back to Camera
        Ok(Camera::from(result))
    }

    /// Update only the extended camera details
    pub async fn update_camera_details(&self, camera: &Camera) -> Result<Camera> {
        // Convert Camera to CameraDb
        let camera_db = CameraDb::from(camera.clone());

        let result = sqlx::query_as::<_, CameraDb>(
            r#"
            UPDATE cameras
            SET firmware_version = $1, serial_number = $2, hardware_id = $3,
                mac_address = $4, ptz_supported = $5, audio_supported = $6,
                analytics_supported = $7, capabilities = $8, profiles = $9,
                stream_details = $10, last_updated = $11, streams = $12, model = $13, manufacturer = $14,
                status = $15, updated_at = $16, name = $17
            WHERE id = $18
            RETURNING id, name, model, manufacturer, ip_address, username, password, 
                     streams, onvif_endpoint, status, firmware_version, serial_number,
                     hardware_id, mac_address, ptz_supported, audio_supported,
                     analytics_supported, capabilities, profiles, stream_details, last_updated
            "#
        )
        .bind(&camera_db.firmware_version)
        .bind(&camera_db.serial_number)
        .bind(&camera_db.hardware_id)
        .bind(&camera_db.mac_address)
        .bind(camera_db.ptz_supported)
        .bind(camera_db.audio_supported)
        .bind(camera_db.analytics_supported)
        .bind(&camera_db.capabilities)
        .bind(&camera_db.profiles)
        .bind(&camera_db.stream_details)
        .bind(camera_db.last_updated)
        .bind(&camera_db.streams)
        .bind(&camera_db.model)
        .bind(&camera_db.manufacturer)
        .bind(&camera_db.status)
        .bind(Utc::now())
        .bind(&camera_db.name)
        .bind(camera_db.id)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update camera details: {}", e)))?;

        // Convert back to Camera
        Ok(Camera::from(result))
    }

    /// Delete camera
    pub async fn delete(&self, id: &Uuid) -> Result<bool> {
        let result = sqlx::query(
            r#"
            DELETE FROM cameras
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to delete camera: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all cameras
    pub async fn get_all(&self) -> Result<Vec<Camera>> {
        let result = sqlx::query_as::<_, CameraDb>(
            r#"
            SELECT id, name, model, manufacturer, ip_address, username, password, 
                   streams, onvif_endpoint, status, firmware_version, serial_number, 
                   hardware_id, mac_address, ptz_supported, audio_supported, 
                   analytics_supported, capabilities, profiles, stream_details, last_updated
            FROM cameras
            ORDER BY name
            "#,
        )
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get all cameras: {}", e)))?;

        // Convert all CameraDb to Camera
        Ok(result.into_iter().map(Camera::from).collect())
    }

    /// Get first active camera
    pub async fn get_first_active(&self) -> Result<Option<Camera>> {
        let result = sqlx::query_as::<_, CameraDb>(
            r#"
            SELECT id, name, model, manufacturer, ip_address, username, password, 
                   streams, onvif_endpoint, status, firmware_version, serial_number, 
                   hardware_id, mac_address, ptz_supported, audio_supported, 
                   analytics_supported, capabilities, profiles, stream_details, last_updated
            FROM cameras
            WHERE status = 'active'
            ORDER BY name
            LIMIT 1
            "#,
        )
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get first active camera: {}", e)))?;

        // Convert CameraDb to Camera if result exists
        Ok(result.map(Camera::from))
    }

    /// Update camera status
    pub async fn update_status(&self, id: &Uuid, status: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE cameras
            SET status = $1, updated_at = $2
            WHERE id = $3
            "#,
        )
        .bind(status)
        .bind(Utc::now())
        .bind(id)
        .execute(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update camera status: {}", e)))?;

        Ok(())
    }
}

