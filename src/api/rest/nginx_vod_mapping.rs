use crate::api::rest::AppState;
use crate::db::models::recording_models::RecordingSearchQuery;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use chrono::{DateTime, Utc};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Parameters for NGINX VOD mapping API
#[derive(Debug, Deserialize)]
pub struct NginxVodMappingParams {
    /// Camera ID for camera-wide playlists
    pub camera_id: Option<String>,
    /// Recording ID for single recording
    pub recording_id: Option<String>,
    /// Optional start time filter
    pub start_time: Option<String>,
    /// Optional end time filter
    pub end_time: Option<String>,
}

/// Response structure for NGINX VOD mapping API
#[derive(Debug, Serialize)]
pub struct VodMappingResponse {
    pub sequences: Vec<VodSequence>,
}

#[derive(Debug, Serialize)]
pub struct VodSequence {
    pub clips: Vec<VodClip>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VodClip {
    #[serde(rename = "type")]
    pub clip_type: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_from: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_to: Option<u64>,
}

/// Generate mapping information for NGINX VOD module
pub async fn generate_vod_mapping(
    Query(params): Query<NginxVodMappingParams>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("VOD mapping request: {:?}", params);

    if let Some(camera_id) = &params.camera_id {
        // Camera-wide mapping - all recordings from a single camera
        match generate_camera_mapping(camera_id, &params, &state).await {
            Ok(mapping) => (StatusCode::OK, Json(mapping)),
            Err(e) => {
                error!("Error generating camera mapping: {}", e);
                let error_response = VodMappingResponse {
                    sequences: vec![], // Empty sequences for error
                };
                (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response))
            }
        }
    } else if let Some(recording_id) = &params.recording_id {
        // Single recording mapping
        match generate_recording_mapping(recording_id, &params, &state).await {
            Ok(mapping) => (StatusCode::OK, Json(mapping)),
            Err(e) => {
                error!("Error generating recording mapping: {}", e);
                let error_response = VodMappingResponse {
                    sequences: vec![], // Empty sequences for error
                };
                (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response))
            }
        }
    } else {
        // Neither camera_id nor recording_id provided
        let error_response = VodMappingResponse {
            sequences: vec![], // Empty sequences for error
        };
        (
            StatusCode::BAD_REQUEST,
            Json(error_response)
        )
    }
}

/// Generate mapping for all recordings from a specific camera
async fn generate_camera_mapping(
    camera_id: &str,
    params: &NginxVodMappingParams,
    state: &AppState,
) -> Result<VodMappingResponse, anyhow::Error> {
    // Parse camera UUID
    let camera_uuid = Uuid::parse_str(camera_id)?;
    
    // Prepare search query
    let start_time = params.start_time.as_ref().and_then(|s| DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc)));
    let end_time = params.end_time.as_ref().and_then(|s| DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc)));
    
    let query = RecordingSearchQuery {
        camera_ids: Some(vec![camera_uuid]),
        stream_ids: None,
        start_time: start_time,
        end_time: end_time,
        event_types: None,
        schedule_id: None,
        min_duration: Some(1), // Exclude 0-duration recordings
        segment_id: None,
        parent_recording_id: None,
        is_segment: None,
        limit: None, // Get all recordings
        offset: None,
    };
    
    // Get recordings for this camera
    let recordings = state.recordings_repo.search(&query).await?;
    
    // Filter recordings with existing files
    let valid_recordings: Vec<_> = recordings
        .into_iter()
        .filter(|r| r.file_path.exists() && r.end_time.is_some())
        .collect();
        
    info!("Found {} valid recordings for camera {}", valid_recordings.len(), camera_id);
    
    if valid_recordings.is_empty() {
        return Err(anyhow::anyhow!("No valid recordings found for camera {}", camera_id));
    }
    
    // Sort recordings chronologically
    let mut sorted_recordings = valid_recordings;
    sorted_recordings.sort_by(|a, b| a.start_time.cmp(&b.start_time));
    
    // Create clips from sorted recordings
    let clips = sorted_recordings.into_iter().map(|recording| {
        // Get the file path - either index.mp4 in the recording directory or original file
        let recordings_base = std::env::var("RECORDINGS_PATH").unwrap_or_else(|_| "/app/recordings".to_string());
        let recording_dir = PathBuf::from(recordings_base)
            .join(recording.id.to_string());
        
        let index_path = recording_dir.join("index.mp4");
        let file_path = if index_path.exists() {
            index_path
        } else {
            recording.file_path
        };
        
        VodClip {
            clip_type: "source".to_string(),
            path: file_path.to_string_lossy().to_string(),
            clip_from: None,
            clip_to: None,
        }
    }).collect();
    
    // Create the sequences and response
    let sequence = VodSequence {
        clips,
        language: None,
        label: Some(format!("Camera {}", camera_id)),
        id: Some(camera_id.to_string()),
    };
    
    Ok(VodMappingResponse {
        sequences: vec![sequence],
    })
}

/// Generate mapping for a single recording
async fn generate_recording_mapping(
    recording_id: &str,
    _params: &NginxVodMappingParams,
    state: &AppState,
) -> Result<VodMappingResponse, anyhow::Error> {
    // Parse recording UUID
    let uuid = Uuid::parse_str(recording_id)?;
    
    // Get recording details
    let recording = match state.recordings_repo.get_by_id(&uuid).await? {
        Some(recording) => recording,
        None => return Err(anyhow::anyhow!("Recording not found: {}", recording_id)),
    };
    
    // Get the file path - either index.mp4 in the recording directory or original file
    let recordings_base = std::env::var("RECORDINGS_PATH").unwrap_or_else(|_| "/app/recordings".to_string());
    let recording_dir = PathBuf::from(recordings_base)
        .join(recording_id);
    
    let index_path = recording_dir.join("index.mp4");
    let file_path = if index_path.exists() {
        index_path
    } else {
        recording.file_path
    };
    
    // Check if file exists
    if !file_path.exists() {
        return Err(anyhow::anyhow!("Recording file not found: {}", file_path.display()));
    }
    
    // Create a clip from the recording
    let clip = VodClip {
        clip_type: "source".to_string(),
        path: file_path.to_string_lossy().to_string(),
        clip_from: None,
        clip_to: None,
    };
    
    // Create the sequence
    let sequence = VodSequence {
        clips: vec![clip],
        language: None, 
        label: Some(format!("Recording {}", recording_id)),
        id: Some(recording_id.to_string()),
    };
    
    Ok(VodMappingResponse {
        sequences: vec![sequence],
    })
}