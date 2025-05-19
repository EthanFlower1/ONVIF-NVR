use crate::api::rest::AppState;
use crate::db::models::recording_models::{RecordingEventType, RecordingSearchQuery};
use crate::db::repositories::cameras::CamerasRepository;
use crate::db::repositories::recordings::RecordingsRepository;
use crate::recorder::record::{RecordingManager, RecordingStatus};
use crate::security::auth::AuthService;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{delete, get, post};
use axum::Router;
use chrono::Utc;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Recording API state
#[derive(Clone)]
pub struct RecordingApiState {
    pub recording_manager: Arc<RecordingManager>,
    pub recordings_repo: RecordingsRepository,
    pub cameras_repo: CamerasRepository,
    pub auth_service: Arc<AuthService>,
}

/// Request for starting a recording
#[derive(Debug, Deserialize)]
pub struct StartRecordingRequest {
    pub event_type: Option<String>,
}

/// Response for recording operations
#[derive(Debug, Serialize)]
pub struct RecordingResponse {
    pub recording_id: Option<Uuid>,
    pub status: String,
    pub message: String,
}

/// Response for recording status
#[derive(Debug, Serialize)]
pub struct RecordingStatusResponse {
    pub recordings: Vec<RecordingStatusItem>,
}

/// Individual recording status
#[derive(Debug, Serialize)]
pub struct RecordingStatusItem {
    pub recording_id: Uuid,
    pub camera_id: Uuid,
    pub stream_id: Uuid,
    pub start_time: String,
    pub duration_seconds: i64,
    pub file_size_bytes: u64,
    pub state: String,
    pub fps: i32,
    pub event_type: String,
    pub segment_id: Option<u32>,
    pub parent_recording_id: Option<Uuid>,
}

/// Search query parameters
#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub camera_id: Option<String>,
    pub stream_id: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub event_type: Option<String>,
    pub segment_id: Option<u32>,
    pub parent_recording_id: Option<String>,
    pub is_segment: Option<bool>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Response for manual cleanup
#[derive(Debug, Serialize)]
pub struct CleanupResponse {
    pub deleted_count: u64,
    pub status: String,
}

/// Helper function to convert AppState to RecordingApiState
pub fn app_state_to_recording_state(app_state: &AppState) -> RecordingApiState {
    RecordingApiState {
        recording_manager: Arc::clone(&app_state.recording_manager),
        recordings_repo: RecordingsRepository::new(Arc::clone(&app_state.db_pool)),
        cameras_repo: CamerasRepository::new(Arc::clone(&app_state.db_pool)),
        auth_service: Arc::clone(&app_state.auth_service),
    }
}

/// Create recording controller router with AppState
pub fn create_router<S: Clone + Send + Sync + 'static>(_state: S) -> Router<AppState> {
    Router::new()
        .route("/start/:camera_id/:stream_id", post(start_recording))
        .route("/start/:camera_id", post(start_primary_recording))
        .route("/stop/:camera_id/:stream_id", post(stop_recording))
        .route("/stop/:camera_id", post(stop_primary_recording))
        .route("/status/:camera_id/:stream_id", get(get_recording_status))
        .route("/status/:camera_id", get(get_camera_recording_status))
        .route("/status", get(get_all_recording_status))
        .route("/search", get(search_recordings))
        .route("/prune/:camera_id", delete(prune_recordings))
}

