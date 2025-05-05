use crate::api::rest::AppState;
use crate::db::models::recording_models::{RecordingEventType, RecordingSearchQuery};
use crate::db::repositories::cameras::CamerasRepository;
use crate::db::repositories::recordings::RecordingsRepository;
use crate::security::auth::AuthService;
use axum::body::StreamBody;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Duration, TimeZone, Utc};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::File;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

/// Timeline API state
#[derive(Clone)]
pub struct TimelineApiState {
    pub recordings_repo: RecordingsRepository,
    pub cameras_repo: CamerasRepository,
    pub auth_service: Arc<AuthService>,
}

/// Timeline query parameters
#[derive(Debug, Deserialize)]
pub struct TimelineParams {
    pub camera_id: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub event_type: Option<String>,
    pub include_segments: Option<bool>,
}

/// Timeline segment response
#[derive(Debug, Serialize)]
pub struct TimelineSegment {
    pub id: String,
    pub camera_id: String,
    pub stream_id: String,
    pub start_time: String,
    pub end_time: Option<String>,
    pub duration: u64,
    pub event_type: String,
    pub file_path: String,
    pub is_segment: bool,
    pub parent_id: Option<String>,
    pub segment_id: Option<u32>,
}

/// Timeline response with all segments
#[derive(Debug, Serialize)]
pub struct TimelineResponse {
    pub start_time: String,
    pub end_time: String,
    pub total_duration: u64,
    pub camera_id: String,
    pub camera_name: String,
    pub segments: Vec<TimelineSegment>,
}

/// Helper function to convert AppState to TimelineApiState
pub fn app_state_to_timeline_state(app_state: &AppState) -> TimelineApiState {
    TimelineApiState {
        recordings_repo: RecordingsRepository::new(Arc::clone(&app_state.db_pool)),
        cameras_repo: CamerasRepository::new(Arc::clone(&app_state.db_pool)),
        auth_service: Arc::clone(&app_state.auth_service),
    }
}

/// Create recording timeline router
pub fn create_router<S: Clone + Send + Sync + 'static>(_state: S) -> Router<AppState> {
    Router::new()
        .route("/timeline", get(get_recording_timeline))
        .route("/playback/:recording_id", get(get_recording_playback_info))
        .route("/recordings_by_date", get(get_recordings_by_date))
        .route("/segments/:parent_id", get(get_recording_segments))
        .route("/video/:recording_id", get(get_video_recording))
}

