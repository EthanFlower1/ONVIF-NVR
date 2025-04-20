use anyhow::Result;
use chrono::Utc;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::{
    db::models::{
        camera_models::{Camera, CameraWithStreams},
        stream_models::{ReferenceType, Stream, StreamReference},
    },
    Error,
};

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

    /// Create a new camera with streams
    pub async fn create_with_streams(
        &self,
        camera_data: &CameraWithStreams,
    ) -> Result<CameraWithStreams, anyhow::Error> {
        info!(
            "Creating new camera with streams: {}",
            camera_data.camera.name
        );

        // Begin transaction
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(format!("Failed to begin transaction: {}", e)))?;

        // Prepare camera data
        let mut camera_db = camera_data.camera.clone();

        // Ensure created_at, and updated_at are set
        camera_db.created_at = Utc::now();
        camera_db.updated_at = Utc::now();

        // Insert camera
        let camera_result = sqlx::query_as::<_, Camera>(
            r#"
            INSERT INTO cameras (
                id, name, model, manufacturer, ip_address, username, password, 
                onvif_endpoint, status, primary_stream_id, sub_stream_id,
                firmware_version, serial_number, hardware_id, mac_address, 
                ptz_supported, audio_supported, analytics_supported,
                events_supported, event_notification_endpoint,
                has_local_storage, storage_type, storage_capacity_gb, storage_used_gb,
                retention_days, recording_mode,
                analytics_capabilities, ai_processor_type, ai_processor_model,
                object_detection_supported, face_detection_supported,
                license_plate_recognition_supported, person_tracking_supported,
                line_crossing_supported, zone_intrusion_supported,
                object_classification_supported, behavior_analysis_supported,
                capabilities, profiles, last_updated, 
                created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, 
                   $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29,
                   $30, $31, $32, $33, $34, $35, $36, $37, $38, $39, $40, $41, $42)
            RETURNING *
            "#,
        )
        .bind(camera_db.id)
        .bind(&camera_db.name)
        .bind(&camera_db.model)
        .bind(&camera_db.manufacturer)
        .bind(&camera_db.ip_address)
        .bind(&camera_db.username)
        .bind(&camera_db.password)
        .bind(&camera_db.onvif_endpoint)
        .bind(&camera_db.status)
        .bind(camera_db.primary_stream_id)
        .bind(camera_db.sub_stream_id)
        .bind(&camera_db.firmware_version)
        .bind(&camera_db.serial_number)
        .bind(&camera_db.hardware_id)
        .bind(&camera_db.mac_address)
        .bind(camera_db.ptz_supported)
        .bind(camera_db.audio_supported)
        .bind(camera_db.analytics_supported)
        .bind(&camera_db.events_supported)
        .bind(&camera_db.event_notification_endpoint)
        .bind(camera_db.has_local_storage)
        .bind(&camera_db.storage_type)
        .bind(camera_db.storage_capacity_gb)
        .bind(camera_db.storage_used_gb)
        .bind(camera_db.retention_days)
        .bind(&camera_db.recording_mode)
        .bind(&camera_db.analytics_capabilities)
        .bind(&camera_db.ai_processor_type)
        .bind(&camera_db.ai_processor_model)
        .bind(camera_db.object_detection_supported)
        .bind(camera_db.face_detection_supported)
        .bind(camera_db.license_plate_recognition_supported)
        .bind(camera_db.person_tracking_supported)
        .bind(camera_db.line_crossing_supported)
        .bind(camera_db.zone_intrusion_supported)
        .bind(camera_db.object_classification_supported)
        .bind(camera_db.behavior_analysis_supported)
        .bind(&camera_db.capabilities)
        .bind(&camera_db.profiles)
        .bind(camera_db.last_updated)
        .bind(camera_db.created_at)
        .bind(camera_db.updated_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| Error::Database(format!("Failed to create camera: {}", e)))?;

        // Create streams and references
        let mut streams = Vec::new();
        let mut stream_references = Vec::new();

        // Track primary and sub stream IDs
        let mut primary_stream_id = None;
        let mut sub_stream_id = None;

        // Insert each stream
        for stream_data in &camera_data.streams {
            let mut stream_db = stream_data.clone();
            stream_db.camera_id = camera_result.id;
            stream_db.created_at = Utc::now();
            stream_db.updated_at = Utc::now();

            let stream_result = sqlx::query_as::<_, Stream>(
                r#"
                INSERT INTO streams (
                    id, camera_id, name, stream_type, url, 
                    resolution, width, height, codec, profile, level,
                    framerate, bitrate, variable_bitrate, keyframe_interval,
                    quality_level, transport_protocol, authentication_required,
                    is_primary, is_audio_enabled, audio_codec, audio_bitrate,
                    audio_channels, audio_sample_rate, is_active, last_connected_at,
                    created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 
                        $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)
                RETURNING *
                "#,
            )
            .bind(stream_db.id)
            .bind(stream_db.camera_id)
            .bind(&stream_db.name)
            .bind(&stream_db.stream_type.to_string()) // Convert enum to string
            .bind(&stream_db.url)
            .bind(&stream_db.resolution)
            .bind(stream_db.width)
            .bind(stream_db.height)
            .bind(&stream_db.codec)
            .bind(&stream_db.profile)
            .bind(&stream_db.level)
            .bind(stream_db.framerate)
            .bind(stream_db.bitrate)
            .bind(stream_db.variable_bitrate)
            .bind(stream_db.keyframe_interval)
            .bind(&stream_db.quality_level)
            .bind(&stream_db.transport_protocol)
            .bind(stream_db.authentication_required)
            .bind(stream_db.is_primary)
            .bind(stream_db.is_audio_enabled)
            .bind(&stream_db.audio_codec)
            .bind(stream_db.audio_bitrate)
            .bind(stream_db.audio_channels)
            .bind(stream_db.audio_sample_rate)
            .bind(stream_db.is_active)
            .bind(stream_db.last_connected_at)
            .bind(stream_db.created_at)
            .bind(stream_db.updated_at)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| Error::Database(format!("Failed to create stream: {}", e)))?;

            // Track primary stream ID if this is a primary stream
            if stream_data.is_primary.unwrap_or(false) {
                primary_stream_id = Some(stream_result.id);
            }

            streams.push(stream_result);
        }

        // Insert each stream reference
        for reference_data in &camera_data.stream_references {
            let mut ref_db = reference_data.clone();
            ref_db.camera_id = camera_result.id;
            ref_db.created_at = Utc::now();
            ref_db.updated_at = Utc::now();

            // Find the stream ID from our newly created streams
            if let Some(stream_index) = camera_data
                .streams
                .iter()
                .position(|s| s.id == reference_data.stream_id)
            {
                ref_db.stream_id = streams[stream_index].id;

                // Track sub stream ID if this is a sub stream reference
                if reference_data.reference_type == ReferenceType::Sub {
                    sub_stream_id = Some(ref_db.stream_id);
                }

                let ref_result = sqlx::query_as::<_, StreamReference>(
                    r#"
                INSERT INTO stream_references (
                    id, camera_id, stream_id, reference_type, 
                    display_order, is_default, created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                RETURNING *
                "#,
                )
                .bind(ref_db.id)
                .bind(ref_db.camera_id)
                .bind(ref_db.stream_id)
                .bind(&ref_db.reference_type.to_string())
                .bind(ref_db.display_order)
                .bind(ref_db.is_default)
                .bind(ref_db.created_at)
                .bind(ref_db.updated_at)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| {
                    Error::Database(format!("Failed to create stream reference: {}", e))
                })?;

                // Add the reference to our vector
                stream_references.push(ref_result);
            }
        } // End of for loop - this was missing a closing brace in your code

        // Update camera with primary and sub stream IDs if found - This was inside the loop, but should be outside
        if primary_stream_id.is_some() || sub_stream_id.is_some() {
            sqlx::query(
                r#"
            UPDATE cameras
            SET primary_stream_id = $1, sub_stream_id = $2, updated_at = $3
            WHERE id = $4
            "#,
            )
            .bind(primary_stream_id)
            .bind(sub_stream_id)
            .bind(Utc::now())
            .bind(camera_result.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                Error::Database(format!("Failed to update camera with stream IDs: {}", e))
            })?;
        }

        // Commit transaction - This was inside the loop, but should be outside
        tx.commit()
            .await
            .map_err(|e| Error::Database(format!("Failed to commit transaction: {}", e)))?;

        info!("Successfully created camera with streams");
        // Return the created camera with streams - This was inside the loop, but should be outside
        Ok(CameraWithStreams {
            camera: camera_result,
            streams,
            stream_references,
        })
    }

    /// Get camera with streams by ID
    pub async fn get_with_streams_by_id(&self, id: &Uuid) -> Result<Option<CameraWithStreams>> {
        // Get the camera
        let camera_result = sqlx::query_as::<_, Camera>(
            r#"
            SELECT * FROM cameras
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get camera by ID: {}", e)))?;

        // If camera not found, return None
        let camera = match camera_result {
            Some(c) => c,
            None => return Ok(None),
        };

        // Get all streams for this camera
        let streams = sqlx::query_as::<_, Stream>(
            r#"
            SELECT * FROM streams
            WHERE camera_id = $1
            "#,
        )
        .bind(id)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get streams for camera: {}", e)))?;

        // Get all stream references for this camera
        let stream_references = sqlx::query_as::<_, StreamReference>(
            r#"
            SELECT * FROM stream_references
            WHERE camera_id = $1
            ORDER BY display_order
            "#,
        )
        .bind(id)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| {
            Error::Database(format!("Failed to get stream references for camera: {}", e))
        })?;

        Ok(Some(CameraWithStreams {
            camera,
            streams,
            stream_references,
        }))
    }

    /// Get camera by ID
    pub async fn get_by_id(&self, id: &Uuid) -> Result<Option<Camera>> {
        let result = sqlx::query_as::<_, Camera>(
            r#"
            SELECT * FROM cameras
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get camera by ID: {}", e)))?;

        Ok(result)
    }

    /// Get camera by IP address
    pub async fn get_by_ip(&self, ip_address: &str) -> Result<Option<Camera>> {
        let result = sqlx::query_as::<_, Camera>(
            r#"
            SELECT * FROM cameras
            WHERE ip_address = $1
            "#,
        )
        .bind(ip_address)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get camera by IP: {}", e)))?;

        Ok(result)
    }

    /// Update camera
    pub async fn update(&self, camera: &Camera) -> Result<Camera> {
        // Prepare updated camera data
        let mut camera_db = camera.clone();
        camera_db.updated_at = Utc::now();

        let result = sqlx::query_as::<_, Camera>(
            r#"
            UPDATE cameras
            SET name = $1, model = $2, manufacturer = $3, ip_address = $4, 
                username = $5, password = $6, onvif_endpoint = $7, 
                status = $8, updated_at = $9, primary_stream_id = $10, sub_stream_id = $11,
                firmware_version = $12, serial_number = $13, hardware_id = $14,
                mac_address = $15, ptz_supported = $16, audio_supported = $17,
                analytics_supported = $18, events_supported = $19, event_notification_endpoint = $20,
                has_local_storage = $21, storage_type = $22, storage_capacity_gb = $23, 
                storage_used_gb = $24, retention_days = $25, recording_mode = $26,
                analytics_capabilities = $27, ai_processor_type = $28, ai_processor_model = $29,
                object_detection_supported = $30, face_detection_supported = $31, 
                license_plate_recognition_supported = $32, person_tracking_supported = $33,
                line_crossing_supported = $34, zone_intrusion_supported = $35,
                object_classification_supported = $36, behavior_analysis_supported = $37,
                capabilities = $38, profiles = $39, last_updated = $40
            WHERE id = $41
            RETURNING *
            "#,
        )
        .bind(&camera_db.name)
        .bind(&camera_db.model)
        .bind(&camera_db.manufacturer)
        .bind(&camera_db.ip_address)
        .bind(&camera_db.username)
        .bind(&camera_db.password)
        .bind(&camera_db.onvif_endpoint)
        .bind(&camera_db.status)
        .bind(camera_db.updated_at)
        .bind(camera_db.primary_stream_id)
        .bind(camera_db.sub_stream_id)
        .bind(&camera_db.firmware_version)
        .bind(&camera_db.serial_number)
        .bind(&camera_db.hardware_id)
        .bind(&camera_db.mac_address)
        .bind(camera_db.ptz_supported)
        .bind(camera_db.audio_supported)
        .bind(camera_db.analytics_supported)
        .bind(&camera_db.events_supported)
        .bind(&camera_db.event_notification_endpoint)
        .bind(camera_db.has_local_storage)
        .bind(&camera_db.storage_type)
        .bind(camera_db.storage_capacity_gb)
        .bind(camera_db.storage_used_gb)
        .bind(camera_db.retention_days)
        .bind(&camera_db.recording_mode)
        .bind(&camera_db.analytics_capabilities)
        .bind(&camera_db.ai_processor_type)
        .bind(&camera_db.ai_processor_model)
        .bind(camera_db.object_detection_supported)
        .bind(camera_db.face_detection_supported)
        .bind(camera_db.license_plate_recognition_supported)
        .bind(camera_db.person_tracking_supported)
        .bind(camera_db.line_crossing_supported)
        .bind(camera_db.zone_intrusion_supported)
        .bind(camera_db.object_classification_supported)
        .bind(camera_db.behavior_analysis_supported)
        .bind(&camera_db.capabilities)
        .bind(&camera_db.profiles)
        .bind(camera_db.last_updated)
        .bind(camera_db.id)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update camera: {}", e)))?;

        Ok(result)
    }

    /// Update camera with streams
    pub async fn update_with_streams(
        &self,
        camera_data: &CameraWithStreams,
    ) -> Result<CameraWithStreams> {
        // Begin transaction
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(format!("Failed to begin transaction: {}", e)))?;

        // Update camera
        let mut camera_db = camera_data.camera.clone();
        camera_db.updated_at = Utc::now();

        let camera_result = sqlx::query_as::<_, Camera>(
            r#"
            UPDATE cameras
            SET name = $1, model = $2, manufacturer = $3, ip_address = $4, 
                username = $5, password = $6, onvif_endpoint = $7, 
                status = $8, updated_at = $9, primary_stream_id = $10, sub_stream_id = $11,
                firmware_version = $12, serial_number = $13, hardware_id = $14,
                mac_address = $15, ptz_supported = $16, audio_supported = $17,
                analytics_supported = $18, events_supported = $19, event_notification_endpoint = $20,
                has_local_storage = $21, storage_type = $22, storage_capacity_gb = $23, 
                storage_used_gb = $24, retention_days = $25, recording_mode = $26,
                analytics_capabilities = $27, ai_processor_type = $28, ai_processor_model = $29,
                object_detection_supported = $30, face_detection_supported = $31, 
                license_plate_recognition_supported = $32, person_tracking_supported = $33,
                line_crossing_supported = $34, zone_intrusion_supported = $35,
                object_classification_supported = $36, behavior_analysis_supported = $37,
                capabilities = $38, profiles = $39, last_updated = $40
            WHERE id = $41
            RETURNING *
            "#,
        )
        .bind(&camera_db.name)
        .bind(&camera_db.model)
        .bind(&camera_db.manufacturer)
        .bind(&camera_db.ip_address)
        .bind(&camera_db.username)
        .bind(&camera_db.password)
        .bind(&camera_db.onvif_endpoint)
        .bind(&camera_db.status)
        .bind(camera_db.updated_at)
        .bind(camera_db.primary_stream_id)
        .bind(camera_db.sub_stream_id)
        .bind(&camera_db.firmware_version)
        .bind(&camera_db.serial_number)
        .bind(&camera_db.hardware_id)
        .bind(&camera_db.mac_address)
        .bind(camera_db.ptz_supported)
        .bind(camera_db.audio_supported)
        .bind(camera_db.analytics_supported)
        .bind(&camera_db.events_supported)
        .bind(&camera_db.event_notification_endpoint)
        .bind(camera_db.has_local_storage)
        .bind(&camera_db.storage_type)
        .bind(camera_db.storage_capacity_gb)
        .bind(camera_db.storage_used_gb)
        .bind(camera_db.retention_days)
        .bind(&camera_db.recording_mode)
        .bind(&camera_db.analytics_capabilities)
        .bind(&camera_db.ai_processor_type)
        .bind(&camera_db.ai_processor_model)
        .bind(camera_db.object_detection_supported)
        .bind(camera_db.face_detection_supported)
        .bind(camera_db.license_plate_recognition_supported)
        .bind(camera_db.person_tracking_supported)
        .bind(camera_db.line_crossing_supported)
        .bind(camera_db.zone_intrusion_supported)
        .bind(camera_db.object_classification_supported)
        .bind(camera_db.behavior_analysis_supported)
        .bind(&camera_db.capabilities)
        .bind(&camera_db.profiles)
        .bind(camera_db.last_updated)
        .bind(camera_db.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| Error::Database(format!("Failed to update camera: {}", e)))?;

        // Update or insert streams
        let mut updated_streams = Vec::new();
        for stream_data in &camera_data.streams {
            let mut stream_db = stream_data.clone();
            stream_db.camera_id = camera_result.id;
            stream_db.updated_at = Utc::now();

            // Check if stream exists
            let existing_stream =
                sqlx::query_as::<_, Stream>("SELECT * FROM streams WHERE id = $1")
                    .bind(stream_db.id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| {
                        Error::Database(format!("Failed to check stream existence: {}", e))
                    })?;

            let stream_result = if existing_stream.is_some() {
                // Update existing stream
                sqlx::query_as::<_, Stream>(
                    r#"
                    UPDATE streams
                    SET name = $1, stream_type = $2, url = $3, resolution = $4, 
                        width = $5, height = $6, codec = $7, profile = $8, level = $9,
                        framerate = $10, bitrate = $11, variable_bitrate = $12,
                        keyframe_interval = $13, quality_level = $14,
                        transport_protocol = $15, authentication_required = $16,
                        is_primary = $17, is_audio_enabled = $18, audio_codec = $19,
                        audio_bitrate = $20, audio_channels = $21, audio_sample_rate = $22,
                        is_active = $23, last_connected_at = $24, updated_at = $25
                    WHERE id = $26 AND camera_id = $27
                    RETURNING *
                    "#,
                )
                .bind(&stream_db.name)
                .bind(&stream_db.stream_type.to_string()) // Convert enum to string
                .bind(&stream_db.url)
                .bind(&stream_db.resolution)
                .bind(stream_db.width)
                .bind(stream_db.height)
                .bind(&stream_db.codec)
                .bind(&stream_db.profile)
                .bind(&stream_db.level)
                .bind(stream_db.framerate)
                .bind(stream_db.bitrate)
                .bind(stream_db.variable_bitrate)
                .bind(stream_db.keyframe_interval)
                .bind(&stream_db.quality_level)
                .bind(&stream_db.transport_protocol)
                .bind(stream_db.authentication_required)
                .bind(stream_db.is_primary)
                .bind(stream_db.is_audio_enabled)
                .bind(&stream_db.audio_codec)
                .bind(stream_db.audio_bitrate)
                .bind(stream_db.audio_channels)
                .bind(stream_db.audio_sample_rate)
                .bind(stream_db.is_active)
                .bind(stream_db.last_connected_at)
                .bind(stream_db.updated_at)
                .bind(stream_db.id)
                .bind(stream_db.camera_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| Error::Database(format!("Failed to update stream: {}", e)))?
            } else {
                // Insert new stream
                stream_db.created_at = Utc::now();
                sqlx::query_as::<_, Stream>(
                    r#"
                    INSERT INTO streams (
                        id, camera_id, name, stream_type, url, 
                        resolution, width, height, codec, profile, level,
                        framerate, bitrate, variable_bitrate, keyframe_interval,
                        quality_level, transport_protocol, authentication_required,
                        is_primary, is_audio_enabled, audio_codec, audio_bitrate,
                        audio_channels, audio_sample_rate, is_active, last_connected_at,
                        created_at, updated_at
                    )
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 
                            $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)
                    RETURNING *
                    "#,
                )
                .bind(stream_db.id)
                .bind(stream_db.camera_id)
                .bind(&stream_db.name)
                .bind(&stream_db.stream_type.to_string()) // Convert enum to string
                .bind(&stream_db.url)
                .bind(&stream_db.resolution)
                .bind(stream_db.width)
                .bind(stream_db.height)
                .bind(&stream_db.codec)
                .bind(&stream_db.profile)
                .bind(&stream_db.level)
                .bind(stream_db.framerate)
                .bind(stream_db.bitrate)
                .bind(stream_db.variable_bitrate)
                .bind(stream_db.keyframe_interval)
                .bind(&stream_db.quality_level)
                .bind(&stream_db.transport_protocol)
                .bind(stream_db.authentication_required)
                .bind(stream_db.is_primary)
                .bind(stream_db.is_audio_enabled)
                .bind(&stream_db.audio_codec)
                .bind(stream_db.audio_bitrate)
                .bind(stream_db.audio_channels)
                .bind(stream_db.audio_sample_rate)
                .bind(stream_db.is_active)
                .bind(stream_db.last_connected_at)
                .bind(stream_db.created_at)
                .bind(stream_db.updated_at)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| Error::Database(format!("Failed to create stream: {}", e)))?
            };

            updated_streams.push(stream_result);
        }

        // Update or insert stream references
        let mut updated_references = Vec::new();
        for reference_data in &camera_data.stream_references {
            let mut ref_db = reference_data.clone();
            ref_db.camera_id = camera_result.id;
            ref_db.updated_at = Utc::now();

            // Check if reference exists
            let existing_ref = sqlx::query_as::<_, StreamReference>(
                "SELECT * FROM stream_references WHERE id = $1",
            )
            .bind(ref_db.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| Error::Database(format!("Failed to check reference existence: {}", e)))?;

            let ref_result = if existing_ref.is_some() {
                // Update existing reference
                sqlx::query_as::<_, StreamReference>(
                    r#"
                    UPDATE stream_references
                    SET stream_id = $1, reference_type = $2, display_order = $3,
                        is_default = $4, updated_at = $5
                    WHERE id = $6 AND camera_id = $7
                    RETURNING *
                    "#,
                )
                .bind(ref_db.stream_id)
                .bind(&ref_db.reference_type.to_string())
                .bind(ref_db.display_order)
                .bind(ref_db.is_default)
                .bind(ref_db.updated_at)
                .bind(ref_db.id)
                .bind(ref_db.camera_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| Error::Database(format!("Failed to update stream reference: {}", e)))?
            } else {
                // Insert new reference
                ref_db.created_at = Utc::now();
                sqlx::query_as::<_, StreamReference>(
                    r#"
                    INSERT INTO stream_references (
                        id, camera_id, stream_id, reference_type, 
                        display_order, is_default, created_at, updated_at
                    )
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    RETURNING *
                    "#,
                )
                .bind(ref_db.id)
                .bind(ref_db.camera_id)
                .bind(ref_db.stream_id)
                .bind(&ref_db.reference_type.to_string())
                .bind(ref_db.display_order)
                .bind(ref_db.is_default)
                .bind(ref_db.created_at)
                .bind(ref_db.updated_at)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| Error::Database(format!("Failed to create stream reference: {}", e)))?
            };

            updated_references.push(ref_result);
        }

        // Update primary and sub stream IDs if needed
        let primary_stream = updated_streams
            .iter()
            .find(|s| s.is_primary.unwrap_or(false));
        let primary_stream_id = primary_stream.map(|s| s.id);

        let sub_stream_ref = updated_references
            .iter()
            .find(|r| r.reference_type == ReferenceType::Sub);
        let sub_stream_id = sub_stream_ref.map(|r| r.stream_id);

        if primary_stream_id != camera_result.primary_stream_id
            || sub_stream_id != camera_result.sub_stream_id
        {
            sqlx::query(
                r#"
                UPDATE cameras
                SET primary_stream_id = $1, sub_stream_id = $2, updated_at = $3
                WHERE id = $4
                "#,
            )
            .bind(primary_stream_id)
            .bind(sub_stream_id)
            .bind(Utc::now())
            .bind(camera_result.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Database(format!("Failed to update camera stream IDs: {}", e)))?;
        }

        // Commit transaction
        tx.commit()
            .await
            .map_err(|e| Error::Database(format!("Failed to commit transaction: {}", e)))?;

        Ok(CameraWithStreams {
            camera: camera_result,
            streams: updated_streams,
            stream_references: updated_references,
        })
    }

    /// Delete camera and all associated streams and references
    pub async fn delete(&self, id: &Uuid) -> Result<bool> {
        // Begin transaction
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(format!("Failed to begin transaction: {}", e)))?;

        // The ON DELETE CASCADE constraints will handle deleting streams and references
        // but we'll do it explicitly to be clear about the operation

        // Delete stream references
        sqlx::query(
            r#"
            DELETE FROM stream_references
            WHERE camera_id = $1
            "#,
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            Error::Database(format!("Failed to delete camera stream references: {}", e))
        })?;

        // Delete streams
        sqlx::query(
            r#"
            DELETE FROM streams
            WHERE camera_id = $1
            "#,
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Database(format!("Failed to delete camera streams: {}", e)))?;

        // Delete camera
        let result = sqlx::query(
            r#"
            DELETE FROM cameras
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Database(format!("Failed to delete camera: {}", e)))?;

        // Commit transaction
        tx.commit()
            .await
            .map_err(|e| Error::Database(format!("Failed to commit transaction: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all cameras
    pub async fn get_all(&self) -> Result<Vec<Camera>> {
        let result = sqlx::query_as::<_, Camera>(
            r#"
            SELECT * FROM cameras
            ORDER BY name
            "#,
        )
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get all cameras: {}", e)))?;

        Ok(result)
    }

    /// Get all cameras with their streams
    pub async fn get_all_with_streams(&self) -> Result<Vec<CameraWithStreams>> {
        // Get all cameras
        let cameras = self.get_all().await?;

        info!("Got cameras {}", cameras.len());

        // For each camera, get streams and references
        let mut result = Vec::new();
        for camera in cameras {
            if let Some(camera_with_streams) = self.get_with_streams_by_id(&camera.id).await? {
                result.push(camera_with_streams);
            }
        }

        Ok(result)
    }

    /// Get first active camera
    pub async fn get_first_active(&self) -> Result<Option<Camera>> {
        let result = sqlx::query_as::<_, Camera>(
            r#"
            SELECT * FROM cameras
            WHERE status = 'active'
            ORDER BY name
            LIMIT 1
            "#,
        )
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get first active camera: {}", e)))?;

        Ok(result)
    }

    /// Get first active camera with streams
    pub async fn get_first_active_with_streams(&self) -> Result<Option<CameraWithStreams>> {
        // Get first active camera
        let camera = self.get_first_active().await?;

        // If camera found, get its streams
        if let Some(camera) = camera {
            return self.get_with_streams_by_id(&camera.id).await;
        }

        Ok(None)
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

    /// Get camera streams
    pub async fn get_streams(&self, camera_id: &Uuid) -> Result<Vec<Stream>> {
        let result = sqlx::query_as::<_, Stream>(
            r#"
            SELECT * FROM streams
            WHERE camera_id = $1
            "#,
        )
        .bind(camera_id)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get camera streams: {}", e)))?;

        Ok(result)
    }

    /// Get camera stream by ID
    pub async fn get_stream_by_id(&self, stream_id: &Uuid) -> Result<Option<Stream>> {
        let result = sqlx::query_as::<_, Stream>(
            r#"
            SELECT * FROM streams
            WHERE id = $1
            "#,
        )
        .bind(stream_id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get stream by ID: {}", e)))?;

        Ok(result)
    }

    /// Update camera stream
    pub async fn update_stream(&self, stream: &Stream) -> Result<Stream> {
        // Prepare updated stream data
        let mut stream_db = stream.clone();
        stream_db.updated_at = Utc::now();

        let result = sqlx::query_as::<_, Stream>(
            r#"
            UPDATE streams
            SET name = $1, stream_type = $2, url = $3, resolution = $4, 
                width = $5, height = $6, codec = $7, profile = $8, level = $9,
                framerate = $10, bitrate = $11, variable_bitrate = $12,
                keyframe_interval = $13, quality_level = $14,
                transport_protocol = $15, authentication_required = $16,
                is_primary = $17, is_audio_enabled = $18, audio_codec = $19,
                audio_bitrate = $20, audio_channels = $21, audio_sample_rate = $22,
                is_active = $23, last_connected_at = $24, updated_at = $25
            WHERE id = $26 AND camera_id = $27
            RETURNING *
            "#,
        )
        .bind(&stream_db.name)
        .bind(&stream_db.stream_type.to_string()) // Convert enum to string
        .bind(&stream_db.url)
        .bind(&stream_db.resolution)
        .bind(stream_db.width)
        .bind(stream_db.height)
        .bind(&stream_db.codec)
        .bind(&stream_db.profile)
        .bind(&stream_db.level)
        .bind(stream_db.framerate)
        .bind(stream_db.bitrate)
        .bind(stream_db.variable_bitrate)
        .bind(stream_db.keyframe_interval)
        .bind(&stream_db.quality_level)
        .bind(&stream_db.transport_protocol)
        .bind(stream_db.authentication_required)
        .bind(stream_db.is_primary)
        .bind(stream_db.is_audio_enabled)
        .bind(&stream_db.audio_codec)
        .bind(stream_db.audio_bitrate)
        .bind(stream_db.audio_channels)
        .bind(stream_db.audio_sample_rate)
        .bind(stream_db.is_active)
        .bind(stream_db.last_connected_at)
        .bind(stream_db.updated_at)
        .bind(stream_db.id)
        .bind(stream_db.camera_id)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update stream: {}", e)))?;

        Ok(result)
    }

    /// Delete camera stream
    pub async fn delete_stream(&self, stream_id: &Uuid) -> Result<bool> {
        // Begin transaction
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(format!("Failed to begin transaction: {}", e)))?;

        // Delete stream references first
        sqlx::query(
            r#"
            DELETE FROM stream_references
            WHERE stream_id = $1
            "#,
        )
        .bind(stream_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Database(format!("Failed to delete stream references: {}", e)))?;

        // Delete stream
        let result = sqlx::query(
            r#"
            DELETE FROM streams
            WHERE id = $1
            "#,
        )
        .bind(stream_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Database(format!("Failed to delete stream: {}", e)))?;

        // Commit transaction
        tx.commit()
            .await
            .map_err(|e| Error::Database(format!("Failed to commit transaction: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }

    /// Update camera stream status
    pub async fn update_stream_status(&self, stream_id: &Uuid, is_active: bool) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE streams
            SET is_active = $1, updated_at = $2
            WHERE id = $3
            "#,
        )
        .bind(is_active)
        .bind(Utc::now())
        .bind(stream_id)
        .execute(&*self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update stream status: {}", e)))?;

        Ok(())
    }
}