/// Start recording for a specific camera and stream
pub async fn start_recording(
    Path((camera_id, stream_id)): Path<(String, String)>,
    State(state): State<AppState>,
    Json(request): Json<StartRecordingRequest>,
) -> Result<Json<RecordingResponse>, StatusCode> {
    // Convert AppState to RecordingApiState
    let state = app_state_to_recording_state(&state);
    // Parse UUIDs
    let camera_uuid = Uuid::parse_str(&camera_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let stream_uuid = Uuid::parse_str(&stream_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Get the stream
    let stream = state
        .cameras_repo
        .get_stream_by_id(&stream_uuid)
        .await
        .map_err(|e| {
            error!("Failed to get stream: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Check that the stream belongs to the camera
    if stream.camera_id != camera_uuid {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Determine event type
    let event_type = match &request.event_type {
        Some(event_type_str) => match event_type_str.to_lowercase().as_str() {
            "motion" => RecordingEventType::Motion,
            "audio" => RecordingEventType::Audio,
            "external" => RecordingEventType::External,
            "analytics" => RecordingEventType::Analytics,
            _ => RecordingEventType::Manual,
        },
        None => RecordingEventType::Manual,
    };

    // Start recording based on event type
    let recording_id = if event_type == RecordingEventType::Manual {
        state
            .recording_manager
            .start_manual_recording(&stream)
            .await
    } else {
        state
            .recording_manager
            .start_event_recording(&stream, event_type)
            .await
    };

    match recording_id {
        Ok(id) => {
            info!(
                "Started recording {} for camera {}, stream {}",
                id, camera_id, stream_id
            );

            Ok(Json(RecordingResponse {
                recording_id: Some(id),
                status: "success".to_string(),
                message: format!("Recording started with ID {}", id),
            }))
        }
        Err(e) => {
            error!("Failed to start recording: {}", e);

            Ok(Json(RecordingResponse {
                recording_id: None,
                status: "error".to_string(),
                message: format!("Failed to start recording: {}", e),
            }))
        }
    }
}

/// Start recording for a camera's primary stream
pub async fn start_primary_recording(
    Path(camera_id): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<StartRecordingRequest>,
) -> Result<Json<RecordingResponse>, StatusCode> {
    // Convert AppState to RecordingApiState
    let state = app_state_to_recording_state(&state);
    // Parse camera UUID
    let camera_uuid = Uuid::parse_str(&camera_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Get the camera
    let camera = state
        .cameras_repo
        .get_by_id(&camera_uuid)
        .await
        .map_err(|e| {
            error!("Failed to get camera: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Check if camera has a primary stream
    let primary_stream_id = camera.primary_stream_id.ok_or(StatusCode::BAD_REQUEST)?;

    // Get the primary stream
    let stream = state
        .cameras_repo
        .get_stream_by_id(&primary_stream_id)
        .await
        .map_err(|e| {
            error!("Failed to get stream: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Determine event type
    let event_type = match &request.event_type {
        Some(event_type_str) => match event_type_str.to_lowercase().as_str() {
            "motion" => RecordingEventType::Motion,
            "audio" => RecordingEventType::Audio,
            "external" => RecordingEventType::External,
            "analytics" => RecordingEventType::Analytics,
            _ => RecordingEventType::Manual,
        },
        None => RecordingEventType::Manual,
    };

    // Start recording based on event type
    let recording_id = if event_type == RecordingEventType::Manual {
        state
            .recording_manager
            .start_manual_recording(&stream)
            .await
    } else {
        state
            .recording_manager
            .start_event_recording(&stream, event_type)
            .await
    };

    match recording_id {
        Ok(id) => {
            info!(
                "Started recording {} for camera {} (primary stream)",
                id, camera_id
            );

            Ok(Json(RecordingResponse {
                recording_id: Some(id),
                status: "success".to_string(),
                message: format!("Recording started with ID {}", id),
            }))
        }
        Err(e) => {
            error!("Failed to start primary recording: {}", e);

            Ok(Json(RecordingResponse {
                recording_id: None,
                status: "error".to_string(),
                message: format!("Failed to start recording: {}", e),
            }))
        }
    }
}

/// Stop recording for a specific camera and stream
pub async fn stop_recording(
    Path((camera_id, stream_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<RecordingResponse>, StatusCode> {
    // Convert AppState to RecordingApiState
    let state = app_state_to_recording_state(&state);
    // Parse UUIDs
    let camera_uuid = Uuid::parse_str(&camera_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let stream_uuid = Uuid::parse_str(&stream_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Get recording status to see what's active
    let all_status = state.recording_manager.get_recording_status().await;

    // Find active recordings for this camera and stream
    let matching_recordings: Vec<&RecordingStatus> = all_status
        .iter()
        .filter(|s| s.camera_id == camera_uuid && s.stream_id == stream_uuid)
        .collect();

    if matching_recordings.is_empty() {
        return Ok(Json(RecordingResponse {
            recording_id: None,
            status: "warning".to_string(),
            message: "No active recordings found for this camera and stream".to_string(),
        }));
    }

    // Save the first recording ID to return later
    let first_recording_id = matching_recordings[0].recording_id;

    // Try to stop all matching recordings
    let mut success_count = 0;
    let mut error_message = String::new();

    for recording in &matching_recordings {
        match state
            .recording_manager
            .stop_recording_by_id(&recording.recording_id)
            .await
        {
            Ok(_) => {
                success_count += 1;
            }
            Err(e) => {
                error_message.push_str(&format!(
                    "Failed to stop recording {}: {}; ",
                    recording.recording_id, e
                ));
            }
        }
    }

    if success_count > 0 {
        Ok(Json(RecordingResponse {
            recording_id: Some(first_recording_id),
            status: "success".to_string(),
            message: format!("Stopped {} recordings", success_count),
        }))
    } else {
        Ok(Json(RecordingResponse {
            recording_id: None,
            status: "error".to_string(),
            message: error_message,
        }))
    }
}

/// Stop recording for a camera's primary stream
pub async fn stop_primary_recording(
    Path(camera_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<RecordingResponse>, StatusCode> {
    // Convert AppState to RecordingApiState
    // let recording_state = app_state_to_recording_state(&state);
    // Parse camera UUID
    let camera_uuid = Uuid::parse_str(&camera_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Get the camera
    let camera = state
        .cameras_repo
        .get_by_id(&camera_uuid)
        .await
        .map_err(|e| {
            error!("Failed to get camera: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Check if camera has a primary stream
    let primary_stream_id = camera.primary_stream_id.ok_or(StatusCode::BAD_REQUEST)?;

    // Call the stream-specific stop method with camera ID and stream ID
    let path_params = (camera_id, primary_stream_id.to_string());

    // Create the original app state to pass to the stop_recording function
    stop_recording(Path(path_params), State(state)).await
}

/// Get status of active recordings for a specific camera and stream
pub async fn get_recording_status(
    Path((camera_id, stream_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<RecordingStatusResponse>, StatusCode> {
    // Convert AppState to RecordingApiState
    let state = app_state_to_recording_state(&state);
    // Parse UUIDs
    let camera_uuid = Uuid::parse_str(&camera_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let stream_uuid = Uuid::parse_str(&stream_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Get all recording status
    let all_status = state.recording_manager.get_recording_status().await;

    // Filter by camera and stream
    let filtered_status: Vec<RecordingStatusItem> = all_status
        .into_iter()
        .filter(|s| s.camera_id == camera_uuid && s.stream_id == stream_uuid)
        .map(|status| RecordingStatusItem {
            recording_id: status.recording_id,
            camera_id: status.camera_id,
            stream_id: status.stream_id,
            start_time: status.start_time.to_rfc3339(),
            duration_seconds: status.duration,
            file_size_bytes: status.file_size,
            state: status.pipeline_state,
            fps: status.fps,
            event_type: format!("{:?}", status.event_type),
            segment_id: status.segment_id,
            parent_recording_id: status.parent_recording_id,
        })
        .collect();

    Ok(Json(RecordingStatusResponse {
        recordings: filtered_status,
    }))
}

/// Get status of all active recordings for a camera
pub async fn get_camera_recording_status(
    Path(camera_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<RecordingStatusResponse>, StatusCode> {
    // Convert AppState to RecordingApiState
    let state = app_state_to_recording_state(&state);
    // Parse camera UUID
    let camera_uuid = Uuid::parse_str(&camera_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Get all recording status
    let all_status = state.recording_manager.get_recording_status().await;

    // Filter by camera
    let filtered_status: Vec<RecordingStatusItem> = all_status
        .into_iter()
        .filter(|s| s.camera_id == camera_uuid)
        .map(|status| RecordingStatusItem {
            recording_id: status.recording_id,
            camera_id: status.camera_id,
            stream_id: status.stream_id,
            start_time: status.start_time.to_rfc3339(),
            duration_seconds: status.duration,
            file_size_bytes: status.file_size,
            state: status.pipeline_state,
            fps: status.fps,
            event_type: format!("{:?}", status.event_type),
            segment_id: status.segment_id,
            parent_recording_id: status.parent_recording_id,
        })
        .collect();

    Ok(Json(RecordingStatusResponse {
        recordings: filtered_status,
    }))
}

/// Get status of all active recordings
pub async fn get_all_recording_status(
    State(state): State<AppState>,
) -> Result<Json<RecordingStatusResponse>, StatusCode> {
    // Convert AppState to RecordingApiState
    let state = app_state_to_recording_state(&state);
    // Get all recording status
    let all_status = state.recording_manager.get_recording_status().await;

    // Convert to response format
    let status_items: Vec<RecordingStatusItem> = all_status
        .into_iter()
        .map(|status| RecordingStatusItem {
            recording_id: status.recording_id,
            camera_id: status.camera_id,
            stream_id: status.stream_id,
            start_time: status.start_time.to_rfc3339(),
            duration_seconds: status.duration,
            file_size_bytes: status.file_size,
            state: status.pipeline_state,
            fps: status.fps,
            event_type: format!("{:?}", status.event_type),
            segment_id: status.segment_id,
            parent_recording_id: status.parent_recording_id,
        })
        .collect();

    Ok(Json(RecordingStatusResponse {
        recordings: status_items,
    }))
}

/// Search recordings
pub async fn search_recordings(
    Query(params): Query<SearchParams>,
    State(state): State<AppState>,
) -> Result<Json<HashMap<String, serde_json::Value>>, StatusCode> {
    // Convert AppState to RecordingApiState
    let state = app_state_to_recording_state(&state);
    // Build search query
    let mut query = RecordingSearchQuery {
        camera_ids: None,
        stream_ids: None,
        start_time: None,
        end_time: None,
        event_types: None,
        schedule_id: None,
        min_duration: None,
        segment_id: params.segment_id,
        parent_recording_id: None,
        is_segment: params.is_segment,
        limit: params.limit,
        offset: params.offset,
    };

    // Parse camera ID if provided
    if let Some(camera_id_str) = params.camera_id {
        if let Ok(camera_id) = Uuid::parse_str(&camera_id_str) {
            query.camera_ids = Some(vec![camera_id]);
        }
    }

    // Parse stream ID if provided
    if let Some(stream_id_str) = params.stream_id {
        if let Ok(stream_id) = Uuid::parse_str(&stream_id_str) {
            query.stream_ids = Some(vec![stream_id]);
        }
    }

    // Parse parent recording ID if provided
    if let Some(parent_id_str) = params.parent_recording_id {
        if let Ok(parent_id) = Uuid::parse_str(&parent_id_str) {
            query.parent_recording_id = Some(parent_id);
        }
    }

    // Parse start time if provided
    if let Some(start_time_str) = params.start_time {
        if let Ok(start_time) = chrono::DateTime::parse_from_rfc3339(&start_time_str) {
            query.start_time = Some(start_time.with_timezone(&Utc));
        }
    }

    // Parse end time if provided
    if let Some(end_time_str) = params.end_time {
        if let Ok(end_time) = chrono::DateTime::parse_from_rfc3339(&end_time_str) {
            query.end_time = Some(end_time.with_timezone(&Utc));
        }
    }

    // Parse event type if provided
    if let Some(event_type_str) = params.event_type {
        let event_type = match event_type_str.to_lowercase().as_str() {
            "continuous" => RecordingEventType::Continuous,
            "motion" => RecordingEventType::Motion,
            "audio" => RecordingEventType::Audio,
            "external" => RecordingEventType::External,
            "manual" => RecordingEventType::Manual,
            "analytics" => RecordingEventType::Analytics,
            _ => return Err(StatusCode::BAD_REQUEST),
        };

        query.event_types = Some(vec![event_type]);
    }

    // Execute search query
    let recordings = state.recordings_repo.search(&query).await.map_err(|e| {
        error!("Failed to search recordings: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Convert to response format (using serde_json for simplicity)
    let mut response = HashMap::new();
    response.insert("count".to_string(), serde_json::json!(recordings.len()));

    // Convert recordings to JSON
    let recordings_json =
        serde_json::to_value(&recordings).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    response.insert("recordings".to_string(), recordings_json);

    Ok(Json(response))
}

/// Prune recordings for a camera
pub async fn prune_recordings(
    Path(camera_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Json<CleanupResponse>, StatusCode> {
    // Convert AppState to RecordingApiState
    let state = app_state_to_recording_state(&state);
    // Parse camera UUID
    let camera_uuid = Uuid::parse_str(&camera_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Parse older_than_days parameter if provided
    let older_than_days = params
        .get("older_than_days")
        .and_then(|s| s.parse::<i32>().ok());

    // Prune recordings
    let deleted_count = state
        .recordings_repo
        .prune_recordings_by_camera(&camera_uuid, older_than_days)
        .await
        .map_err(|e| {
            error!("Failed to prune recordings: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(CleanupResponse {
        deleted_count,
        status: "success".to_string(),
    }))
}