pub async fn get_video_recording(
    Path(recording_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("TRiggering video recordings...........................................................................");
    // Parse recording ID
    let uuid = match Uuid::parse_str(&recording_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid recording ID").into_response(),
    };

    let recording = match state.recordings_repo.get_by_id(&uuid).await {
        Ok(Some(recording)) => recording,
        Ok(None) => return (StatusCode::NOT_FOUND, "Recording not found").into_response(),
        Err(e) => {
            error!("Error fetching recording: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Server error").into_response();
        }
    };

    // Here you would load the file and return it
    let path = recording.file_path;
    match tokio::fs::File::open(&path).await {
        Ok(file) => {
            let stream = ReaderStream::new(file);
            let body = StreamBody::new(stream);

            let headers = HeaderMap::from_iter([
                (header::CONTENT_TYPE, "video/mp4".parse().unwrap()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}.mp4\"", recording_id)
                        .parse()
                        .unwrap(),
                ),
            ]);

            (StatusCode::OK, headers, body).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "Video recording not found").into_response(),
    }
}
/// Get timeline data for a camera over a specific time period
pub async fn get_recording_timeline(
    Query(params): Query<TimelineParams>,
    State(state): State<AppState>,
) -> Result<Json<TimelineResponse>, StatusCode> {
    // Convert AppState to TimelineApiState
    let state = app_state_to_timeline_state(&state);

    // Parse camera ID
    let camera_id = match Uuid::parse_str(&params.camera_id) {
        Ok(id) => id,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Get the camera info
    let camera = match state.cameras_repo.get_by_id(&camera_id).await {
        Ok(Some(camera)) => camera,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Error fetching camera: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Determine time range (default to last 24 hours if not specified)
    let end_time = match params.end_time {
        Some(end_str) => match DateTime::parse_from_rfc3339(&end_str) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => Utc::now(),
        },
        None => Utc::now(),
    };

    let start_time = match params.start_time {
        Some(start_str) => match DateTime::parse_from_rfc3339(&start_str) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => end_time - Duration::days(1),
        },
        None => end_time - Duration::days(1),
    };

    // Parse event type if provided
    let event_types = match params.event_type {
        Some(ref type_str) => match type_str.to_lowercase().as_str() {
            "continuous" => Some(vec![RecordingEventType::Continuous]),
            "motion" => Some(vec![RecordingEventType::Motion]),
            "audio" => Some(vec![RecordingEventType::Audio]),
            "external" => Some(vec![RecordingEventType::External]),
            "manual" => Some(vec![RecordingEventType::Manual]),
            "analytics" => Some(vec![RecordingEventType::Analytics]),
            _ => None,
        },
        None => None,
    };

    // Create search query
    let mut query = RecordingSearchQuery {
        camera_ids: Some(vec![camera_id]),
        stream_ids: None,
        start_time: Some(start_time),
        end_time: Some(end_time),
        event_types,
        schedule_id: None,
        min_duration: None,
        segment_id: None,
        parent_recording_id: None,
        is_segment: match params.include_segments {
            Some(true) => None,                // Include both segments and parents
            Some(false) | None => Some(false), // Only parent recordings by default
        },
        limit: Some(1000), // Higher limit for timeline view
        offset: Some(0),
    };

    // Execute search
    let recordings = match state.recordings_repo.search(&query).await {
        Ok(recordings) => recordings,
        Err(e) => {
            error!("Error searching recordings: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Convert to timeline segments
    let mut segments = Vec::new();
    let mut total_duration: u64 = 0;
    for recording in recordings {
        // Skip invalid recordings
        if recording.duration == 0 || recording.file_path.to_str().is_none() {
            continue;
        }

        let is_segment = recording.parent_recording_id.is_some();

        // Exclude segments if requested
        if params.include_segments == Some(false) && is_segment {
            continue;
        }

        let segment = TimelineSegment {
            id: recording.id.to_string(),
            camera_id: recording.camera_id.to_string(),
            stream_id: recording.stream_id.to_string(),
            start_time: recording.start_time.to_rfc3339(),
            end_time: recording.end_time.map(|dt| dt.to_rfc3339()),
            duration: recording.duration as u64,
            event_type: recording.event_type.to_string(),
            file_path: recording.file_path.to_string_lossy().to_string(),
            is_segment,
            parent_id: recording.parent_recording_id.map(|id| id.to_string()),
            segment_id: recording.segment_id,
        };

        total_duration += recording.duration as u64;
        segments.push(segment);
    }

    // Create timeline response
    let response = TimelineResponse {
        start_time: start_time.to_rfc3339(),
        end_time: end_time.to_rfc3339(),
        total_duration,
        camera_id: camera_id.to_string(),
        camera_name: camera.name.clone(),
        segments,
    };

    Ok(Json(response))
}

/// Get detailed information about a specific recording for playback
pub async fn get_recording_playback_info(
    Path(recording_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<HashMap<String, serde_json::Value>>, StatusCode> {
    // Convert AppState to TimelineApiState
    let state = app_state_to_timeline_state(&state);

    // Parse recording ID
    let uuid = match Uuid::parse_str(&recording_id) {
        Ok(id) => id,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Get recording details
    let recording = match state.recordings_repo.get_by_id(&uuid).await {
        Ok(Some(recording)) => recording,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Error fetching recording: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Get camera details
    let camera = match state.cameras_repo.get_by_id(&recording.camera_id).await {
        Ok(Some(camera)) => camera,
        Ok(None) => {
            error!("Camera not found for recording: {}", recording.id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            error!("Error fetching camera: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Check if file exists
    let file_path = recording.file_path.clone();
    if !file_path.exists() {
        error!("Recording file not found: {}", file_path.display());
        return Err(StatusCode::NOT_FOUND);
    }

    // Build response with all needed info
    let mut response = HashMap::new();
    response.insert(
        "id".to_string(),
        serde_json::json!(recording.id.to_string()),
    );
    response.insert(
        "camera_id".to_string(),
        serde_json::json!(recording.camera_id.to_string()),
    );
    response.insert("camera_name".to_string(), serde_json::json!(camera.name));
    response.insert(
        "stream_id".to_string(),
        serde_json::json!(recording.stream_id.to_string()),
    );
    response.insert(
        "start_time".to_string(),
        serde_json::json!(recording.start_time.to_rfc3339()),
    );

    if let Some(end_time) = recording.end_time {
        response.insert(
            "end_time".to_string(),
            serde_json::json!(end_time.to_rfc3339()),
        );
    }

    response.insert(
        "duration".to_string(),
        serde_json::json!(recording.duration),
    );
    response.insert(
        "file_size".to_string(),
        serde_json::json!(recording.file_size),
    );
    response.insert("format".to_string(), serde_json::json!(recording.format));
    response.insert(
        "resolution".to_string(),
        serde_json::json!(recording.resolution),
    );
    response.insert("fps".to_string(), serde_json::json!(recording.fps));
    response.insert(
        "event_type".to_string(),
        serde_json::json!(recording.event_type.to_string()),
    );
    response.insert(
        "file_path".to_string(),
        serde_json::json!(recording.file_path.to_string_lossy().to_string()),
    );

    if let Some(segment_id) = recording.segment_id {
        response.insert("segment_id".to_string(), serde_json::json!(segment_id));
    }

    if let Some(parent_id) = recording.parent_recording_id {
        response.insert(
            "parent_recording_id".to_string(),
            serde_json::json!(parent_id.to_string()),
        );
    }

    // Include metadata if available
    if let Some(metadata) = recording.metadata {
        response.insert("metadata".to_string(), metadata);
    }

    Ok(Json(response))
}

/// Get recordings grouped by date (for calendar view)
pub async fn get_recordings_by_date(
    Query(params): Query<TimelineParams>,
    State(state): State<AppState>,
) -> Result<Json<HashMap<String, serde_json::Value>>, StatusCode> {
    // Convert AppState to TimelineApiState
    let state = app_state_to_timeline_state(&state);

    // Parse camera ID
    let camera_id = match Uuid::parse_str(&params.camera_id) {
        Ok(id) => id,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Determine time range (default to last 30 days if not specified)
    let end_time = match params.end_time {
        Some(end_str) => match DateTime::parse_from_rfc3339(&end_str) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => Utc::now(),
        },
        None => Utc::now(),
    };

    let start_time = match params.start_time {
        Some(start_str) => match DateTime::parse_from_rfc3339(&start_str) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => end_time - Duration::days(30),
        },
        None => end_time - Duration::days(30),
    };

    // Create search query for non-segment recordings (parent recordings only)
    let query = RecordingSearchQuery {
        camera_ids: Some(vec![camera_id]),
        stream_ids: None,
        start_time: Some(start_time),
        end_time: Some(end_time),
        event_types: None, // All event types
        schedule_id: None,
        min_duration: None,
        segment_id: None,
        parent_recording_id: None,
        is_segment: Some(false), // Only parent recordings
        limit: Some(1000),
        offset: Some(0),
    };

    // Execute search
    let recordings = match state.recordings_repo.search(&query).await {
        Ok(recordings) => recordings,
        Err(e) => {
            error!("Error searching recordings: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Group recordings by date (YYYY-MM-DD)
    let mut recordings_by_date: HashMap<String, Vec<serde_json::Value>> = HashMap::new();

    for recording in recordings {
        let date_key = recording.start_time.format("%Y-%m-%d").to_string();

        let recording_json = serde_json::json!({
            "id": recording.id.to_string(),
            "start_time": recording.start_time.to_rfc3339(),
            "end_time": recording.end_time.map(|dt| dt.to_rfc3339()),
            "duration": recording.duration,
            "event_type": recording.event_type.to_string(),
            "file_size": recording.file_size,
        });

        recordings_by_date
            .entry(date_key)
            .or_insert_with(Vec::new)
            .push(recording_json);
    }

    // Build final response
    let mut response = HashMap::new();
    response.insert(
        "camera_id".to_string(),
        serde_json::json!(camera_id.to_string()),
    );
    response.insert(
        "start_date".to_string(),
        serde_json::json!(start_time.format("%Y-%m-%d").to_string()),
    );
    response.insert(
        "end_date".to_string(),
        serde_json::json!(end_time.format("%Y-%m-%d").to_string()),
    );

    // Convert recordings_by_date into JSON
    let dates_json = serde_json::json!(recordings_by_date);
    response.insert("dates".to_string(), dates_json);

    Ok(Json(response))
}

/// Get all segments for a parent recording
pub async fn get_recording_segments(
    Path(parent_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Vec<TimelineSegment>>, StatusCode> {
    // Convert AppState to TimelineApiState
    let state = app_state_to_timeline_state(&state);

    // Parse parent recording ID
    let parent_uuid = match Uuid::parse_str(&parent_id) {
        Ok(id) => id,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Create search query for segments of this parent
    let query = RecordingSearchQuery {
        camera_ids: None,
        stream_ids: None,
        start_time: None,
        end_time: None,
        event_types: None,
        schedule_id: None,
        min_duration: None,
        segment_id: None,
        parent_recording_id: Some(parent_uuid),
        is_segment: Some(true), // Only segments
        limit: Some(1000),
        offset: Some(0),
    };

    // Execute search
    let segments = match state.recordings_repo.search(&query).await {
        Ok(recordings) => recordings,
        Err(e) => {
            error!("Error searching for segments: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Convert to timeline segments
    let segments = segments
        .into_iter()
        .map(|recording| TimelineSegment {
            id: recording.id.to_string(),
            camera_id: recording.camera_id.to_string(),
            stream_id: recording.stream_id.to_string(),
            start_time: recording.start_time.to_rfc3339(),
            end_time: recording.end_time.map(|dt| dt.to_rfc3339()),
            duration: recording.duration as u64,
            event_type: recording.event_type.to_string(),
            file_path: recording.file_path.to_string_lossy().to_string(),
            is_segment: true,
            parent_id: Some(parent_uuid.to_string()),
            segment_id: recording.segment_id,
        })
        .collect();

    Ok(Json(segments))
}

